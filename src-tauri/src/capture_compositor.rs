//! Compose the Android screencap PNG with the existing transparent annotation
//! canvas from phone-frame-*. Independent of SDL/WebView2 window stacking:
//! no unrelated desktop/browser pixels, hover rail or phone shell are captured.
//! Uses only the PNG and base64 crates already present in Cargo.lock.

use std::io::Cursor;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use png::{BitDepth, ColorType, Decoder, Encoder, Transformations};

const MAX_OVERLAY_BASE64_BYTES: usize = 12 * 1024 * 1024;
const MAX_PIXELS: u64 = 16_000_000;
const MAX_COMPOSED_PNG: usize = 32 * 1024 * 1024;

struct Image {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

fn read_png(png_bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = Decoder::new(Cursor::new(png_bytes));
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
    let mut reader = decoder.read_info()
        .map_err(|e| format!("Invalid PNG header: {e}"))?;
    let (w, h) = (reader.info().width as usize, reader.info().height as usize);
    if w == 0 || h == 0 || w > 8192 || h > 8192 ||
        (w as u64).saturating_mul(h as u64) > MAX_PIXELS
    {
        return Err("Screenshot or overlay dimensions exceed the safe 16MP limit".into());
    }
    let mut data = vec![0u8; reader.output_buffer_size()];
    let decoded = reader.next_frame(&mut data)
        .map_err(|e| format!("Could not decode PNG: {e}"))?;
    if decoded.bit_depth != BitDepth::Eight {
        return Err("Only 8-bit Android and WebView2 PNG images are supported".into());
    }
    let raw = &data[..decoded.buffer_size()];
    let mut rgba = Vec::with_capacity(w * h * 4);
    match decoded.color_type {
        ColorType::Rgba => rgba.extend_from_slice(raw),
        ColorType::Rgb => for rgb in raw.chunks_exact(3) {
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        },
        ColorType::Grayscale => for &gray in raw {
            rgba.extend_from_slice(&[gray, gray, gray, 255]);
        },
        ColorType::GrayscaleAlpha => for px in raw.chunks_exact(2) {
            rgba.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
        },
        _ => return Err("Unsupported PNG color format".into()),
    }
    if rgba.len() != w * h * 4 {
        return Err("PNG pixel buffer has inconsistent dimensions".into());
    }
    Ok(Image { width: w, height: h, rgba })
}

fn encode_png(image: &Image) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    {
        let mut encoder = Encoder::new(&mut bytes, image.width as u32, image.height as u32);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        let mut writer = encoder.write_header()
            .map_err(|e| format!("Could not initialize annotated PNG: {e}"))?;
        writer.write_image_data(&image.rgba)
            .map_err(|e| format!("Could not compress annotated PNG: {e}"))?;
    }
    if bytes.len() > MAX_COMPOSED_PNG {
        return Err("Annotated PNG is larger than the 32MiB export limit".into());
    }
    Ok(bytes)
}

/// Straight-alpha source-over, sampling the transparent overlay with
/// bilinear interpolation into original device pixels. Interpolating
/// premultiplied RGB prevents dark halos at semi-transparent brush edges.
fn sample_overlay(overlay: &Image, x: usize, y: usize, w: usize, h: usize) -> [f64; 4] {
    let u = (x as f64 + 0.5) * (overlay.width as f64 / w as f64) - 0.5;
    let v = (y as f64 + 0.5) * (overlay.height as f64 / h as f64) - 0.5;
    let x0 = u.floor().clamp(0.0, (overlay.width - 1) as f64) as usize;
    let y0 = v.floor().clamp(0.0, (overlay.height - 1) as f64) as usize;
    let x1 = (x0 + 1).min(overlay.width - 1);
    let y1 = (y0 + 1).min(overlay.height - 1);
    let fx = u.clamp(0.0, (overlay.width - 1) as f64).fract();
    let fy = v.clamp(0.0, (overlay.height - 1) as f64).fract();
    let weights = [
        ((1.0 - fx) * (1.0 - fy), x0, y0),
        (fx * (1.0 - fy), x1, y0),
        ((1.0 - fx) * fy, x0, y1),
        (fx * fy, x1, y1),
    ];
    let mut result = [0.0; 4];
    for (weight, px, py) in weights {
        let start = (py * overlay.width + px) * 4;
        let alpha = overlay.rgba[start + 3] as f64 / 255.0;
        for c in 0..3 {
            result[c] += weight * alpha * overlay.rgba[start + c] as f64;
        }
        result[3] += weight * alpha;
    }
    result
}

