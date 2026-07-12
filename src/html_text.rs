//! Chapter XHTML -> plain text + a **char-offset->element map**.
//!
//! This is the basis for highlight: a `WordMark`'s word (from Edge-TTS) is a
//! substring of the synthesized utterance, which is a slice of the chapter
//! text. To highlight it we need to map a char-range in the chapter text back
//! to the DOM (so the Slint reader can draw a highlight over the right text).
//!
//! v0 granularity: one [`TextSegment`] per block element (paragraph / heading /
//! list item). Sentence-level highlight = locate the sentence's char-range
//! (the caller splits on sentence boundaries) and highlight that range.
//!
//! Resampling preserves time, so a `WordMark::time_s` maps directly to a
//! playback-clock instant at which the sentence containing that char-range
//! should be highlighted (see `spike-a0-findings.md` shared-clock decision).

#[cfg(feature = "reader")]
pub mod extract;
pub mod lines;

#[cfg(feature = "reader")]
pub use extract::{extract, segment_at, TextSegment};
pub use lines::{lines, sentence_index_at, Line};
