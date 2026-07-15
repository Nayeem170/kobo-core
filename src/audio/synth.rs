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
const SYNTH_TIMEOUT_SECS: u64 = 10;

pub async fn synthesize_prepared(
    utt_idx: usize,
    text: &str,
    voice: &str,
    rate: &str,
    lang: &str,
) -> Result<Prepared, String> {
    let mut last_err: Option<String> = None;

    for attempt in 0..MAX_ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(BACKOFF_MS[attempt as usize])).await;
            info!(
                "synth #{utt_idx}: retry {attempt}/{MAX_ATTEMPTS} ({})",
                last_err.as_deref().unwrap_or("?")
            );
        }

        let net_t0 = std::time::Instant::now();
        let synth_result = tokio::time::timeout(
            Duration::from_secs(SYNTH_TIMEOUT_SECS),
            EdgeTts.synthesize(text, voice, rate, lang),
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
                info!("synth #{utt_idx}: TTS timeout after {SYNTH_TIMEOUT_SECS}s");
                last_err = Some(format!("synth: timeout after {SYNTH_TIMEOUT_SECS}s"));
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
