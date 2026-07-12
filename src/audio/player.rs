//! `Player` - the streaming Read Aloud audio spine (review #1 + #3).
//!
//! - **Bounded / per-utterance (#3):** the caller splits text into utterances
//!   and calls [`Player::play_utterance`] for each. Each utterance's MP3 is
//!   decoded/resampled/written in isolation (bounded RAM, not whole-chapter),
//!   and `write_all` blocks on the A2DP socket's full buffer -> real-time pacing.
//! - **Drain before STOP (#1):** [`Player::drain_and_stop`] waits for the sink's
//!   buffered audio to finish playing before issuing STOP, so a chapter's last
//!   sentence isn't truncated (the spike's bug).
//!
//! Frame-level streaming within an utterance is a further optimization; this is
//! the streaming v0 that fixes both #1 and #3.
use super::a2dp::{A2dpError, AospA2dpSink, TARGET_RATE};
use super::edge_tts::TtsEvent;
use super::pipeline;
use std::time::{Duration, Instant};
use thiserror::Error;

const EDGE_TICKS_PER_SECOND: f64 = 10_000_000.0;
const BYTES_PER_STEREO_FRAME: u64 = 4;

/// Safety pad added to the drain wait to cover A2DP startup + headset-buffer
/// latency (>=100-300 ms) that the wall-clock estimate ignores (review #1-residual).
/// Until the sink exposes a TX-queue-empty poll (SIOCOUTQ), this prevents the
/// BT-pipeline tail from clipping on longer/slower content.
const DRAIN_SAFETY_PAD: f64 = 0.5;

/// One word-boundary mark on the playback timeline (review: highlight data).
/// `time_s` is the absolute playback time (seconds from the first sample) at
/// which this word should be highlighted; `text` is the word.
#[derive(Debug, Clone)]
pub struct WordMark {
    pub time_s: f64,
    pub text: String,
}

/// A decoded, resampled, stereo-ized utterance ready to write to the sink - the
/// pure-CPU output of [`Player::prepare`] (no I/O). The paced-write driver owns
/// one of these and feeds it to the sink in small chunks, so a Pause aborts the
/// write with only a tiny lead buffered (~[`LEAD_FRAMES`] worth), instead of the
/// whole-utterance burst that `play_utterance` issues.
#[derive(Debug, Clone, Default)]
pub struct Prepared {
    /// Interleaved stereo S16LE @ [`TARGET_RATE`] (2 i16 per frame).
    pub stereo: Vec<i16>,
    /// Word boundaries in 100-ns ticks from the utterance start (for 3b highlight).
    pub bounds: Vec<(u64, String)>,
}

/// Chunk size for paced writes: ~50 ms of stereo frames @ [`TARGET_RATE`].
/// (50 ms x 44 100 = 2205 frames.) Tunable on device.
pub const CHUNK_FRAMES: usize = 2_205;
/// Write-ahead lead to keep in the sink: ~200 ms of stereo frames. The paced
/// loop stops feeding once `frames_written - playback >= LEAD_FRAMES`, so a Pause
/// leaves at most this much buffered -> pause latency ~ 200 ms (vs the 5-10 s
/// burst buffer). Start here; raise if the device underruns (garble), lower if
/// pause feels sluggish.
pub const LEAD_FRAMES: u64 = 8_820;
/// Silence lead-in written right after START: ~500 ms. The A2DP HAL clips/
/// distorts the first audio after START (codec + link ramp-up), so we feed
/// silence first - the artifact consumes silence, not the first spoken word.
/// NOTE: on-device capture confirmed our sent data is clean (pure silence +
/// clean speech at ~657 ms); a residual startup noise remains that is
/// downstream of our pipeline (HAL/headset amp ramp) - not fixable from here
/// beyond this lead-in. Tunable.
pub const LEAD_IN_FRAMES: usize = TARGET_RATE / 2;

/// Inter-sentence silence baked into each utterance's PCM (~400 ms).
pub const SENTENCE_GAP_FRAMES: usize = TARGET_RATE / 1000 * 400;

/// Extra silence after a paragraph-ending sentence (~700 ms total).
pub const PARA_GAP_FRAMES: usize = TARGET_RATE / 1000 * 700;