pub(crate) fn composite_annotated_png(
    android_png: &[u8],
    overlay_data_url: &str,
    orientation: &str,
) -> Result<Vec<u8>, String> {
    let payload = overlay_data_url.strip_prefix("data:image/png;base64,")
        .ok_or("The annotation overlay returned a non-PNG image")?;
    if payload.len() > MAX_OVERLAY_BASE64_BYTES {
        return Err("Annotation image exceeded the 12MiB transfer limit".into());
    }
    let overlay_bytes = STANDARD.decode(payload)
        .map_err(|_| "Annotation image base64 encoding was invalid")?;
    let mut screenshot = read_png(android_png)?;
    let overlay = read_png(&overlay_bytes)?;

    let portrait = screenshot.height > screenshot.width;
    if !matches!(orientation, "portrait" | "landscape") ||
        (portrait && orientation != "portrait") ||
        (!portrait && screenshot.height != screenshot.width && orientation != "landscape")
    {
        return Err("Phone rotated while capturing. Retry in the current orientation.".into());
    }
    let ratio_a = screenshot.width as f64 / screenshot.height as f64;
    let ratio_b = overlay.width as f64 / overlay.height as f64;
    if ((ratio_a / ratio_b) - 1.0).abs() > 0.06 {
        return Err("Pen canvas and Android screenshot aspect ratios differ. Retry after rotation settles.".into());
    }

    for y in 0..screenshot.height {
        for x in 0..screenshot.width {
            let src = sample_overlay(&overlay, x, y, screenshot.width, screenshot.height);
            if src[3] <= 0.0 { continue; }
            let index = (y * screenshot.width + x) * 4;
            let dst_alpha = screenshot.rgba[index + 3] as f64 / 255.0;
            let out_alpha = src[3] + dst_alpha * (1.0 - src[3]);
            if out_alpha <= 0.0 { continue; }
            for c in 0..3 {
                let dst = screenshot.rgba[index + c] as f64 * dst_alpha * (1.0 - src[3]);
                screenshot.rgba[index + c] = ((src[c] + dst) / out_alpha).round().clamp(0.0, 255.0) as u8;
            }
            screenshot.rgba[index + 3] = (out_alpha * 255.0).round() as u8;
        }
    }
    encode_png(&screenshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rgba_png(width: usize, height: usize, pixels: Vec<u8>) -> Vec<u8> {
        encode_png(&Image { width, height, rgba: pixels }).unwrap()
    }

    #[test]
    fn merges_colored_ink_without_changing_transparent_pixels() {
        let base = rgba_png(4, 8, [10, 20, 50, 255].repeat(32));
        let mut overlay = [0, 0, 0, 0].repeat(8);
        // Half of a 2 x 4 transparent canvas is bright red; the remaining
        // alpha-zero section represents a clean or erased drawing area.
        for y in 0..4 {
            let offset = (y * 2) * 4;
            overlay[offset..offset + 4].copy_from_slice(&[240, 0, 0, 255]);
        }
        let ink = format!("data:image/png;base64,{}", STANDARD.encode(rgba_png(2, 4, overlay)));
        let composite = composite_annotated_png(&base, &ink, "portrait").unwrap();
        let image = read_png(&composite).unwrap();
        let red = &image.rgba[..4];
        assert_eq!(red, &[240, 0, 0, 255]);
        let clear = &image.rgba[((3 * 4 + 3) * 4)..((3 * 4 + 4) * 4)];
        assert_eq!(clear, &[10, 20, 50, 255]);
    }

    #[test]
    fn rejects_rotations_bad_aspect_and_untrusted_overlay() {
        let base = rgba_png(3, 6, [40, 40, 40, 255].repeat(18));
        let square = format!("data:image/png;base64,{}", STANDARD.encode(
            rgba_png(3, 3, [255, 0, 0, 255].repeat(9))
        ));
        assert!(composite_annotated_png(&base, &square, "portrait").is_err());
        let portrait = format!("data:image/png;base64,{}", STANDARD.encode(
            rgba_png(3, 6, [255, 0, 0, 255].repeat(18))
        ));
        assert!(composite_annotated_png(&base, &portrait, "landscape").is_err());
        assert!(composite_annotated_png(&base, "data:text/html;base64,AA==", "portrait").is_err());
        assert!(composite_annotated_png(&base, "data:image/png;base64,%%%%", "portrait").is_err());
    }
}
