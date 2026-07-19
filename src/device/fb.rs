// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Framebuffer: mmap'd `/dev/fb0` with MXCFB e-ink refresh.
//!
//! Takes raw `&[u8]` RGB565 buffers (2 bytes/pixel, little-endian) so the
//! caller is not locked into a specific pixel wrapper type (slint's
//! `Rgb565Pixel`, a plain `u16` newtype, etc.). The app crate provides a
//! `rgb565_as_bytes_ref` helper to convert from `&[Rgb565Pixel]` at the
//! call site.

use crate::rendering::eink::{
    FbFixScreeninfo, FbVarScreeninfo, MxcfbRect, MxcfbUpdateData, FBIOGET_FSCREENINFO,
    FBIOGET_VSCREENINFO, MXCFB_SEND_UPDATE,
};
use log::{debug, info, warn};

const UPDATE_MARKER: u32 = 1;
const WAIT_FALLBACK_MS: u64 = 400;

#[derive(Debug, Clone, Copy)]
pub struct UpdateRegion {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

pub fn dump_ppm(path: &str, buf: &[u8], w: usize, h: usize) {
    let mut out = Vec::with_capacity(15 + w * h * 3);
    out.extend_from_slice(format!("P6\n{} {}\n255\n", w, h).as_bytes());
    for i in 0..w * h {
        let off = i * 2;
        let v = (buf[off] as u16) | ((buf[off + 1] as u16) << 8);
        let r = ((v >> 11) & 0x1f) as u8;
        let g = ((v >> 5) & 0x3f) as u8;
        let b = (v & 0x1f) as u8;
        out.push((r << 3) | (r >> 2));
        out.push((g << 2) | (g >> 4));
        out.push((b << 3) | (b >> 2));
    }
    match std::fs::write(path, &out) {
        Ok(_) => debug!("wrote {} ({} bytes)", path, out.len()),
        Err(e) => warn!("PPM write err: {}", e),
    }
}

pub struct Fb {
    _file: std::fs::File,
    pub fd: libc::c_int,
    pub ptr: *mut u8,
    pub map_len: usize,
    pub stride: usize,
    pub bpp: usize,
    pub xres: usize,
    pub yres: usize,
    r_off: u32,
    g_off: u32,
    b_off: u32,
}

impl Fb {
    pub fn open() -> Option<Fb> {
        use std::os::unix::io::AsRawFd;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/fb0")
            .ok()?;
        let fd = file.as_raw_fd();
        let mut var = FbVarScreeninfo::default();
        let mut fix = FbFixScreeninfo::default();
        // SAFETY: fd is a valid /dev/fb0 descriptor (file alive on this frame). FBIOGET_* read
        // one fb_var/fb_fix screeninfo into the supplied &mut; both are #[repr(C)] structs of
        // the kernel-expected layout, exclusively borrowed. A failing ioctl returns <0 and we
        // bail; on success the structs are fully overwritten with valid values.
        unsafe {
            if libc::ioctl(fd, FBIOGET_VSCREENINFO as _, &mut var) < 0 {
                return None;
            }
            if libc::ioctl(fd, FBIOGET_FSCREENINFO as _, &mut fix) < 0 {
                return None;
            }
        }
        let map_len = fix.smem_len as usize;
        // SAFETY: mmap of the framebuffer with MAP_SHARED over the valid fd. map_len comes
        // straight from the kernel's fix.smem_len. NULL addr = kernel picks. We check for
        // MAP_FAILED immediately and bail; the returned pointer is the device-backed mapping
        // owned by Fb (unmapped in Drop).
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                map_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return None;
        }
        info!(
            "fb: {}x{} bpp={} line_length={} smem_len={}",
            var.xres, var.yres, var.bits_per_pixel, fix.line_length, fix.smem_len
        );
        let (r_off, g_off, b_off) = (
            var.rest.first().copied().unwrap_or(16),
            var.rest.get(3).copied().unwrap_or(8),
            var.rest.get(6).copied().unwrap_or(0),
        );
        info!("fb: rgb offsets r={} g={} b={}", r_off, g_off, b_off);
        Some(Fb {
            _file: file,
            fd,
            ptr: ptr as *mut u8,
            map_len,
            stride: fix.line_length as usize,
            bpp: var.bits_per_pixel as usize,
            xres: var.xres as usize,
            yres: var.yres as usize,
            r_off,
            g_off,
            b_off,
        })
    }

