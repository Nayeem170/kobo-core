// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
#![doc = include_str!("../README.md")]

#[cfg(feature = "audio")]
pub mod audio;
pub mod capabilities;
pub mod clock;
pub mod device;
#[cfg(feature = "reader")]
pub mod formats;
pub mod html_text;
pub mod logger;
pub mod rendering;
#[cfg(feature = "audio")]
pub mod stub_player;

pub use capabilities::{Capabilities, MockCapabilities};
pub use clock::{highlight_index, SharedClock};

#[cfg(feature = "reader")]
pub use formats::epub::{Chapter, EpubBook, EpubError};
#[cfg(feature = "reader")]
pub use html_text::TextSegment;

pub use html_text::{lines, sentence_index_at, Line};

#[cfg(feature = "audio")]
pub use stub_player::StubPlayer;
