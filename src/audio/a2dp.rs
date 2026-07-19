// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! `AospA2dpSink` - drives the Android `audio.a2dp.default` HAL on the Kobo.
//!
//! Proven path (Spike A1): open the A2DP control Unix socket, send the AOSP
//! `audio_a2dp_hw` commands (`CHECK_READY`/`START`/`STOP`), then write S16LE
//! PCM to the data socket. `btservice` (separate from nickel) owns the sockets
//! and serves the paired BT headset - no nickel needed at runtime (Spike A2).
//!
//! Gap #5 (backpressure/pacing): the data socket is written with `write_all`,
//! which **blocks when the socket buffer is full**. Since the BT sink drains at
//! real time (44.1 kHz), this yields natural sample-clock pacing. Continuous
//! speech that doesn't underrun/garble is the C-play gate.
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

pub const CTRL_PATH: &str = "/tmp/audio.a2dp_ctrl";
pub const DATA_PATH: &str = "/tmp/audio.a2dp_data";

// AOSP audio_a2dp_hw command opcodes (audio_a2dp_hw.h)
const CMD_CHECK_READY: u8 = 0x01;
const CMD_START: u8 = 0x02;
const CMD_STOP: u8 = 0x03;
// ack values
const ACK_SUCCESS: u8 = 0x00;
// Max wait for btservice to respond (ctrl-socket ack) or drain (data-socket
// write) before treating the BT stack as wedged (gap #5 / review #2).
const BTSERVICE_TIMEOUT_SECS: u64 = 3;

#[derive(Debug, Error)]
pub enum A2dpError {
    #[error("connect {path}: {source}")]
    Connect {
        path: String,
        source: std::io::Error,
    },
    #[error("ctrl command {cmd:#04x} ack {ack:#04x} (expected {expected:#04x})")]
    BadAck { cmd: u8, ack: u8, expected: u8 },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// The sample format the A1 proof validated on the Havit headset.
/// (Edge-TTS PCM is 24 kHz mono -> resampled to this by [`crate::pipeline`].)
pub const TARGET_RATE: usize = 44_100;
pub const TARGET_CHANNELS: usize = 2;

pub struct AospA2dpSink {
    ctrl: UnixStream,
    data: Option<UnixStream>,
    started: bool,
}

impl AospA2dpSink {
    /// Open the control socket and verify the sink is ready.
    /// (The data socket is NOT connected here - it only exists after `start()`.)
    pub async fn open() -> Result<Self, A2dpError> {
        let ctrl = UnixStream::connect(CTRL_PATH)
            .await
            .map_err(|e| A2dpError::Connect {
                path: CTRL_PATH.into(),
                source: e,
            })?;
        let mut s = AospA2dpSink {
            ctrl,
            data: None,
            started: false,
        };
        s.ctrl_cmd(CMD_CHECK_READY, "CHECK_READY").await?;
        Ok(s)
    }

    async fn ctrl_cmd(&mut self, cmd: u8, _name: &str) -> Result<u8, A2dpError> {
        self.ctrl.write_all(&[cmd]).await?;
        let mut ack = [0u8; 1];
        // Distinguish timeout from a real read error (review #2): previously the
        // inner io::Result was dropped, so a read error left ack=[0x00] = false success.
        match tokio::time::timeout(
            Duration::from_secs(BTSERVICE_TIMEOUT_SECS),
            self.ctrl.read_exact(&mut ack),
        )
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => return Err(A2dpError::Io(e)),
            Err(_) => {
                return Err(A2dpError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "ctrl ack timeout",
                )))
            }
        }
        if (cmd == CMD_CHECK_READY || cmd == CMD_START) && ack[0] != ACK_SUCCESS {
            return Err(A2dpError::BadAck {
                cmd,
                ack: ack[0],
                expected: ACK_SUCCESS,
            });
        }
        Ok(ack[0])
    }

    /// START the stream and open the data socket (created by the HAL on START).
    pub async fn start(&mut self) -> Result<(), A2dpError> {
        self.ctrl_cmd(CMD_START, "START").await?;
        self.data = Some(
            UnixStream::connect(DATA_PATH)
                .await
                .map_err(|e| A2dpError::Connect {
                    path: DATA_PATH.into(),
                    source: e,
                })?,
        );
        self.started = true;
        Ok(())
    }

    /// Write PCM bytes (S16LE interleaved) to the A2DP data socket.
    /// Blocks on a full buffer -> real-time pacing (gap #5).
    pub async fn write_pcm(&mut self, pcm: &[u8]) -> Result<(), A2dpError> {
        match self.data.as_mut() {
            Some(d) => tokio::time::timeout(
                Duration::from_secs(BTSERVICE_TIMEOUT_SECS),
                d.write_all(pcm),
            )
            .await
            .map_err(|_| {
                A2dpError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "a2dp data write timeout (btservice not draining)",
                ))
            })?
            .map_err(A2dpError::Io),
            None => Err(A2dpError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "a2dp data socket not open (call start() first)",
            ))),
        }
    }

    /// Query the kernel socket send buffer for unsent bytes (TIOCOUTQ).
    /// This gives the TRUE buffered amount - no wall-clock drift.
    /// Returns bytes not yet consumed by the A2DP HAL.
    pub fn unsent_bytes(&self) -> usize {
        use std::os::unix::io::AsRawFd;
        if let Some(ref data) = self.data {
            let fd = data.as_raw_fd();
            let mut value: std::ffi::c_int = 0;
            // SAFETY: fd is a live socket descriptor (data owns it for this
            // call); TIOCOUTQ writes one int into &mut value (C-compatible),
            // no aliasing. A failing ioctl returns <0 and value stays 0.
            unsafe {
                libc::ioctl(fd, libc::TIOCOUTQ, &mut value);
            }
            value.max(0) as usize
        } else {
            0
        }
    }

    /// STOP the stream.
    pub async fn stop(&mut self) -> Result<(), A2dpError> {
        if self.started {
            // best-effort: STOP on an already-closing stream; Drop will clean up
            log::warn!("a2dp: best-effort STOP (error ignored if stream already closing)");
            let _ = self.ctrl_cmd(CMD_STOP, "STOP").await;
            self.started = false;
        }
        Ok(())
    }
}

impl Drop for AospA2dpSink {
    fn drop(&mut self) {
        // best-effort synchronous stop (the async stop() should be awaited first)
        use std::os::unix::net::UnixStream as StdStream;
        if self.started {
            if let Ok(mut c) = StdStream::connect(CTRL_PATH) {
                use std::io::Write;
                // best-effort: Drop runs during teardown; a write failure here
                // means the control socket is already gone (btservice stopped).
                if let Err(e) = c.write_all(&[CMD_STOP]) {
                    log::warn!("a2dp drop STOP write failed: {e}");
                }
            }
        }
    }
}
