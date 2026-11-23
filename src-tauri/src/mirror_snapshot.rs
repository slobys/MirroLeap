//! Window-scoped video capture for the two iOS magnifiers.
use std::{slice, thread, time::{Duration, Instant}};
use windows::{core::{factory, Interface}, Graphics::{
    SizeInt32,
    Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
    DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
}, Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::{Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP}, Direct3D11::*,
        Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS},
        Dxgi::{IDXGIAdapter, IDXGIDevice}, Gdi::ClientToScreen},
    System::WinRT::{Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
        Graphics::Capture::IGraphicsCaptureItemInterop, RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
    UI::{HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
        WindowsAndMessaging::{GetClientRect, GetWindowRect}},
}};

fn client_box(hwnd:HWND, size:SizeInt32) -> Result<RECT, String> {
    let mut client = RECT::default();
    let mut screen_origin = POINT::default();
    let mut visible_frame = RECT::default();
    unsafe {
        GetClientRect(hwnd, &mut client).map_err(|e| e.to_string())?;
        let mut screen_end = POINT { x: client.right, y: client.bottom };
        if !ClientToScreen(hwnd, &mut screen_origin).as_bool() || !ClientToScreen(hwnd, &mut screen_end).as_bool() {
            return Err("无法定位投屏画面".into());
        }
        if DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut visible_frame as *mut RECT).cast(), std::mem::size_of::<RECT>() as u32).is_err(){
            GetWindowRect(hwnd,&mut visible_frame).map_err(|e|e.to_string())?;
        }
        let width=screen_end.x-screen_origin.x;
        let height=screen_end.y-screen_origin.y;
        let left=(screen_origin.x-visible_frame.left).clamp(0,(size.Width-width).max(0));
        let top=(screen_origin.y-visible_frame.top).clamp(0,(size.Height-height).max(0));
        let right=(left+width).min(size.Width);
        let bottom=(top+height).min(size.Height);
        if width<=0 || height<=0 || right-left<width-8 || bottom-top<height-8 ||
            i64::from(width)*i64::from(height)>16_000_000{
            return Err("请在投屏画面尺寸稳定后重试放大镜".into());
        }
        Ok(RECT{left,top,right,bottom})
    }
}

struct Apartment(bool);
impl Drop for Apartment {
    fn drop(&mut self){if self.0{unsafe{RoUninitialize();}}}
}

pub(crate) struct WindowCapture {
    pool:Direct3D11CaptureFramePool,
    session:GraphicsCaptureSession,
    winrt_device:IDirect3DDevice,
    device:ID3D11Device,
    context:ID3D11DeviceContext,
    hwnd:HWND,
    size:SizeInt32,
    client:RECT,
    texture:Option<ID3D11Texture2D>,
    staging:Option<((u32,u32),ID3D11Texture2D)>,
    _apartment:Apartment,
}

impl WindowCapture {
    const FORMAT:DirectXPixelFormat=DirectXPixelFormat::B8G8R8A8UIntNormalized;

    pub(crate) fn open(handle:usize)->Result<Self,String>{
        let apartment=match unsafe{RoInitialize(RO_INIT_MULTITHREADED)} {
            Ok(())=>Apartment(true),
            Err(e) if e.code().0==0x80010106_u32 as i32=>Apartment(false),
            Err(e)=>return Err(e.to_string()),
        };
        let error=|e:windows::core::Error|e.to_string();
        if !GraphicsCaptureSession::IsSupported().map_err(error)?{
            return Err("此 Windows 设备不支持捕获投屏画面".into());
        }
        let hwnd=HWND(handle as *mut _);
        let item:GraphicsCaptureItem=unsafe{
            factory::<GraphicsCaptureItem,IGraphicsCaptureItemInterop>().and_then(|api|api.CreateForWindow(hwnd))
        }.map_err(error)?;
        let size=item.Size().map_err(error)?;
        let client=client_box(hwnd,size)?;
        let mut device=None;
        let mut context=None;
        let mut create=|driver|unsafe{D3D11CreateDevice(None::<&IDXGIAdapter>,driver,Default::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,None,D3D11_SDK_VERSION,Some(&mut device),None,Some(&mut context))};
        if create(D3D_DRIVER_TYPE_HARDWARE).is_err(){create(D3D_DRIVER_TYPE_WARP).map_err(error)?;}
        let device=device.ok_or("无法创建投屏捕获设备")?;
        let context=context.ok_or("无法创建投屏捕获上下文")?;
        let dxgi=device.cast::<IDXGIDevice>().map_err(error)?;
        let winrt_device:IDirect3DDevice=unsafe{CreateDirect3D11DeviceFromDXGIDevice(&dxgi)}
            .and_then(|value|value.cast()).map_err(error)?;
        let pool=Direct3D11CaptureFramePool::CreateFreeThreaded(&winrt_device,Self::FORMAT,2,size).map_err(error)?;
        let session=pool.CreateCaptureSession(&item).map_err(error)?;
        let capture=Self{pool,session,winrt_device,device,context,hwnd,size,client,texture:None,staging:None,_apartment:apartment};
        let _=capture.session.SetIsCursorCaptureEnabled(false);
        capture.session.StartCapture().map_err(error)?;
        Ok(capture)
    }

