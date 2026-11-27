//! Bluetooth HID controls for an explicitly selected iPhone or iPad.

use crate::ios_input::{self, Event, Position, State};
use crate::ios_diagnostics::{self, Channel};
use std::io::Write;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::core::{GUID, HSTRING};
use windows::Devices::Bluetooth::{
    BluetoothAdapter, BluetoothError, BluetoothLEDevice, GenericAttributeProfile::*,
};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Devices::Radios::RadioState;
use windows::Foundation::TypedEventHandler;
use windows::Security::Cryptography::CryptographicBuffer;

// Keyboard, absolute positioning, relative mouse buttons/drag/wheel, and consumer keys.
// Reports are sent by GATT characteristic, without an in-band report ID.
const ABSOLUTE_POINTER_ID: u8 = 2;
const RELATIVE_MOUSE_ID: u8 = 3;
const REPORT_MAP: &[u8] = &[
    0x05,
    0x01,
    0x09,
    0x06,
    0xA1,
    0x01,
    0x85,
    1,
    0x05,
    0x07,
    0x19,
    0xE0,
    0x29,
    0xE7,
    0x15,
    0,
    0x25,
    1,
    0x75,
    1,
    0x95,
    8,
    0x81,
    2,
    0x75,
    8,
    0x95,
    1,
    0x81,
    1,
    0x75,
    8,
    0x95,
    6,
    0x15,
    0,
    0x25,
    0x65,
    0x05,
    0x07,
    0x19,
    0,
    0x29,
    0x65,
    0x81,
    0,
    0xC0,
    0x05,
    0x01,
    0x09,
    0x02,
    0xA1,
    0x01,
    0x85,
    ABSOLUTE_POINTER_ID,
    0x09,
    0x01,
    0xA1,
    0x00,
    0x05,
    0x09,
    0x19,
    1,
    0x29,
    3,
    0x15,
    0,
    0x25,
    1,
    0x75,
    1,
    0x95,
    3,
    0x81,
    2,
    0x75,
    5,
    0x95,
    1,
    0x81,
    3,
    0x05,
    0x01,
    0x09,
    0x30,
    0x09,
    0x31,
    0x15,
    0,
    0x26,
    0xFF,
    0x7F,
    0x75,
    16,
    0x95,
    2,
    0x81,
    2,
    0xC0,
    0xC0,
    0x05,
    0x01,
    0x09,
    0x02,
    0xA1,
    0x01,
    0x85,
    RELATIVE_MOUSE_ID,
    0x09,
    0x01,
    0xA1,
    0x00,
    0x05,
    0x09,
    0x19,
    1,
    0x29,
    3,
    0x15,
    0,
    0x25,
    1,
    0x75,
    1,
    0x95,
    3,
    0x81,
    2,
    0x75,
    5,
    0x95,
    1,
    0x81,
    3,
    0x05,
    0x01,
    0x09,
    0x30,
    0x09,
    0x31,
    0x09,
    0x38,
    0x15,
    0x81,
    0x25,
    0x7F,
    0x75,
    8,
    0x95,
    3,
    0x81,
    6,
    // Consumer AC Pan, horizontal scrolling.
    0x05,
    0x0C,
    0x0A,
    0x38,
    0x02,
    0x95,
    1,
    0x81,
    6,
    0xC0,
    0xC0,
    0x05,
    0x0C,
    0x09,
    0x01,
    0xA1,
    0x01,
    0x85,
    4,
    0x15,
    0,
    0x26,
    0xFF,
    3,
    0x75,
    16,
    0x95,
    1,
    0x0A,
    0x9D,
    2,
    0x81,
    2,
    0xC0,
    0x05,
    0x0C,
    0x09,
    0x01,
    0xA1,
    0x01,
    0x85,
    5,
    0x15,
    0,
    0x25,
    1,
    0x75,
    1,
    0x95,
    2,
    0x0A,
    0x24,
    2,
    0x09,
    0x40,
    0x81,
    2,
    0x75,
    1,
    0x95,
    14,
    0x81,
    3,
    0xC0,
];
const GLOBE: [u8; 2] = 0x029Du16.to_le_bytes();
const COMMAND_LEFT_BRACKET: [u8; 8] = [0x08, 0, 0x2F, 0, 0, 0, 0, 0];
const MENU: [u8; 2] = [2, 0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Home,
    Back,
    Recents,
    Lock,
}

