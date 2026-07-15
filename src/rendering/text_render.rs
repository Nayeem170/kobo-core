pub mod blit;
pub mod fonts;

pub use blit::*;
pub use fonts::*;

const FONT_LATIN: &[u8] = include_bytes!("../../fonts/NotoSansLatin.ttf");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Latin,
    Bengali,
    Devanagari,
    Arabic,
    Hebrew,
    Cjk,
    Thai,
    Other,
}

impl Script {
    pub fn is_rtl(self) -> bool {
        matches!(self, Script::Arabic | Script::Hebrew)
    }

    pub fn uses_word_spacing(self) -> bool {
        !matches!(self, Script::Cjk | Script::Thai)
    }

    pub fn lang_tag(self) -> &'static str {
        match self {
            Script::Latin => "en-US",
            Script::Bengali => "bn-BD",
            Script::Devanagari => "hi-IN",
            Script::Arabic => "ar-SA",
            Script::Hebrew => "he-IL",
            Script::Cjk => "ja-JP",
            Script::Thai => "th-TH",
            Script::Other => "en-US",
        }
    }
}

pub fn detect_script(text: &str) -> Script {
    for c in text.chars() {
        if !c.is_alphabetic() {
            continue;
        }
        let code = c as u32;
        return match code {
            0x0980..=0x09FF => Script::Bengali,
            0x0900..=0x097F => Script::Devanagari,
            0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => Script::Arabic,
            0x0590..=0x05FF => Script::Hebrew,
            0x3040..=0x30FF | 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xAC00..=0xD7AF => Script::Cjk,
            0x0E00..=0x0E7F => Script::Thai,
            0x0000..=0x024F => Script::Latin,
            _ => Script::Other,
        };
    }
    Script::Latin
}

static WIDTH_CACHE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<(String, u32), f32>>,
> = std::sync::OnceLock::new();

pub(crate) fn width_cache(
) -> &'static std::sync::Mutex<std::collections::HashMap<(String, u32), f32>> {
    WIDTH_CACHE.get_or_init(std::sync::Mutex::default)
}

#[allow(dead_code)]
pub fn clear_width_cache() {
    if let Ok(mut cache) = width_cache().lock() {
        cache.clear();
    }
}

pub struct DecodedImage {
    pub rgb: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

pub struct DecodedRgba {
    pub rgba: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

pub fn decode_image(raw: &[u8], max_w: usize, max_h: usize) -> Option<DecodedImage> {
    let img = image::load_from_memory(raw).ok()?;
    let (ow, oh) = (img.width() as usize, img.height() as usize);
    if ow == 0 || oh == 0 {
        return None;
    }
    let scale = max_w as f32 / ow as f32;
    let mut nw = max_w;
    let mut nh = (oh as f32 * scale).round() as usize;
    if nh == 0 {
        return None;
    }
    if nh > max_h {
        let hscale = max_h as f32 / nh as f32;
        nh = max_h;
        nw = (nw as f32 * hscale).round() as usize;
        if nw == 0 {
            return None;
        }
    }
    let resized = img.resize(nw as u32, nh as u32, image::imageops::FilterType::Triangle);
    let rgb = resized.to_rgb8();
    let (rw, rh) = (rgb.width() as usize, rgb.height() as usize);
    Some(DecodedImage {
        rgb: rgb.into_raw(),
        width: rw,
        height: rh,
    })
}

pub fn decode_image_rgba(raw: &[u8], max_w: usize, max_h: usize) -> Option<DecodedRgba> {
    let img = image::load_from_memory(raw).ok()?;
    let (ow, oh) = (img.width() as usize, img.height() as usize);
    if ow == 0 || oh == 0 {
        return None;
    }
    let scale = max_w as f32 / ow as f32;
    let mut nw = max_w;
    let mut nh = (oh as f32 * scale).round() as usize;
    if nh == 0 {
        return None;
    }
    if nh > max_h {
        let hscale = max_h as f32 / nh as f32;
        nh = max_h;
        nw = (nw as f32 * hscale).round() as usize;
        if nw == 0 {
            return None;
        }
    }
    let resized = img.resize(nw as u32, nh as u32, image::imageops::FilterType::Triangle);
    let rgba = resized.to_rgba8();
    let (rw, rh) = (rgba.width() as usize, rgba.height() as usize);
    Some(DecodedRgba {
        rgba: rgba.into_raw(),
        width: rw,
        height: rh,
    })
}

pub fn blit_rgb565_image(
    buf: &mut [u8],
    buf_stride: usize,
    rgb: &[u8],
    iw: usize,
    ih: usize,
    ox: usize,
    oy: usize,
    max_w: usize,
    max_h: usize,
) {
    for ry in 0..ih {
        let py = oy + ry;
        if py >= max_h {
            break;
        }
        for rx in 0..iw {
            let px = ox + rx;
            if px >= max_w {
                break;
            }
            let idx = (ry * iw + rx) * 3;
            let r = rgb[idx] as u16;
            let g = rgb[idx + 1] as u16;
            let b = rgb[idx + 2] as u16;
            let r5 = (r >> 3) & 0x1f;
            let g6 = (g >> 2) & 0x3f;
            let b5 = (b >> 3) & 0x1f;
            let v = (r5 << 11) | (g6 << 5) | b5;
            let off = (py * buf_stride + px) * 2;
            if off + 2 > buf.len() {
                continue;
            }
            buf[off] = (v & 0xff) as u8;
            buf[off + 1] = (v >> 8) as u8;
        }
    }
}

pub fn blit_rgb565_image_alpha(
    buf: &mut [u8],
    buf_stride: usize,
    rgba: &[u8],
    iw: usize,
    ih: usize,
    ox: usize,
    oy: usize,
    max_w: usize,
    max_h: usize,
) {
    for ry in 0..ih {
        let py = oy + ry;
        if py >= max_h {
            break;
        }
        for rx in 0..iw {
            let px = ox + rx;
            if px >= max_w {
                break;
            }
            let idx = (ry * iw + rx) * 4;
            let a = rgba[idx + 3];
            if a == 0 {
                continue;
            }
            let r = rgba[idx] as u16;
            let g = rgba[idx + 1] as u16;
            let b = rgba[idx + 2] as u16;
            let r5 = (r >> 3) & 0x1f;
            let g6 = (g >> 2) & 0x3f;
            let b5 = (b >> 3) & 0x1f;
            let v = (r5 << 11) | (g6 << 5) | b5;
            let off = (py * buf_stride + px) * 2;
            if off + 2 > buf.len() {
                continue;
            }
            buf[off] = (v & 0xff) as u8;
            buf[off + 1] = (v >> 8) as u8;
        }
    }
}

#[cfg(test)]
mod tests;
