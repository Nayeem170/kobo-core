//! Audio pipeline: MP3 decode → resample → mono→stereo.
//!
//! Edge-TTS `audio-24khz-48kbitrate-mono-mp3` → A1's proven **44100 Hz stereo S16LE**.

const IN_RATE: usize = 24_000; // Edge-TTS PCM
const OUT_RATE: usize = 44_100; // A1-proven target

/// Decode MP3 bytes (Edge-TTS `audio-24khz-48kbitrate-mono-mp3`) → mono S16LE
/// samples via symphonia (pure Rust). The PCM `riff-…` format is rejected by
/// the current endpoint, so MP3+decode is used (PCM remains a documented
/// follow-up optimization).
pub fn decode_mp3(mp3: &[u8]) -> Result<Vec<i16>, String> {
    use symphonia::core::{
        audio::SampleBuffer, codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream,
        meta::MetadataOptions, probe::Hint,
    };
    let mss = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(mp3.to_vec())),
        Default::default(),
    );
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("probe: {e}"))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| "no track".to_string())?
        .clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("decoder: {e}"))?;
    let mut samples: Vec<i16> = Vec::new();
    while let Ok(packet) = format.next_packet() {
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let frames = decoded.frames();
                let mut sbuf = SampleBuffer::<i16>::new(frames as u64, spec);
                sbuf.copy_interleaved_ref(decoded);
                samples.extend_from_slice(sbuf.samples());
            }
            // A bad MP3 frame shouldn't truncate the whole utterance (review #5):
            // skip it; symphonia resyncs on the next packet.
            Err(_) => continue,
        }
    }
    Ok(samples)
}

/// Gap #4: resample mono S16LE from 24 kHz → 44.1 kHz (non-integer) via rubato.
///
/// This is provably lossless across the whole utterance:
/// - A chunk of **leading silence** is prepended so the filter's group-delay
///   transient settles on silence instead of softening/garbling the first
///   spoken word (each utterance starts a fresh resampler instance).
/// - `process_partial(None)` **flushes the trailing delay line** so the last
///   word/syllable isn't clipped.
/// - The documented `output_delay` frames are trimmed from the start so the
///   real audio isn't delayed by the ramp-up.
pub fn resample_mono(pcm: &[i16]) -> Result<Vec<i16>, String> {
    use rubato::{FftFixedIn, Resampler};
    if pcm.is_empty() {
        return Ok(Vec::new());
    }
    const CHUNK: usize = 1024;
    let mut r =
        FftFixedIn::new(IN_RATE, OUT_RATE, CHUNK, 2, 1).map_err(|e| format!("rubato new: {e}"))?;

    // Lead with one chunk of silence so the group-delay transient consumes
    // silence, not the first word.
    let mut frames: Vec<f64> = Vec::with_capacity(pcm.len() + CHUNK);
    frames.resize(CHUNK, 0.0);
    for &s in pcm {
        frames.push(s as f64 / 32768.0);
    }
    while !frames.len().is_multiple_of(CHUNK) {
        frames.push(0.0);
    }
    let ratio = OUT_RATE as f64 / IN_RATE as f64;
    let mut out: Vec<f64> = Vec::with_capacity((frames.len() as f64 * ratio) as usize + 64);
    for chunk in frames.chunks(CHUNK) {
        let input = vec![chunk.to_vec()]; // 1 channel
        let mut o = r
            .process(&input, None)
            .map_err(|e| format!("rubato process: {e}"))?;
        if let Some(ch) = o.get_mut(0) {
            out.append(ch);
        }
    }
    // Flush the held-back tail so the last word/syllable is emitted.
    let mut tail = r
        .process_partial::<Vec<f64>>(None, None)
        .map_err(|e| format!("rubato process_partial: {e}"))?;
    if let Some(ch) = tail.get_mut(0) {
        out.append(ch);
    }

    // Trim the filter's leading delay (the prepended silence covered it, so
    // this never touches real audio).
    let delay = r.output_delay().min(out.len());

    Ok(out[delay..]
        .iter()
        .map(|&f| (f * 32767.0).clamp(-32768.0, 32767.0) as i16)
        .collect())
}

/// Gap #2: mono S16LE → stereo S16LE by duplicating the channel.
pub fn mono_to_stereo(mono: &[i16]) -> Vec<i16> {
    let mut out = Vec::with_capacity(mono.len() * 2);
    for &s in mono {
        out.push(s);
        out.push(s);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_to_stereo_doubles_length_and_interleaves() {
        let out = mono_to_stereo(&[10, -20, 30]);
        assert_eq!(out, vec![10, 10, -20, -20, 30, 30]);
        assert_eq!(out.len(), 6, "length doubles");
    }

    #[test]
    fn mono_to_stereo_empty_is_empty() {
        assert!(mono_to_stereo(&[]).is_empty());
    }

    #[test]
    fn resample_mono_empty_returns_empty() {
        assert!(resample_mono(&[]).unwrap().is_empty());
    }

    #[test]
    fn resample_mono_grows_by_out_in_ratio() {
        // 2 s of mono @ 24 kHz. The output must grow ~OUT_RATE/IN_RATE (1.8375×)
        // to 44.1 kHz; the lead-silence + delay-trim are small relative to 2 s,
        // so the result lands in a tight band around 88 200 samples.
        let input: Vec<i16> = (0..48_000).map(|i| (i as i32 & 0xff) as i16).collect();
        let out = resample_mono(&input).expect("resample ok");
        assert!(!out.is_empty());
        let expected = 48_000.0 * OUT_RATE as f32 / IN_RATE as f32;
        let lo = (expected * 0.93) as usize;
        let hi = (expected * 1.07) as usize;
        assert!(
            (lo..=hi).contains(&out.len()),
            "resampled length {} not within [{}, {}]",
            out.len(),
            lo,
            hi
        );
    }
}