#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("a2dp: {0}")]
    A2dp(#[from] A2dpError),
    #[error("pipeline: {0}")]
    Pipeline(String),
}

pub struct Player {
    sink: AospA2dpSink,
    started_at: Option<Instant>,
    frames_written: u64,
    clock_base: u64,
    clock_anchor: Option<Instant>,
    paused_since: Option<Instant>,
    chunk_buf: Vec<u8>,
    silence_buf: Vec<u8>,
}

impl Player {
    /// Open the control socket + START the A2DP stream.
    pub async fn open() -> Result<Self, PlayerError> {
        let mut sink = AospA2dpSink::open().await?;
        sink.start().await?;
        Ok(Player {
            sink,
            started_at: None,
            frames_written: 0,
            clock_base: 0,
            clock_anchor: None,
            paused_since: None,
            chunk_buf: vec![0u8; CHUNK_FRAMES * BYTES_PER_STEREO_FRAME as usize],
            silence_buf: vec![0u8; CHUNK_FRAMES * BYTES_PER_STEREO_FRAME as usize],
        })
    }

    /// Pure-CPU prep for the paced-write path: collect MP3 + word bounds, then
    /// decode -> resample 24k->44.1k -> mono->stereo. No I/O - the driver controls
    /// the actual writes (so it can poll for Pause/Stop between chunks).
    pub fn prepare(events: &[TtsEvent]) -> Result<Prepared, PlayerError> {
        let mut mp3: Vec<u8> = Vec::new();
        let mut bounds: Vec<(u64, String)> = Vec::new();
        for ev in events {
            match ev {
                TtsEvent::Audio(b) => mp3.extend_from_slice(b),
                TtsEvent::WordBoundary { offset, text, .. } => bounds.push((*offset, text.clone())),
                TtsEvent::TurnEnd => {}
            }
        }
        if mp3.is_empty() {
            return Ok(Prepared {
                stereo: Vec::new(),
                bounds,
            });
        }
        let mono = pipeline::decode_mp3(&mp3).map_err(PlayerError::Pipeline)?;
        let resampled = pipeline::resample_mono(&mono).map_err(PlayerError::Pipeline)?;
        let stereo = pipeline::mono_to_stereo(&resampled);
        Ok(Prepared { stereo, bounds })
    }

    /// Playback-clock start (absolute seconds since the first sample) for the
    /// NEXT utterance's [`WordMark`]s - = frames already queued / rate. Capture
    /// this before writing an utterance, then pass to [`Player::marks`].
    pub fn next_utt_start_s(&self) -> f64 {
        self.frames_written as f64 / TARGET_RATE as f64
    }

    /// Map a prepared utterance's word bounds to absolute-playback-time
    /// [`WordMark`]s, given the utterance's start (from [`Player::next_utt_start_s`]).
    pub fn marks(prepared: &Prepared, utt_start_s: f64) -> Vec<WordMark> {
        prepared
            .bounds
            .iter()
            .map(|(ticks, text)| WordMark {
                time_s: utt_start_s + *ticks as f64 / EDGE_TICKS_PER_SECOND,
                text: text.clone(),
            })
            .collect()
    }

    /// Write one chunk of interleaved stereo S16LE to the sink and advance
    /// `frames_written`. Blocks on the socket's (now small) buffer. The driver
    /// calls this only when the lead is below [`LEAD_FRAMES`].
    pub async fn write_chunk(&mut self, stereo: &[i16]) -> Result<(), PlayerError> {
        if self.started_at.is_none() {
            self.started_at = Some(Instant::now());
        }
        let n = stereo.len() * 2;
        self.chunk_buf.resize(n, 0);
        for (i, s) in stereo.iter().enumerate() {
            self.chunk_buf[i * 2] = (*s & 0xFF) as u8;
            self.chunk_buf[i * 2 + 1] = (*s >> 8) as u8;
        }
        self.sink.write_pcm(&self.chunk_buf[..n]).await?;
        self.frames_written += (stereo.len() / 2) as u64;
        Ok(())
    }

