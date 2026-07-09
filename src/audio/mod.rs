pub mod a2dp;
pub mod pipeline;
pub mod player;
pub mod synth;

pub use a2dp::{A2dpError, AospA2dpSink, TARGET_RATE};
pub use player::{
    Player, PlayerError, Prepared, WordMark, CHUNK_FRAMES, LEAD_FRAMES, LEAD_IN_FRAMES,
    PARA_GAP_FRAMES, SENTENCE_GAP_FRAMES,
};
pub use synth::synthesize_prepared;

pub use kothok_edge_tts::{self as edge_tts, init_tls, EdgeTts, Engine, TtsError, TtsEvent};