    /// Blit and refresh an arbitrary rectangle, rather than `present`'s
    /// full-width band.
    ///
    /// Needed when a waveform must be kept off neighbouring pixels: a 1-bit
    /// waveform (A2/DU) drives every pixel in its update region to black or
    /// white, so a full-width band would flatten anything colourful sharing those
    /// rows. Bounding the region horizontally keeps the waveform on the animated
    /// area alone.
    pub fn present_rect(&self, buf: &[u8], w: usize, h: usize, rect: &UpdateRegion, waveform: u32) {
        let x0 = rect.x.min(self.xres);
        let y0 = rect.y.min(self.yres);
        let x1 = (rect.x + rect.w).min(w).min(self.xres);
        let y1 = (rect.y + rect.h).min(h).min(self.yres);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        // SAFETY: identical to `present` -- self.ptr is the mmap'd framebuffer of
        // length self.map_len, and we only write fb pixels through this &self.
        let fb = unsafe { std::slice::from_raw_parts_mut(self.ptr, self.map_len) };
        let bpp = self.bpp;
        let stride = self.stride;
        for py in y0..y1 {
            let row = py * w;
            let fb_row = py * stride;
            for px in x0..x1 {
                let buf_off = (row + px) * 2;
                let pix = (buf[buf_off] as u16) | ((buf[buf_off + 1] as u16) << 8);
                let off = fb_row + px * (bpp / 8);
                match bpp {
                    32 => {
                        let r = (((pix >> 11) & 0x1f) << 3) as u8;
                        let g = (((pix >> 5) & 0x3f) << 2) as u8;
                        let b = ((pix & 0x1f) << 3) as u8;
                        fb[off + (self.r_off / 8) as usize] = r;
                        fb[off + (self.g_off / 8) as usize] = g;
                        fb[off + (self.b_off / 8) as usize] = b;
                        fb[off + 3] = 0xff;
                    }
                    16 => {
                        fb[off] = (pix & 0xff) as u8;
                        fb[off + 1] = (pix >> 8) as u8;
                    }
                    _ => {}
                }
            }
        }
        // msync whole rows spanning the rect: page granularity makes a tighter
        // flush pointless, and MS_SYNC on the row span is what the ioctl needs.
        let sync_start = y0 * stride;
        let sync_len = ((y1 - y0) * stride).max(1);
        // SAFETY: sync_start + sync_len <= yres*stride <= map_len (y1 clamped to
        // yres above), so the range stays inside the mapping.
        unsafe {
            libc::msync(
                self.ptr.add(sync_start) as *mut libc::c_void,
                sync_len,
                libc::MS_SYNC,
            );
        }
        let upd = MxcfbUpdateData {
            update_region: MxcfbRect {
                top: y0 as u32,
                left: x0 as u32,
                width: (x1 - x0) as u32,
                height: (y1 - y0) as u32,
            },
            waveform_mode: waveform,
            update_mode: 0,
            update_marker: UPDATE_MARKER,
            temp: 0x1000,
            flags: 0,
        };
        // SAFETY: MXCFB_SEND_UPDATE reads one initialized #[repr(C)]
        // MxcfbUpdateData; self.fd is the valid fb0 descriptor. rc<0 is non-fatal.
        let rc = unsafe { libc::ioctl(self.fd, MXCFB_SEND_UPDATE as _, &upd) };
        debug!(
            "eink refresh (RECT wf={} x=[{},{}] y=[{},{}]) rc={}",
            waveform, x0, x1, y0, y1, rc
        );
    }