    /// Trickle one chunk (~50 ms) of silence to the A2DP data socket while idle
    /// (paused/stopped), to keep the link warm so the paired headset doesn't
    /// drop it - the idle drop is what wedges `btservice` into `ctrl-ack-timeout`
    /// (findings A5). Transparent to the pacing/resume model: does NOT touch
    /// `frames_written` or the playback clock. Data-socket only -> no START/STOP,
    /// so no churn fatigue (unlike A4).
    pub async fn keepalive(&mut self) -> Result<(), PlayerError> {
        self.silence_buf.fill(0);
        self.sink.write_pcm(&self.silence_buf).await?;
        Ok(())
    }

    /// Frames the sink has played per the pausable clock (frozen while paused).
    pub fn playback_frames(&self) -> u64 {
        let span = match self.clock_anchor {
            Some(a) => (TARGET_RATE as f64 * a.elapsed().as_secs_f64()) as u64,
            None => 0,
        };
        self.clock_base + span
    }

    /// Stereo frames written but (per the clock) not yet played - the write-ahead
    /// lead the paced loop caps at [`LEAD_FRAMES`].
    pub fn lead_frames(&self) -> u64 {
        self.frames_written.saturating_sub(self.playback_frames())
    }

    /// ACTUAL stereo frames buffered in the kernel socket (via TIOCOUTQ).
    /// Unlike lead_frames() (wall-clock estimate), this is a direct measurement
    /// - no drift. Use for pacing to avoid both underrun (fot-fot) and
    /// unbounded buffer (E8: pause latency + resume overlap).
    pub fn socket_buffered_frames(&self) -> u64 {
        self.sink.unsent_bytes() as u64 / 4 // 2 bytes/sample x 2 channels
    }

    /// Total stereo frames pushed into the sink since open (2 i16 per frame).
    /// Used for telemetry + (3b) absolute WordMark timing.
    pub fn frames_written(&self) -> u64 {
        self.frames_written
    }

    /// Start (or resume) the playback clock. Idempotent - only sets the anchor if
    /// the clock is currently frozen. Call when (re)entering playback so the lead
    /// can drain even before the next write.
    pub fn resume_clock(&mut self) {
        if self.clock_anchor.is_none() {
            self.clock_anchor = Some(Instant::now());
        }
        self.paused_since = None;
    }

    pub fn pause_clock(&mut self) {
        self.clock_base = self.frames_written;
        self.clock_anchor = None;
        self.paused_since = Some(Instant::now());
    }

    pub fn paused_since(&self) -> Option<Duration> {
        self.paused_since.map(|t| t.elapsed())
    }

    /// Play one utterance's events (decode -> resample 24k->44.1k -> mono->stereo ->
    /// write to the sink, paced by backpressure). Bounded to one utterance.
    ///
    /// Returns the utterance's [`WordMark`]s tagged with **absolute playback
    /// time** (review: surface WordBoundary - the data highlight depends on),
    /// mapped as: `time_s = utterance_playback_start + edge_offset_seconds`,
    /// where the edge offset is in 100-ns ticks (resampling preserves time).
    pub async fn play_utterance(
        &mut self,
        events: &[TtsEvent],
    ) -> Result<Vec<WordMark>, PlayerError> {
        let prepared = Self::prepare(events)?;
        if prepared.stereo.is_empty() {
            return Ok(Vec::new());
        }
        if self.started_at.is_none() {
            self.started_at = Some(Instant::now());
        }
        let utt_start_s = self.frames_written as f64 / TARGET_RATE as f64;
        let bytes: Vec<u8> = prepared
            .stereo
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        self.sink.write_pcm(&bytes).await?;
        self.frames_written += (prepared.stereo.len() / 2) as u64;
        Ok(Self::marks(&prepared, utt_start_s))
    }

    /// Buffered audio still in the sink (stereo frames not yet played).
    fn buffered_frames(&self) -> u64 {
        self.sink.unsent_bytes() as u64 / BYTES_PER_STEREO_FRAME
    }

    /// Wait for the sink to finish playing buffered audio (+ a safety pad for
    /// BT pipeline latency), then STOP (#1).
    pub async fn drain_and_stop(mut self) -> Result<(), PlayerError> {
        let drain_secs = self.buffered_frames() as f64 / TARGET_RATE as f64 + DRAIN_SAFETY_PAD;
        if drain_secs > 0.0 {
            tokio::time::sleep(Duration::from_secs_f64(drain_secs)).await;
        }
        self.sink.stop().await?;
        Ok(())
    }
}
