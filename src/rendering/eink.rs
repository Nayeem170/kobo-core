//! E-ink framebuffer constants, MXCFB ioctl structs, and row-diff helper.
//!
//! The `Fb` struct (mmap + ioctl + `present()`) stays in the app crate - it
//! needs the slint `Rgb565Pixel` type and runtime framebuffer access.

pub const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;
pub const FBIOGET_FSCREENINFO: libc::c_ulong = 0x4602;
pub const MXCFB_SEND_UPDATE: libc::c_ulong = 0x4024462E;

#[repr(C)]
#[derive(Default)]
pub struct FbVarScreeninfo {
    pub xres: u32,
    pub yres: u32,
    pub xres_virtual: u32,
    pub yres_virtual: u32,
    pub xoffset: u32,
    pub yoffset: u32,
    pub bits_per_pixel: u32,
    pub grayscale: u32,
    pub rest: [u32; 32],
}

#[repr(C)]
#[derive(Default)]
pub struct FbFixScreeninfo {
    id: [u8; 16],
    pub smem_start: usize,
    pub smem_len: u32,
    typ: u32,
    type_aux: u32,
    visual: u32,
    xpanstep: u16,
    ypanstep: u16,
    ywrapstep: u16,
    pub line_length: u32,
    rest: [u32; 16],
}

#[repr(C)]
pub struct MxcfbRect {
    pub top: u32,
    pub left: u32,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
pub struct MxcfbUpdateData {
    pub update_region: MxcfbRect,
    pub waveform_mode: u32,
    pub update_mode: u32,
    pub update_marker: u32,
    pub temp: u32,
    pub flags: u32,
}

pub const WAVE_INIT: u32 = 0;
pub const WAVE_DU: u32 = 1;
pub const WAVE_GC16: u32 = 2;
pub const WAVE_GL16: u32 = 3;
pub const WAVE_A2: u32 = 4;
pub const WAVE_GLR16: u32 = 5;
pub const WAVE_GLD16: u32 = 6;

/// Pick the best waveform for a given render scenario on Kaleido 3 color e-ink.
///
/// - `Transition`: panel open/close, chapter overlay toggle. Needs good clearing
///   to avoid ghosting, but no full flash. `GC16` partial clears better than
///   `GL16` for large-area changes.
/// - `Content`: regular text page updates. `GL16` has less ghosting on partial
///   updates than `GC16`, preserving color quality for highlighted text and
///   the reading cursor.
/// - `Animation`: spinner, loading bar. `A2` is fastest (monochrome).
pub fn waveform_for(scenario: RenderScenario) -> u32 {
    match scenario {
        RenderScenario::Transition => WAVE_GL16,
        RenderScenario::Content => WAVE_GL16,
        RenderScenario::Animation => WAVE_A2,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderScenario {
    Transition,
    Content,
    Animation,
}

/// Compare two RGB565 byte buffers (same dimensions) and return the dirty row
/// range aligned to 8-pixel boundaries. Returns `None` when buffers are
/// identical.
///
/// `prev` and `cur` are raw byte views of `w * h` RGB565 pixels (2 bytes each,
/// stride = `w * 2`). The caller is responsible for providing matching-length
/// slices - typically via a `rgb565_as_bytes` helper on the pixel type.
pub fn diff_rows(prev: &[u8], cur: &[u8], w: usize, h: usize) -> Option<(usize, usize)> {
    let mut min_y = h;
    let mut max_y = 0;
    let mut dirty = false;
    for y in 0..h {
        let base = y * w * 2;
        if prev[base..base + w * 2] != cur[base..base + w * 2] {
            dirty = true;
            if y < min_y {
                min_y = y;
            }
            if y > max_y {
                max_y = y;
            }
        }
    }
    if !dirty {
        return None;
    }
    const A: usize = 8;
    let top = (min_y / A) * A;
    let bottom = ((max_y + A) / A) * A;
    let rh = bottom.min(h) - top;
    Some((top, rh))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_buf(w: usize, h: usize, fill: u8) -> Vec<u8> {
        vec![fill; w * h * 2]
    }

    #[test]
    fn diff_rows_detects_change() {
        let w = 10;
        let h = 16;
        let prev = make_buf(w, h, 0);
        let mut cur = prev.clone();
        let px = (5 * w + 3) * 2;
        cur[px] = 1;
        cur[px + 1] = 1;
        let (top, rh) = diff_rows(&prev, &cur, w, h).unwrap();
        assert_eq!(top, 0);
        assert!(rh > 0);
    }

    #[test]
    fn diff_rows_returns_none_when_identical() {
        let w = 10;
        let h = 10;
        let buf = make_buf(w, h, 0);
        assert!(diff_rows(&buf, &buf, w, h).is_none());
    }

    #[test]
    fn diff_rows_aligns_to_8px_boundary() {
        let w = 10;
        let h = 32;
        let prev = make_buf(w, h, 0);
        let mut cur = prev.clone();
        let px = (13 * w) * 2;
        cur[px] = 1;
        let (top, rh) = diff_rows(&prev, &cur, w, h).unwrap();
        assert_eq!(top, 8);
        assert_eq!(rh, 8);
    }
}