impl Action {
    pub(crate) fn from_toolbar(action: &str) -> Option<Self> {
        Some(match action {
            "quick_home" => Self::Home,
            "quick_back" => Self::Back,
            "quick_recents" => Self::Recents,
            "quick_power" => Self::Lock,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owner {
    usb: bool,
    generation: u32,
}

static GENERATIONS: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
static NEXT_TOKEN: AtomicU32 = AtomicU32::new(1);
static SESSION: tokio::sync::Mutex<Option<Session>> = tokio::sync::Mutex::const_new(None);

impl Owner {
    pub(crate) fn new(usb: bool) -> Self {
        Self {
            usb,
            generation: GENERATIONS[usize::from(usb)].load(Ordering::Acquire),
        }
    }

    fn current(self) -> bool {
        self == Self::new(self.usb)
    }

    fn report_control_status(self, status: &'static str) {
        if self.current() {
            ios_diagnostics::observe(Channel::Bluetooth, status, Some(if self.usb { "usb" } else { "wifi" }));
        }
    }
}

pub(crate) fn close(usb: bool) {
    let generation = GENERATIONS[usize::from(usb)].fetch_add(1, Ordering::AcqRel);
    let owner = Owner { usb, generation };
    tauri::async_runtime::spawn(async move {
        let mut slot = SESSION.lock().await;
        if slot.as_ref().is_some_and(|session| session.owner == owner) {
            let _ = stop_session(&mut slot).await;
        }
    });
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    phase: &'static str,
    device_name: Option<String>,
    token: Option<u32>,
    pointer_ready: bool,
}

impl Status {
    fn new(phase: &'static str) -> Self {
        Self {
            phase,
            device_name: None,
            token: None,
            pointer_ready: false,
        }
    }
}

struct Target {
    id: HSTRING,
    name: String,
    token: u32,
}

struct Session {
    owner: Owner,
    hid: HidService,
    candidate: Option<Target>,
    bound: Option<Target>,
    input: State,
}

fn uuid(id: u16) -> GUID {
    GUID::from_u128((u128::from(id) << 96) | 0x0000_0000_0000_1000_8000_00805f9b34fb)
}

fn win_error(error: windows::core::Error) -> String {
    format!("Windows 蓝牙操作失败（{}）", error.code())
}

async fn characteristic(
    service: &GattLocalService,
    id: u16,
    properties: GattCharacteristicProperties,
    value: &[u8],
) -> Result<GattLocalCharacteristic, String> {
    let params = GattLocalCharacteristicParameters::new().map_err(win_error)?;
    params
        .SetCharacteristicProperties(properties)
        .map_err(win_error)?;
    params
        .SetReadProtectionLevel(GattProtectionLevel::EncryptionRequired)
        .map_err(win_error)?;
    params
        .SetWriteProtectionLevel(GattProtectionLevel::EncryptionRequired)
        .map_err(win_error)?;
    params
        .SetStaticValue(&CryptographicBuffer::CreateFromByteArray(value).map_err(win_error)?)
        .map_err(win_error)?;
    let result = service
        .CreateCharacteristicAsync(uuid(id), &params)
        .map_err(win_error)?
        .await
        .map_err(win_error)?;
    let error = result.Error().map_err(win_error)?;
    if error != BluetoothError::Success {
        return Err(format!("蓝牙 HID 特征 {id:04X} 创建失败（{}）", error.0));
    }
    result.Characteristic().map_err(win_error)
}

async fn report(
    service: &GattLocalService,
    id: u8,
    initial: &[u8],
) -> Result<GattLocalCharacteristic, String> {
    let properties = GattCharacteristicProperties::Read | GattCharacteristicProperties::Notify;
    let report = characteristic(service, 0x2a4d, properties, initial).await?;
    let params = GattLocalDescriptorParameters::new().map_err(win_error)?;
    params
        .SetReadProtectionLevel(GattProtectionLevel::EncryptionRequired)
        .map_err(win_error)?;
    params
        .SetStaticValue(&CryptographicBuffer::CreateFromByteArray(&[id, 1]).map_err(win_error)?)
        .map_err(win_error)?;
    let result = report
        .CreateDescriptorAsync(uuid(0x2908), &params)
        .map_err(win_error)?
        .await
        .map_err(win_error)?;
    if result.Error().map_err(win_error)? != BluetoothError::Success {
        return Err("创建蓝牙 HID 输入报告描述失败".into());
    }
    Ok(report)
}

struct HidService {
    provider: GattServiceProvider,
    advertising_event: i64,
    keyboard: GattLocalCharacteristic,
    absolute_pointer: GattLocalCharacteristic,
    relative_mouse: GattLocalCharacteristic,
    _control_point: GattLocalCharacteristic,
    globe: GattLocalCharacteristic,
    navigation: GattLocalCharacteristic,
}

impl Drop for HidService {
    fn drop(&mut self) {
        if self.is_advertising().unwrap_or(true) {
            let _ = self.provider.StopAdvertising();
        }
        let _ = self
            .provider
            .RemoveAdvertisementStatusChanged(self.advertising_event);
    }
}

impl HidService {
    async fn stop(&self) -> Result<(), String> {
        self.provider.StopAdvertising().map_err(win_error)?;
        for _ in 0..12 {
            if !self.is_advertising()? {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // Some drivers keep the cached advertisement status after StopAdvertising.
        // The caller is disposing this provider and all its local reports.
        eprintln!("[ios bluetooth] releasing provider without a confirmed advertising stop");
        Ok(())
    }

    fn is_advertising(&self) -> Result<bool, String> {
        use GattServiceProviderAdvertisementStatus as Advertising;
        Ok(matches!(
            self.provider.AdvertisementStatus().map_err(win_error)?,
            Advertising::Started | Advertising::StartedWithoutAllAdvertisementData
        ))
    }

    async fn start() -> Result<Self, String> {
        use GattCharacteristicProperties as Properties;
        let adapter = BluetoothAdapter::GetDefaultAsync()
            .map_err(win_error)?
            .await
            .map_err(|_| "未找到可用的 Windows 蓝牙适配器")?;
        if !adapter.IsPeripheralRoleSupported().map_err(win_error)? {
            return Err("此电脑的蓝牙适配器不支持 BLE 外设模式".into());
        }
        let radio = adapter
            .GetRadioAsync()
            .map_err(win_error)?
            .await
            .map_err(win_error)?;
        if radio.State().map_err(win_error)? != RadioState::On {
            return Err("请先打开 Windows 蓝牙，再点击连接".into());
        }
        let result = GattServiceProvider::CreateAsync(uuid(0x1812))
            .map_err(win_error)?
            .await
            .map_err(win_error)?;
        let error = result.Error().map_err(win_error)?;
        if error != BluetoothError::Success {
            return Err(format!(
                "无法创建蓝牙 HID 服务（{}）。请检查蓝牙状态及其他反控程序",
                error.0
            ));
        }
        let provider = result.ServiceProvider().map_err(win_error)?;
        let service = provider.Service().map_err(win_error)?;
        characteristic(&service, 0x2a4a, Properties::Read, &[0x11, 1, 0, 2]).await?;
        characteristic(&service, 0x2a4b, Properties::Read, REPORT_MAP).await?;
        let control_point =
            characteristic(&service, 0x2a4c, Properties::WriteWithoutResponse, &[0]).await?;
        control_point
            .WriteRequested(&TypedEventHandler::<_, GattWriteRequestedEventArgs>::new(
                |_, args| {
                    let args = args.ok()?.clone();
                    let deferral = args.GetDeferral()?;
                    tauri::async_runtime::spawn(async move {
                        if let Ok(operation) = args.GetRequestAsync() {
                            if let Ok(request) = operation.await {
                                if request.Option().ok() == Some(GattWriteOption::WriteWithResponse)
                                {
                                    let _ = request.Respond();
                                }
                            }
                        }
                        let _ = deferral.Complete();
                    });
                    Ok(())
                },
            ))
            .map_err(win_error)?;
        let keyboard = report(&service, 1, &[0; 8]).await?;
        let absolute_pointer = report(&service, ABSOLUTE_POINTER_ID, &[0; 5]).await?;
        let relative_mouse = report(&service, RELATIVE_MOUSE_ID, &[0; 5]).await?;
        let globe = report(&service, 4, &[0; 2]).await?;
        let navigation = report(&service, 5, &[0; 2]).await?;
        let params = GattServiceProviderAdvertisingParameters::new().map_err(win_error)?;
        params.SetIsConnectable(true).map_err(win_error)?;
        params.SetIsDiscoverable(true).map_err(win_error)?;
        let last_error = Arc::new(AtomicI32::new(0));
        let event = {
            let errors = last_error.clone();
            let handler = TypedEventHandler::<
                _,
                GattServiceProviderAdvertisementStatusChangedEventArgs,
            >::new(move |_, args| {
                if let Ok(error) = args.ok().and_then(|args| args.Error()) {
                    errors.store(error.0, Ordering::Relaxed);
                }
                Ok(())
            });
            provider
                .AdvertisementStatusChanged(&handler)
                .map_err(win_error)?
        };
        let hid = Self {
            provider,
            advertising_event: event,
            keyboard,
            absolute_pointer,
            relative_mouse,
            _control_point: control_point,
            globe,
            navigation,
        };
        let result = async {
            hid.provider
                .StartAdvertisingWithParameters(&params)
                .map_err(win_error)?;
            for _ in 0..40 {
                if hid.is_advertising()? {
                    return Ok(());
                }
                // Some Windows Bluetooth drivers report Aborted briefly before Started.
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(format!(
                "Windows 蓝牙 HID 广播未启动（{}）",
                last_error.load(Ordering::Relaxed)
            ))
        }
        .await;
        result.map(|()| hid)
    }

    fn ready_ids(&self) -> Result<Vec<HSTRING>, String> {
        let keyboard = clients(&self.keyboard)?;
        let globe = clients(&self.globe)?;
        let navigation = clients(&self.navigation)?;
        let mut ids = Vec::new();
        for (id, _) in keyboard {
            if globe.iter().any(|(other, _)| *other == id)
                && navigation.iter().any(|(other, _)| *other == id)
                && !ids.contains(&id)
            {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    fn pointer_ready(&self, id: &HSTRING) -> bool {
        subscribed_client(&self.absolute_pointer, id).is_some()
            && subscribed_client(&self.relative_mouse, id).is_some()
    }
}

fn clients(
    report: &GattLocalCharacteristic,
) -> Result<Vec<(HSTRING, GattSubscribedClient)>, String> {
    let clients = report.SubscribedClients().map_err(win_error)?;
    let mut active = Vec::new();
    for i in 0..clients.Size().map_err(win_error)? {
        let client = clients.GetAt(i).map_err(win_error)?;
        let session = client.Session().map_err(win_error)?;
        if session.SessionStatus().map_err(win_error)? == GattSessionStatus::Active {
            let id = session
                .DeviceId()
                .and_then(|device| device.Id())
                .map_err(win_error)?;
            active.push((id, client));
        }
    }
    Ok(active)
}

async fn paired_name(id: &HSTRING) -> Result<String, String> {
    let lookup = async {
        let peripheral = BluetoothLEDevice::FromIdAsync(id)?.await?;
        peripheral.DeviceInformation()
    };
    let info = match tokio::time::timeout(Duration::from_secs(1), lookup).await {
        Ok(Ok(info)) => info,
        _ => {
            let lookup = DeviceInformation::CreateFromIdAsync(id).map_err(win_error)?;
            tokio::time::timeout(Duration::from_secs(1), lookup)
                .await
                .map_err(|_| "读取蓝牙设备名称超时")?
                .map_err(win_error)?
        }
    };
    if !info
        .Pairing()
        .and_then(|pairing| pairing.IsPaired())
        .map_err(win_error)?
    {
        return Err("请先在 Windows 和 iPhone 或 iPad 上确认蓝牙配对".into());
    }
    let name = info.Name().map_err(win_error)?.to_string_lossy();
    if name.trim().is_empty() {
        return Err("请在蓝牙设置中确认设备名称后重试".into());
    }
    Ok(name)
}

impl Session {
    async fn status(&mut self) -> Result<Status, String> {
        let ids = self.hid.ready_ids()?;
        if let Some(target) = &self.bound {
            let connected = ids.contains(&target.id);
            return Ok(Status {
                phase: if connected {
                    "connected"
                } else {
                    "reconnecting"
                },
                device_name: Some(target.name.clone()),
                token: None,
                pointer_ready: connected && self.hid.pointer_ready(&target.id),
            });
        }
        if ids.len() != 1 {
            self.candidate = None;
            return Ok(Status::new(if ids.is_empty() {
                "waiting"
            } else {
                "multiple"
            }));
        }
        let id = &ids[0];
        if self
            .candidate
            .as_ref()
            .is_none_or(|candidate| candidate.id != *id)
        {
            self.candidate = Some(Target {
                id: id.clone(),
                name: paired_name(id).await?,
                token: NEXT_TOKEN.fetch_add(1, Ordering::Relaxed),
            });
        }
        let target = self.candidate.as_ref().ok_or("请等待蓝牙设备出现")?;
        Ok(Status {
            phase: "confirm",
            device_name: Some(target.name.clone()),
            token: Some(target.token),
            pointer_ready: false,
        })
    }
}

async fn stop_session(slot: &mut Option<Session>) -> Result<(), String> {
    match slot.take() {
        Some(mut session) => {
            ios_diagnostics::observe(Channel::Bluetooth, "off", None);
            if let Some(target) = &session.bound {
                release_input(&session.hid, &target.id, &mut session.input).await;
            }
            session.hid.stop().await
        }
        None => Ok(()),
    }
}

pub(crate) async fn setup(
    owner: Owner,
    action: &str,
    token: Option<u32>,
) -> Result<Status, String> {
    if !matches!(action, "status" | "start" | "confirm" | "stop") {
        return Err("不支持的蓝牙控制操作".into());
    }
    let mut slot = SESSION.lock().await;
    if slot
        .as_ref()
        .is_some_and(|session| !session.owner.current())
    {
        stop_session(&mut slot).await?;
    }
    if !owner.current() {
        return Err("iOS 投屏会话已结束".into());
    }
    if slot.as_ref().is_some_and(|session| session.owner != owner) {
        return if action == "status" {
            Ok(Status::new("in_use"))
        } else {
            Err("另一个 iOS 投屏正在使用蓝牙控制".into())
        };
    }
    match action {
        "stop" => {
            stop_session(&mut slot).await?;
        }
        "start" if slot.is_none() => {
            let hid = tokio::time::timeout(Duration::from_secs(10), HidService::start())
                .await
                .map_err(|_| "蓝牙控制启动超时")??;
            if !owner.current() {
                return Err("iOS 投屏会话已结束".into());
            }
            *slot = Some(Session {
                owner,
                hid,
                candidate: None,
                bound: None,
                input: State::default(),
            });
        }
        "confirm" => {
            let session = slot.as_mut().ok_or("请先开启蓝牙控制")?;
            let candidate = session.candidate.as_ref().ok_or("请等待蓝牙设备出现")?;
            let ids = session.hid.ready_ids()?;
            if token != Some(candidate.token)
                || ids.as_slice() != std::slice::from_ref(&candidate.id)
            {
                return Err("蓝牙设备已变化，请重新确认".into());
            }
            session.bound = session.candidate.take();
            if let Some(target) = &session.bound {
                if session.hid.pointer_ready(&target.id) {
                    trace_mouse("pointer-reports-subscribed");
                    release_mouse(&session.hid, &target.id, &mut session.input, true).await;
                } else {
                    trace_mouse("waiting-for-pointer-reports");
                }
            }
        }
        _ => {}
    }
    match slot.as_mut() {
        Some(session) => {
            let status = session.status().await?;
            owner.report_control_status(match status.phase {
                "connected" if status.pointer_ready => "ready",
                "connected" => "keyboard_only",
                phase => phase,
            });
            Ok(status)
        }
        None => Ok(Status::new("off")),
    }
}

async fn notify(
    report: &GattLocalCharacteristic,
    id: &HSTRING,
    bytes: &[u8],
) -> Result<(), String> {
    let client = subscribed_client(report, id).ok_or("已确认的 iOS 蓝牙设备当前不可用")?;
    notify_client(report, &client, bytes).await
}

fn subscribed_client(
    report: &GattLocalCharacteristic,
    id: &HSTRING,
) -> Option<GattSubscribedClient> {
    clients(report)
        .ok()?
        .into_iter()
        .find(|(client, _)| client == id)
        .map(|(_, client)| client)
}

async fn notify_client(
    report: &GattLocalCharacteristic,
    client: &GattSubscribedClient,
    bytes: &[u8],
) -> Result<(), String> {
    let pending = {
        let buffer = CryptographicBuffer::CreateFromByteArray(bytes).map_err(win_error)?;
        report
            .NotifyValueForSubscribedClientAsync(&buffer, client)
            .map_err(win_error)?
    };
    let result = tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .map_err(|_| "发送蓝牙按键超时")?
        .map_err(win_error)?;
    if result.Status().map_err(win_error)? != GattCommunicationStatus::Success {
        return Err("iOS 蓝牙设备未收到按键，请检查连接".into());
    }
    Ok(())
}

fn trace_mouse(operation: &str) {
    static TRACE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    if let Some(mut output) = TRACE
        .get_or_init(|| {
            let path = std::env::var_os("MIRROLEAP_IOS_INPUT_TRACE")
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    (option_env!("MIRROLEAP_IOS_HID_DIAGNOSTICS") == Some("1")).then(|| {
                        std::env::temp_dir()
                            .join(format!("MirroLeap-iOS-HID-{}.log", std::process::id()))
                    })
                })?;
            std::fs::File::create(path).ok().map(Mutex::new)
        })
        .as_ref()
        .and_then(|file| file.lock().ok())
    {
        if !output
            .metadata()
            .is_ok_and(|metadata| metadata.len() < 128 * 1024)
        {
            return;
        }
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let _ = writeln!(output, "{} {operation}", time.as_millis());
    }
}

async fn send_absolute(
    hid: &HidService,
    id: &HSTRING,
    state: &mut State,
    point: Position,
) -> Result<(), String> {
    let result = notify(
        &hid.absolute_pointer,
        id,
        &ios_input::absolute_report(point),
    )
    .await;
    if result.is_ok() {
        state.pointer = Some(point);
    } else {
        trace_mouse("absolute-report-error");
    }
    result
}

async fn send_relative(
    hid: &HidService,
    id: &HSTRING,
    state: &mut State,
    report: [u8; 5],
) -> Result<(), String> {
    let previous = state.buttons();
    let result = notify(&hid.relative_mouse, id, &report).await;
    state.record_buttons(report[0], result.is_ok());
    if result.is_err() {
        trace_mouse("relative-report-error");
    } else if report[0] != previous {
        trace_mouse(if report[0] & !previous != 0 {
            "relative-button-down"
        } else {
            "relative-button-up"
        });
    } else if report[3] != 0 || report[4] != 0 {
        trace_mouse(if report[4] != 0 {
            "horizontal-scroll"
        } else {
            "vertical-scroll"
        });
    }
    result
}

async fn send_pointer(
    hid: &HidService,
    id: &HSTRING,
    state: &mut State,
    point: Position,
    buttons: u8,
    scroll: (i8, i8),
    viewport: Position,
) -> Result<(), String> {
    let previous = state.buttons();
    if previous == 0 {
        if state.pointer != Some(point) {
            send_absolute(hid, id, state, point).await?;
            if buttons != 0 || scroll != (0, 0) {
                // Let iOS apply the new position before the other report arrives.
                tokio::time::sleep(Duration::from_millis(16)).await;
            }
        }
    } else if let Some(from) = state.pointer {
        // Absolute movement can interrupt the drag owned by the relative mouse.
        for (dx, dy) in ios_input::drag_steps(from, point, viewport) {
            send_relative(
                hid,
                id,
                state,
                ios_input::relative_report(previous, dx, dy, (0, 0)),
            )
            .await?;
        }
        state.pointer = Some(point);
    }
    if previous != buttons || scroll != (0, 0) {
        send_relative(
            hid,
            id,
            state,
            ios_input::relative_report(buttons, 0, 0, scroll),
        )
        .await?;
    }
    if previous != 0 && buttons == 0 {
        // Restore exact hover coordinates after a relative drag ends.
        tokio::time::sleep(Duration::from_millis(16)).await;
        send_absolute(hid, id, state, point).await?;
    }
    Ok(())
}

async fn send_swipe_scroll(
    hid: &HidService,
    id: &HSTRING,
    state: &mut State,
    point: Position,
    scroll: (i8, i8),
    start: bool,
) -> Result<(), String> {
    let spacing = Duration::from_millis(12);
    if state.buttons() != 0 {
        send_relative(hid, id, state, [0; 5]).await?;
    }
    if state.pointer != Some(point) {
        send_absolute(hid, id, state, point).await?;
        tokio::time::sleep(Duration::from_millis(16)).await;
    }
    if start {
        // iOS can absorb the first few wheel events after an idle interval.
        // Prime the axis with small deltas so a quick swipe still scrolls.
        let lead = (scroll.0.signum(), scroll.1.signum());
        for _ in 0..3 {
            send_relative(hid, id, state, ios_input::relative_report(0, 0, 0, lead)).await?;
            tokio::time::sleep(spacing).await;
        }
    }
    let mut steps = ios_input::swipe_steps(scroll).peekable();
    while let Some(step) = steps.next() {
        send_relative(hid, id, state, ios_input::relative_report(0, 0, 0, step)).await?;
        if steps.peek().is_some() {
            tokio::time::sleep(spacing).await;
        }
    }
    Ok(())
}

async fn release_mouse(hid: &HidService, id: &HSTRING, state: &mut State, force: bool) {
    if !force && state.buttons() == 0 {
        return;
    }
    let mut result = send_relative(hid, id, state, [0; 5]).await;
    if result.is_err() {
        tokio::time::sleep(Duration::from_millis(25)).await;
        result = send_relative(hid, id, state, [0; 5]).await;
    }
    if result.is_ok() {
        trace_mouse("relative-buttons-cleared");
    }
}

async fn release_input(hid: &HidService, id: &HSTRING, state: &mut State) {
    let release_keys = state.release_keys();
    let (keyboard_result, _) = tokio::join!(
        async {
            if release_keys {
                notify(&hid.keyboard, id, &[0; 8]).await
            } else {
                Ok(())
            }
        },
        release_mouse(hid, id, state, false)
    );
    if release_keys {
        state.record_keyboard_delivery(keyboard_result.is_ok(), Instant::now());
    }
    state.pointer = None;
}

fn physical_mouse_held() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
    };
    [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
        .into_iter()
        .any(|key| (unsafe { GetAsyncKeyState(key.0 as i32) }) < 0)
}

pub(crate) fn stale_mouse_press(owner: Owner) -> bool {
    !physical_mouse_held()
        && SESSION.try_lock().is_ok_and(|slot| {
            slot.as_ref()
                .is_some_and(|session| session.owner == owner && session.input.buttons() != 0)
        })
}

pub(crate) async fn recover_mouse_release(owner: Owner) {
    let Ok(mut slot) = SESSION.try_lock() else {
        return;
    };
    if physical_mouse_held() || !owner.current() {
        return;
    }
    if let Some(session) = slot.as_mut().filter(|session| session.owner == owner) {
        if let Some(target) = &session.bound {
            trace_mouse("recovering-missing-button-up");
            release_mouse(&session.hid, &target.id, &mut session.input, false).await;
            session.input.pointer = None;
        }
    }
}

pub(crate) fn input_ready(owner: Owner, previous: bool) -> bool {
    if !owner.current() {
        return false;
    }
    let Ok(slot) = SESSION.try_lock() else {
        return previous;
    };
    slot.as_ref()
        .filter(|session| session.owner == owner)
        .and_then(|session| {
            session
                .bound
                .as_ref()
                .map(|target| (&session.hid, &target.id))
        })
        .is_some_and(|(hid, id)| {
            let keyboard_ready = subscribed_client(&hid.keyboard, id).is_some();
            let ready = keyboard_ready && hid.pointer_ready(id);
            owner.report_control_status(if ready { "ready" } else if keyboard_ready { "keyboard_only" } else { "reconnecting" });
            ready
        })
}

pub(crate) async fn send_input(
    owner: Owner,
    events: Vec<Event>,
    pointer_allowed: bool,
    keyboard_allowed: bool,
    viewport: Position,
) -> Result<(), String> {
    if events.is_empty()
        || events.len() > crate::ios_input::BATCH_LIMIT
        || !events.iter().all(Event::valid)
    {
        return Err("无效的投屏输入".into());
    }
    let mut slot = SESSION.lock().await;
    let Some(session) = slot
        .as_mut()
        .filter(|session| session.owner == owner && owner.current())
    else {
        return Ok(());
    };
    let Some(target) = &session.bound else {
        return Ok(());
    };
    let hid = &session.hid;
    let id = &target.id;
    let state = &mut session.input;
    if !pointer_allowed {
        release_input(hid, id, state).await;
        return Ok(());
    }
    let result = async {
        for event in events {
            if !owner.current() {
                break;
            }
            match event {
                Event::Release => release_input(hid, id, state).await,
                Event::Key { usage, down } if keyboard_allowed => {
                    if usage == 0x29 && down {
                        release_mouse(hid, id, state, true).await;
                        state.pointer = None;
                    }
                    if let Some(report) = state.keyboard(usage, down) {
                        if let Some(deadline) = state.keyboard_ready_at() {
                            tokio::time::sleep_until(deadline.into()).await;
                        }
                        let delivery = notify(&hid.keyboard, id, &report).await;
                        state.record_keyboard_delivery(delivery.is_ok(), Instant::now());
                        delivery?;
                    }
                }
                Event::Pointer {
                    x,
                    y,
                    buttons,
                    wheel,
                } => {
                    send_pointer(hid, id, state, (x, y), buttons, (wheel, 0), viewport).await?;
                }
                Event::Scroll {
                    x,
                    y,
                    wheel,
                    pan,
                    start,
                } => {
                    send_swipe_scroll(hid, id, state, (x, y), (wheel, pan), start).await?;
                }
                Event::Click { x, y, button } => {
                    let previous = state.buttons() & !button;
                    if previous != state.buttons() {
                        send_pointer(hid, id, state, (x, y), previous, (0, 0), viewport).await?;
                    }
                    send_pointer(hid, id, state, (x, y), previous | button, (0, 0), viewport)
                        .await?;
                    tokio::time::sleep(Duration::from_millis(34)).await;
                    send_relative(
                        hid,
                        id,
                        state,
                        ios_input::relative_report(previous, 0, 0, (0, 0)),
                    )
                    .await?;
                    if button == 1 {
                        state.note_primary_click(Instant::now());
                    }
                }
                Event::Key { .. } => {
                    if state.release_keys() {
                        let delivery = notify(&hid.keyboard, id, &[0; 8]).await;
                        state.record_keyboard_delivery(delivery.is_ok(), Instant::now());
                        delivery?;
                    }
                }
            }
        }
        Ok(())
    }
    .await;
    if result.is_err() {
        release_input(hid, id, state).await;
    }
    result
}

async fn pulse<const N: usize>(
    report: &GattLocalCharacteristic,
    id: &HSTRING,
    bytes: &[u8; N],
) -> Result<(), String> {
    let pressed = notify(report, id, bytes).await;
    if pressed.is_ok() {
        tokio::time::sleep(Duration::from_millis(35)).await;
    }
    let released = notify(report, id, &[0; N]).await;
    pressed.and(released)
}

pub(crate) async fn send(owner: Owner, action: Action) -> Result<(), String> {
    let mut slot = SESSION.lock().await;
    let session = slot
        .as_mut()
        .filter(|session| session.owner == owner && owner.current())
        .ok_or("请先连接蓝牙控制")?;
    let id = &session
        .bound
        .as_ref()
        .ok_or("请先确认要控制的 iPhone 或 iPad")?
        .id;
    let hid = &session.hid;
    release_input(hid, id, &mut session.input).await;
    match action {
        Action::Back => pulse(&hid.keyboard, id, &COMMAND_LEFT_BRACKET).await,
        Action::Recents => {
            pulse(&hid.navigation, id, &MENU).await?;
            tokio::time::sleep(Duration::from_millis(100)).await;
            if !owner.current() {
                return Err("iOS 投屏会话已结束".into());
            }
            pulse(&hid.navigation, id, &MENU).await
        }
        Action::Home | Action::Lock => {
            let globe = notify(&hid.globe, id, &GLOBE).await;
            if globe.is_err() || !owner.current() {
                let _ = notify(&hid.globe, id, &[0; 2]).await;
                return globe.and(Err("iOS 投屏会话已结束".into()));
            }
            let usage = if action == Action::Home { 0x0B } else { 0x11 };
            let pressed = notify(&hid.keyboard, id, &[0, 0, usage, 0, 0, 0, 0, 0]).await;
            if pressed.is_ok() {
                tokio::time::sleep(Duration::from_millis(35)).await;
            }
            let key_up = notify(&hid.keyboard, id, &[0; 8]).await;
            let globe_up = notify(&hid.globe, id, &[0; 2]).await;
            pressed.and(key_up).and(globe_up)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_report_map_matches_gatt_payloads() {
        let mut inputs = Vec::new();
        let (mut id, mut size, mut count) = (0, 0, 0);
        let mut items = REPORT_MAP;
        while let [prefix, rest @ ..] = items {
            let length = match prefix & 3 {
                3 => 4,
                length => usize::from(length),
            };
            let value = rest.first().copied().unwrap_or(0);
            match prefix & 0xfc {
                0x84 => id = value,
                0x74 => size = value,
                0x94 => count = value,
                0x80 if matches!(id, ABSOLUTE_POINTER_ID | RELATIVE_MOUSE_ID) => {
                    inputs.push((id, size * count, value));
                }
                _ => {}
            }
            items = &rest[length..];
        }
        assert_eq!(
            inputs,
            [
                (ABSOLUTE_POINTER_ID, 3, 2),
                (ABSOLUTE_POINTER_ID, 5, 3),
                (ABSOLUTE_POINTER_ID, 32, 2),
                (RELATIVE_MOUSE_ID, 3, 2),
                (RELATIVE_MOUSE_ID, 5, 3),
                (RELATIVE_MOUSE_ID, 24, 6),
                (RELATIVE_MOUSE_ID, 8, 6),
            ]
        );
    }

    #[tokio::test]
    async fn toolbar_keys_require_an_active_confirmed_connection() {
        for (label, expected) in [
            ("quick_home", Action::Home),
            ("quick_back", Action::Back),
            ("quick_recents", Action::Recents),
            ("quick_power", Action::Lock),
        ] {
            assert_eq!(Action::from_toolbar(label), Some(expected));
            assert!(send(Owner::new(true), expected).await.is_err());
        }
        assert_eq!(Action::from_toolbar("quick_paste"), None);
        assert!(setup(Owner::new(true), "confirm", Some(1)).await.is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires an enabled BLE peripheral adapter"]
    async fn windows_hid_service_can_be_reopened() {
        let owner = Owner::new(false);
        for _ in 0..2 {
            setup(owner, "start", None)
                .await
                .expect("start HID advertising");
            let slot = SESSION.lock().await;
            assert!(slot.as_ref().unwrap().hid.is_advertising().unwrap());
            drop(slot);
            let stopped = setup(owner, "stop", None)
                .await
                .expect("release HID service");
            assert_eq!(stopped.phase, "off");
            assert!(SESSION.lock().await.is_none());
        }
    }
}