    pub(crate) fn poll(&mut self)->Result<bool,String>{
        // Check out one surface at a time. A second poll is unnecessary at
        // the lens's native update cadence and pins another pool surface.
        let Ok(frame)=self.pool.TryGetNextFrame() else{return Ok(false);};
        let error=|e:windows::core::Error|e.to_string();
        let actual=frame.ContentSize().map_err(error)?;
        if actual!=self.size{
            let client=client_box(self.hwnd,actual)?;
            drop(frame);
            self.pool.Recreate(&self.winrt_device,Self::FORMAT,2,actual).map_err(error)?;
            self.client=client;
            self.size=actual;
            self.texture=None;
            return Ok(false);
        }
        let surface=frame.Surface().and_then(|s|s.cast::<IDirect3DDxgiInterfaceAccess>()).map_err(error)?;
        let source:ID3D11Texture2D=unsafe{surface.GetInterface()}.map_err(error)?;
        if self.texture.is_none(){
            let mut desc=D3D11_TEXTURE2D_DESC::default();
            unsafe{source.GetDesc(&mut desc);}
            desc.BindFlags=0;
            desc.MiscFlags=0;
            desc.CPUAccessFlags=0;
            self.texture=Some(make_texture(&self.device,&desc)?);
        }
        unsafe{self.context.CopyResource(self.texture.as_ref().unwrap(),&source);}
        Ok(true)
    }

    pub(crate) fn dimensions(&self)->(u32,u32){
        ((self.client.right-self.client.left) as u32,(self.client.bottom-self.client.top) as u32)
    }

    pub(crate) fn read(&mut self, crop:(u32,u32,u32,u32), bgra:&mut Vec<u8>)->Result<bool,String>{
        let Some(source)=self.texture.as_ref() else{return Ok(false);};
        let (x,y,width,height)=crop;
        let (client_width,client_height)=self.dimensions();
        if width==0 || height==0 || x.saturating_add(width)>client_width || y.saturating_add(height)>client_height {
            return Err("放大区域超出投屏画面".into());
        }
        if !self.staging.as_ref().is_some_and(|(size,_)|*size==(width,height)) {
            let mut desc=D3D11_TEXTURE2D_DESC::default();
            unsafe{source.GetDesc(&mut desc);}
            desc.Width=width;
            desc.Height=height;
            desc.Usage=D3D11_USAGE_STAGING;
            desc.CPUAccessFlags=D3D11_CPU_ACCESS_READ.0 as u32;
            self.staging=Some(((width,height),make_texture(&self.device,&desc)?));
        }
        let (_,staging)=self.staging.as_ref().unwrap();
        let left=x+self.client.left as u32;
        let top=y+self.client.top as u32;
        let region=D3D11_BOX{left,top,front:0,right:left+width,bottom:top+height,back:1};
        let mut data=D3D11_MAPPED_SUBRESOURCE::default();
        let row_len=width as usize*4;
        bgra.resize(row_len*height as usize,0);
        unsafe{
            self.context.CopySubresourceRegion(staging,0,0,0,0,source,0,Some(&region));
            self.context.Map(staging,0,D3D11_MAP_READ,0,Some(&mut data)).map_err(|e|e.to_string())?;
            if data.pData.is_null() || (data.RowPitch as usize)<row_len{
                self.context.Unmap(staging,0);
                return Err("Windows 返回了无效的投屏纹理".into());
            }
            for (row,target) in bgra.chunks_exact_mut(row_len).enumerate(){
                let source=slice::from_raw_parts(data.pData.cast::<u8>().add(row*data.RowPitch as usize),row_len);
                target.copy_from_slice(source);
            }
            self.context.Unmap(staging,0);
        }
        Ok(true)
    }
}

