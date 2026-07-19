// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! High-level TTS synthesis orchestration: retry/backoff, turn-end validation,
//! PCM-shortness ground-truth check, and inter-sentence gap baking.

use crate::audio::{EdgeTts, Engine, Player, Prepared, TtsEvent, SENTENCE_GAP_FRAMES, TARGET_RATE};
use log::{debug, info};
use std::time::Duration;

const TICKS_PER_SECOND: f64 = 10_000_000.0;
const STEREO_CHANNELS: f64 = 2.0;
const MIN_DETECTABLE_SPEECH_S: f64 = 0.5;
const PCM_SHORT_TOLERANCE: f64 = 1.0;
const PCM_SHORT_EPSILON_S: f64 = 0.05;
const MAX_ATTEMPTS: u32 = 3;
const BACKOFF_MS: [u64; 3] = [0, 250, 600];
/// Per-request timeout floor, for short sentences.
const SYNTH_TIMEOUT_BASE_SECS: u64 = 10;
/// Extra timeout budget per character of text. Edge streams audio at roughly
/// speaking speed, so a 300-character sentence legitimately takes far longer to
/// arrive than a short one. A flat timeout makes the SAME long sentence fail on
/// every run over a slow link, which reads as the reader always stopping in one
/// spot and skipping the rest of the book.
const SYNTH_TIMEOUT_PER_CHAR_MS: u64 = 20;
/// Ceiling, so a pathological sentence still fails in bounded time.
const SYNTH_TIMEOUT_MAX_SECS: u64 = 30;

/// Timeout for one synth attempt, scaled to how much audio the text implies.
fn synth_timeout_secs(text: &str) -> u64 {
    let extra = text.chars().count() as u64 * SYNTH_TIMEOUT_PER_CHAR_MS / 1000;
    (SYNTH_TIMEOUT_BASE_SECS + extra).min(SYNTH_TIMEOUT_MAX_SECS)
}

/// Normalise text before handing it to Edge TTS.
///
/// - Danda/double-danda (Devanagari/Bengali full stops) -> ". " so the engine
///   inserts a pause instead of running sentences together.
/// - Bengali YA + nukta -> YYA: Edge mis-shapes the conjunct; the precomposed
///   codepoint renders correctly.
/// - Smart quotes stripped: Edge reads them literally or skips them.
fn normalize_tts_text(text: &str) -> String {
    text.replace('\u{0964}', ". ")
        .replace('\u{0965}', ". ")
        .replace("\u{09AF}\u{09BC}", "\u{09DF}")
        .replace(['\u{201C}', '\u{201D}', '"', '\u{2018}', '\u{2019}'], "")
        .trim()
        .to_string()
}

pub async fn synthesize_prepared(
    utt_idx: usize,
    text: &str,
    voice: &str,
    rate: &str,
    lang: &str,
) -> Result<Prepared, String> {
    let text = normalize_tts_text(text);
    let mut last_err: Option<String> = None;

    for attempt in 0..MAX_ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(BACKOFF_MS[attempt as usize])).await;
            info!(
                "synth #{utt_idx}: retry {attempt}/{MAX_ATTEMPTS} ({})",
                last_err.as_deref().unwrap_or("?")
            );
        }

        let timeout_secs = synth_timeout_secs(&text);
        let net_t0 = std::time::Instant::now();
        let synth_result = tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            EdgeTts.synthesize(&text, voice, rate, lang),
        )
        .await;

        let events = match synth_result {
            Ok(Ok(ev)) => ev,
            Ok(Err(e)) => {
                info!("synth #{utt_idx}: TTS error: {e}");
                last_err = Some(format!("synth: {e}"));
                continue;
            }
            Err(_) => {
                info!(
                    "synth #{utt_idx}: TTS timeout after {timeout_secs}s ({} chars)",
                    text.chars().count()
                );
                last_err = Some(format!("synth: timeout after {timeout_secs}s"));
                continue;
            }
        };
        let net_ms = net_t0.elapsed().as_millis();

        let complete = events.iter().any(|e| matches!(e, TtsEvent::TurnEnd));
        if !complete {
            last_err = Some("synth: truncated (no turn.end marker - network drop)".into());
            continue;
        }

        let prep = match tokio::task::spawn_blocking(move || Player::prepare(&events))
            .await
            .map_err(|e| format!("prepare join: {e}"))?
        {
            Ok(p) => p,
            Err(e) => {
                last_err = Some(format!("prepare: {e}"));
                continue;
            }
        };

        let expected_speech_s = prep
            .bounds
            .iter()
            .map(|(ticks, _)| *ticks as f64 / TICKS_PER_SECOND)
            .next_back()
            .unwrap_or(0.0);
        let actual_s = prep.stereo.len() as f64 / (TARGET_RATE as f64 * STEREO_CHANNELS);
        if expected_speech_s > MIN_DETECTABLE_SPEECH_S
            && actual_s < expected_speech_s * PCM_SHORT_TOLERANCE - PCM_SHORT_EPSILON_S
        {
            last_err = Some(format!(
                "synth: short PCM {:.2}s vs {:.2}s expected (dropped audio frames)",
                actual_s, expected_speech_s
            ));
            continue;
        }

        let gap_frames = SENTENCE_GAP_FRAMES;
        let mut prep = prep;
        prep.stereo
            .extend(std::iter::repeat_n(0i16, gap_frames * 2));
        debug!(
            "synth #{utt_idx}: ok after {attempt} retr{}, {net_ms}ms net, {} samples incl. {gap_frames} gap, {:.1}s audio",
            if attempt == 1 { "y" } else { "ies" },
            prep.stereo.len() / 2,
            prep.stereo.len() as f64 / (TARGET_RATE as f64 * STEREO_CHANNELS)
        );
        return Ok(prep);
    }
    Err(last_err.unwrap_or_else(|| "synth: exhausted retries".into()))
}

#[cfg(test)]
mod tests {
    use super::normalize_tts_text;

    #[test]
    fn danda_becomes_period_space() {
        assert_eq!(
            normalize_tts_text("part one\u{0964}part two"),
            "part one. part two"
        );
    }

    #[test]
    fn double_danda_becomes_period_space() {
        assert_eq!(normalize_tts_text("end\u{0965}"), "end.");
    }

    #[test]
    fn bengali_ya_nukta_becomes_yya() {
        assert_eq!(normalize_tts_text("\u{09AF}\u{09BC}"), "\u{09DF}");
    }

    #[test]
    fn smart_quotes_stripped() {
        assert_eq!(
            normalize_tts_text("\u{201C}hello\u{201D} \u{2018}world\u{2019}"),
            "hello world"
        );
    }

    #[test]
    fn leading_trailing_whitespace_trimmed() {
        assert_eq!(normalize_tts_text("  hi  "), "hi");
    }

    #[test]
    fn plain_text_unchanged() {
        assert_eq!(normalize_tts_text("Hello world."), "Hello world.");
    }
}