    /// Blit an RGB565 byte buffer to the framebuffer and trigger an e-ink
    /// refresh. `buf` is `w * h * 2` bytes, little-endian RGB565.
    pub fn present(
        &self,
        buf: &[u8],
        w: usize,
        h: usize,
        full: bool,
        top: usize,
        rh: usize,
        waveform: u32,
    ) {
        // SAFETY: self.ptr is the mmap'd framebuffer of length self.map_len (set in open(),
        // unmapped in Drop). We hold &self (shared) but only write fb pixels here - no other
        // aliasing byte slice of this mapping is live concurrently. The slice length is exactly
        // map_len, matching the mapping.
        let fb = unsafe { std::slice::from_raw_parts_mut(self.ptr, self.map_len) };
        let bpp = self.bpp;
        let stride = self.stride;
        let (y0, y1) = if full {
            (0, h.min(self.yres))
        } else {
            let end = (top + rh).min(h).min(self.yres);
            (top.min(end), end)
        };
        let x1 = w.min(self.xres);
        for y in y0..y1 {
            let row = y * w;
            let fb_row = y * stride;
            for x in 0..x1 {
                let buf_off = (row + x) * 2;
                let px = (buf[buf_off] as u16) | ((buf[buf_off + 1] as u16) << 8);
                let off = fb_row + x * (bpp / 8);
                match bpp {
                    32 => {
                        let r = (((px >> 11) & 0x1f) << 3) as u8;
                        let g = (((px >> 5) & 0x3f) << 2) as u8;
                        let b = ((px & 0x1f) << 3) as u8;
                        fb[off + (self.r_off / 8) as usize] = r;
                        fb[off + (self.g_off / 8) as usize] = g;
                        fb[off + (self.b_off / 8) as usize] = b;
                        fb[off + 3] = 0xff;
                    }
                    16 => {
                        fb[off] = (px & 0xff) as u8;
                        fb[off + 1] = (px >> 8) as u8;
                    }
                    _ => {}
                }
            }
        }
        let sync_start = y0 * self.stride;
        let sync_len = ((y1 - y0) * self.stride).max(1);
        // SAFETY: self.ptr.add(sync_start) stays within [self.ptr, self.ptr+map_len) because
        // sync_start = y0*stride and sync_len = (y1-y0)*stride, with y0/y1 clamped to yres and
        // stride*xres*... <= map_len for a linear framebuffer. MS_SYNC flushes the dirty pages.
        unsafe {
            libc::msync(
                self.ptr.add(sync_start) as *mut libc::c_void,
                sync_len,
                libc::MS_SYNC,
            );
        }
        let upd = MxcfbUpdateData {
            update_region: MxcfbRect {
                top: y0 as u32,
                left: 0,
                width: x1 as u32,
                height: (y1 - y0) as u32,
            },
            waveform_mode: waveform,
            update_mode: if full { 1 } else { 0 },
            update_marker: UPDATE_MARKER,
            temp: 0x1000,
            flags: 0,
        };
        // SAFETY: MXCFB_SEND_UPDATE ioctl reads one #[repr(C)] MxcfbUpdateData from the &upd
        // pointer (fully initialized above) to schedule the e-ink refresh. self.fd is the
        // valid fb0 descriptor; a failing ioctl returns <0 (logged) and is non-fatal.
        let rc = unsafe { libc::ioctl(self.fd, MXCFB_SEND_UPDATE as _, &upd) };
        debug!(
            "eink refresh (GC16 {} rows=[{},{}] {}x{}) rc={}",
            if full { "FULL" } else { "PARTIAL" },
            y0,
            y1,
            x1,
            y1 - y0,
            rc
        );
    }

    pub fn wait_for_update_complete(&self) {
        let marker: u32 = UPDATE_MARKER;
        // SAFETY: self.fd is the valid fb0 descriptor opened in Fb::open; marker is a stack u32
        // passed by const ptr, read-once by the ioctl to match the update_marker from present().
        let rc = unsafe {
            libc::ioctl(
                self.fd,
                crate::rendering::eink::MXCFB_WAIT_FOR_UPDATE_COMPLETE as _,
                &marker as *const u32,
            )
        };
        if rc < 0 {
            std::thread::sleep(std::time::Duration::from_millis(WAIT_FALLBACK_MS));
        }
    }
}

impl Drop for Fb {
    fn drop(&mut self) {
        // SAFETY: self.ptr/self.map_len describe the single mmap acquired in open(); Drop runs
        // once, no other reference to the mapping exists (file dropped right after), so
        // munmap is sound and releases the device mapping.
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, self.map_len);
        }
    }
}