impl Drop for WindowCapture {
    fn drop(&mut self){let _=self.session.Close();let _=self.pool.Close();}
}

fn make_texture(device:&ID3D11Device,desc:&D3D11_TEXTURE2D_DESC)->Result<ID3D11Texture2D,String>{
    let mut texture=None;
    unsafe{device.CreateTexture2D(desc,None,Some(&mut texture))}.map_err(|e|e.to_string())?;
    texture.ok_or_else(||"无法创建投屏纹理".into())
}

pub(crate) fn window_png(raw_hwnd:usize)->Result<Vec<u8>,String>{
    thread::spawn(move||{
        unsafe{SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);}
        capture_png(raw_hwnd)
    }).join().map_err(|_|"无法完成窗口定格".to_string())?
}

fn capture_png(hwnd:usize)->Result<Vec<u8>,String>{
    let mut capture=WindowCapture::open(hwnd)?;
    let deadline=Instant::now()+Duration::from_millis(1100);
    while !capture.poll()?{
        if Instant::now()>=deadline{return Err("投屏窗口没有返回可定格的画面".into());}
        thread::sleep(Duration::from_millis(12));
    }
    let (width,height)=capture.dimensions();
    let mut bgra=Vec::new();
    if !capture.read((0,0,width,height),&mut bgra)?{return Err("没有可定格的画面".into());}
    drop(capture);
    let mut rgb=Vec::with_capacity(width as usize*height as usize*3);
    for pixel in bgra.chunks_exact(4){rgb.extend_from_slice(&[pixel[2],pixel[1],pixel[0]]);}
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().and_then(|mut writer| writer.write_image_data(&rgb)).map_err(|e| e.to_string())?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::{core::w, Win32::Graphics::{Dwm::DwmFlush, Gdi::{FillRect, GetDC,
        GetStockObject, ReleaseDC, UpdateWindow, HBRUSH, WHITE_BRUSH}},
        Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE, WINDOW_STYLE}};

    #[test]
    #[ignore = "requires an interactive DWM desktop"]
    fn one_frame_is_cropped_to_the_synthetic_mirror_client() {
        struct TestWindow(HWND);
        impl Drop for TestWindow {
            fn drop(&mut self) { let _ = unsafe { DestroyWindow(self.0) }; }
        }
        let hwnd = unsafe { CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, w!("STATIC"),
            w!("MirroLeap snapshot test"), WS_POPUP | WS_VISIBLE | WINDOW_STYLE(6),
            120, 120, 180, 120, None, None, None, None) }.unwrap();
        let _window = TestWindow(hwnd);
        unsafe {
            let _ = UpdateWindow(hwnd);
            let dc = GetDC(Some(hwnd));
            let mut client = RECT::default();
            GetClientRect(hwnd, &mut client).unwrap();
            assert_ne!(FillRect(dc, &client, HBRUSH(GetStockObject(WHITE_BRUSH).0)), 0);
            ReleaseDC(Some(hwnd), dc);
        }
        let _ = unsafe { DwmFlush() };
        let bytes = window_png(hwnd.0 as usize).unwrap();
        let mut image = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let mut pixels = vec![0; image.output_buffer_size()];
        let frame = image.next_frame(&mut pixels).unwrap();
        assert!(frame.width > 0 && frame.height > 0);
        assert_eq!(frame.width * 2, frame.height * 3);
        let center = ((frame.height / 2 * frame.width) + frame.width / 2) as usize * 3;
        assert!(pixels[center..center + 3].iter().all(|color| *color > 220),
            "the test window should be white, center RGB is {:?}", &pixels[center..center + 3]);
        let mut capture=WindowCapture::open(hwnd.0 as usize).unwrap();
        let deadline=Instant::now()+Duration::from_secs(1);
        while !capture.poll().unwrap(){
            assert!(Instant::now()<deadline,"live capture did not produce a frame");
            thread::sleep(Duration::from_millis(12));
        }
        let mut crop=Vec::new();
        for x in [12,32,52]{
            assert!(capture.read((x,10,40,40),&mut crop).unwrap());
            assert_eq!(crop.len(),40*40*4);
            assert!(crop[..3].iter().all(|color|*color>220));
        }
        drop(capture);
    }
}
