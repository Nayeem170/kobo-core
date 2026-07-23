// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_ABS: u16 = 0x03;
pub const SYN_REPORT: u16 = 0x00;
pub const ABS_MT_POSITION_X: u16 = 0x35;
pub const ABS_MT_POSITION_Y: u16 = 0x36;
pub const BTN_TOUCH_CODE: u16 = 330;

pub const EVIOCGABS_X: libc::c_ulong = 0x80184572;
pub const EVIOCGABS_Y: libc::c_ulong = 0x80184576;

/// Decode (type, code, value) from a 16-byte arm evdev `input_event`. The first
/// 8 bytes are the timeval timestamp, ignored.
pub fn decode_input_event(buf: &[u8; 16]) -> (u16, u16, i32) {
    (
        u16::from_le_bytes([buf[8], buf[9]]),
        u16::from_le_bytes([buf[10], buf[11]]),
        i32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
    )
}

#[repr(C)]
#[derive(Default)]
pub struct InputAbsinfo {
    pub value: i32,
    pub minimum: i32,
    pub maximum: i32,
    pub fuzz: i32,
    pub flat: i32,
    pub resolution: i32,
}

pub fn query_abs_max(fd: libc::c_int, ioctl: libc::c_ulong) -> i32 {
    let mut info = InputAbsinfo::default();
    // SAFETY: ioctl with an EVIOCGABS-class code writes one `struct input_absinfo` into the
    // supplied &mut. `info` is a valid, initialized, exclusively-borrowed value of exactly
    // that layout (C-compatible #[repr(C)] struct of 5 i32). fd/ioctl are caller-provided
    // kernel tokens; a bad fd returns -1 (ignored) and `info` stays default.
    unsafe {
        libc::ioctl(fd, ioctl as _, &mut info);
    }
    info.maximum
}
