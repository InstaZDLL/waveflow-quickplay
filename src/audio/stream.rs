//! One open track: a symphonia demuxer + decoder, or the DSD → PCM pipeline
//! from `waveflow-core`, followed by channel conversion and resampling to
//! the device's layout. Mirrors `ActiveStream` in the desktop engine's
//! `crossfade.rs` minus crossfade, DoP, ReplayGain, speed and network
//! pre-load.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler as _, WindowFunction};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;
use waveflow_core::audio_format::dsd::parser::{parse_dff, parse_dsf, DsdLayout};
use waveflow_core::audio_format::dsd::pcm::DsdToPcm;

/// How many DSD bytes to pull from disk per decode cycle (64 KiB ≈ 23 ms of
/// DSD64 stereo) — the desktop engine's figure.
const DSD_READ_CHUNK: usize = 64 * 1024;

/// FIR length for the DSD → PCM converter: the desktop default.
const DSD_TAPS: usize = 64;

enum Backend {
    Symphonia {
        format: Box<dyn FormatReader>,
        decoder: Box<dyn AudioDecoder>,
        decoded: Vec<f32>,
        track_id: u32,
        spec_captured: bool,
    },
    Dsd {
        file: File,
        layout: DsdLayout,
        converter: Box<DsdToPcm>,
        bytes_read: u64,
        dsd_scratch: Vec<u8>,
        pcm_scratch: Vec<f32>,
    },
}

pub struct ActiveStream {
    backend: Backend,
    resampler: Resampler,
    src_channels: usize,
    src_sample_rate: u32,
    duration_ms: Option<u64>,
    codec: Option<String>,
}

impl ActiveStream {
    /// Open `path` by content (symphonia's probe) through WaveFlow's one
    /// codec registry, or through the DSD pipeline for `.dsf` / `.dff`.
    pub fn open(path: &Path) -> Result<Self, String> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase());
        if matches!(ext.as_deref(), Some("dsf") | Some("dff")) {
            return Self::open_dsd(path, ext.as_deref().unwrap_or(""));
        }

        let file = File::open(path).map_err(|e| format!("open: {e}"))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = ext.as_deref() {
            hint.with_extension(ext);
        }
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|e| format!("probe: {e}"))?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| "no decodable audio track".to_string())?;
        let track_id = track.id;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| "track has no audio codec parameters".to_string())?;

        // Container duration, the way the scanner's probe computes it.
        let time_base = track.time_base;
        let duration_ms = track
            .duration
            .and_then(|d| time_base.and_then(|tb| tb.calc_duration(d)))
            .map(|t| (t.as_micros() / 1000) as u64)
            .or_else(|| {
                track
                    .num_frames
                    .zip(params.sample_rate.filter(|r| *r > 0))
                    .map(|(n, r)| n.saturating_mul(1000) / u64::from(r))
            });

        // WaveFlow's registry, not symphonia's: it is the one carrying Opus.
        let registry = waveflow_core::audio_format::opus::codecs();
        let decoder = registry
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(|e| format!("codec init: {e}"))?;
        let codec = registry
            .get_audio_decoder(params.codec)
            .map(|registered| codec_label(registered.codec.info.short_name));

        Ok(Self {
            backend: Backend::Symphonia {
                format,
                decoder,
                decoded: Vec::new(),
                track_id,
                spec_captured: false,
            },
            resampler: Resampler::Passthrough,
            src_channels: 0,
            src_sample_rate: 0,
            duration_ms,
            codec,
        })
    }

    fn open_dsd(path: &Path, ext: &str) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|e| format!("open: {e}"))?;
        let layout = match ext {
            "dsf" => parse_dsf(&mut file).map_err(|e| format!("dsf parse: {e}"))?,
            "dff" => parse_dff(&mut file).map_err(|e| format!("dff parse: {e}"))?,
            _ => return Err(format!("unexpected DSD extension: {ext}")),
        };
        file.seek(SeekFrom::Start(layout.data_offset))
            .map_err(|e| format!("dsd seek to data: {e}"))?;
        let src_channels = layout.channels.count() as usize;
        let converter = Box::new(DsdToPcm::new_with_taps(&layout, DSD_TAPS));
        let duration_ms = Some(layout.duration_ms());
        let codec = layout
            .dsd_rate_multiple()
            .map(|m| format!("DSD{m}"))
            .or_else(|| Some("DSD".to_string()));
        Ok(Self {
            backend: Backend::Dsd {
                file,
                layout,
                converter,
                bytes_read: 0,
                dsd_scratch: Vec::with_capacity(DSD_READ_CHUNK),
                pcm_scratch: Vec::with_capacity(DSD_READ_CHUNK),
            },
            resampler: Resampler::Passthrough,
            src_channels,
            src_sample_rate: 0,
            duration_ms,
            codec,
        })
    }

    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }

    pub fn codec(&self) -> Option<&str> {
        self.codec.as_deref()
    }

    /// The source rate once the first packet has been decoded, `0` before.
    pub fn source_sample_rate(&self) -> u32 {
        self.src_sample_rate
    }

    /// Flush decoder / converter state after a seek.
    pub fn reset_decoder(&mut self) {
        match &mut self.backend {
            Backend::Symphonia { decoder, .. } => decoder.reset(),
            Backend::Dsd {
                layout, converter, ..
            } => {
                **converter = DsdToPcm::new_with_taps(layout, DSD_TAPS);
            }
        }
        self.resampler.flush();
    }

    /// Seek to `ms`; best-effort past EOF.
    pub fn seek_ms(&mut self, ms: u64) {
        match &mut self.backend {
            Backend::Symphonia {
                format, track_id, ..
            } => {
                let time = Time::try_from_secs_f64(ms as f64 / 1000.0).unwrap_or_default();
                if let Err(err) = format.seek(
                    SeekMode::Accurate,
                    SeekTo::Time {
                        time,
                        track_id: Some(*track_id),
                    },
                ) {
                    tracing::warn!(?err, ms, "format seek failed");
                }
            }
            Backend::Dsd {
                file,
                layout,
                bytes_read,
                ..
            } => {
                let target = dsd_byte_offset_for_ms(layout, ms);
                let stride = interleave_stride(layout);
                let aligned = aligned_byte_offset(target, stride, layout.data_len_bytes);
                match file.seek(SeekFrom::Start(layout.data_offset + aligned)) {
                    Ok(_) => *bytes_read = aligned,
                    Err(err) => tracing::warn!(?err, ms, "dsd seek failed"),
                }
            }
        }
    }

    /// Decode one packet / one DSD block and append the resampled,
    /// channel-converted interleaved f32 samples into `out`. `Ok(true)` on
    /// end of stream, `Ok(false)` after a successful step (which may have
    /// produced no samples yet while the resampler or the FIR primes).
    pub fn decode_next(
        &mut self,
        out: &mut Vec<f32>,
        scratch: &mut Vec<f32>,
        dst_rate: u32,
        dst_channels: usize,
    ) -> Result<bool, String> {
        match &mut self.backend {
            Backend::Symphonia {
                format,
                decoder,
                decoded,
                track_id,
                spec_captured,
            } => loop {
                let packet = match format.next_packet() {
                    Ok(Some(p)) => p,
                    Ok(None) | Err(SymphoniaError::ResetRequired) => return Ok(true),
                    Err(SymphoniaError::IoError(e))
                        if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                    {
                        return Ok(true)
                    }
                    Err(e) => return Err(format!("next_packet: {e}")),
                };
                if packet.track_id != *track_id {
                    continue;
                }
                let buffer = match decoder.decode(&packet) {
                    Ok(b) => b,
                    Err(SymphoniaError::DecodeError(e)) => {
                        tracing::warn!(error = %e, "decode error, skipping packet");
                        continue;
                    }
                    Err(e) => return Err(format!("decode: {e}")),
                };
                if !*spec_captured {
                    let spec = buffer.spec();
                    self.src_channels = spec.channels().count();
                    self.src_sample_rate = spec.rate();
                    self.resampler = Resampler::new(spec.rate(), dst_rate, dst_channels)?;
                    *spec_captured = true;
                }
                buffer.copy_to_vec_interleaved(decoded);
                scratch.clear();
                convert_channels(decoded, self.src_channels, dst_channels, scratch);
                self.resampler.process(scratch, out)?;
                return Ok(false);
            },
            Backend::Dsd {
                file,
                layout,
                converter,
                bytes_read,
                dsd_scratch,
                pcm_scratch,
            } => {
                if *bytes_read >= layout.data_len_bytes {
                    return Ok(true);
                }
                // Whole interleave strides only, so the converter never
                // attributes a byte to the wrong channel.
                let stride = interleave_stride(layout);
                let remaining = layout.data_len_bytes - *bytes_read;
                let raw_want = remaining.min(DSD_READ_CHUNK as u64);
                let aligned = raw_want / stride.max(1) * stride.max(1);
                if aligned == 0 {
                    *bytes_read = layout.data_len_bytes;
                    return Ok(true);
                }
                dsd_scratch.resize(aligned as usize, 0);
                let read =
                    read_full_block(file, dsd_scratch).map_err(|e| format!("dsd read: {e}"))?;
                let read = (read as u64 / stride.max(1) * stride.max(1)) as usize;
                if read == 0 {
                    *bytes_read = layout.data_len_bytes;
                    return Ok(true);
                }
                *bytes_read += read as u64;
                dsd_scratch.truncate(read);

                pcm_scratch.clear();
                converter.decode_block(dsd_scratch, pcm_scratch);
                if pcm_scratch.is_empty() {
                    // FIR still priming: progress without samples.
                    return Ok(false);
                }
                if self.src_sample_rate == 0 {
                    self.src_sample_rate = converter.output_rate_hz;
                    self.resampler =
                        Resampler::new(converter.output_rate_hz, dst_rate, dst_channels)?;
                }
                scratch.clear();
                convert_channels(pcm_scratch, self.src_channels, dst_channels, scratch);
                self.resampler.process(scratch, out)?;
                Ok(false)
            }
        }
    }
}

/// A symphonia short name as WaveFlow's format chip spells it.
fn codec_label(short_name: &str) -> String {
    match short_name {
        n if n.starts_with("pcm") => "PCM".into(),
        "vorbis" => "Vorbis".into(),
        "opus" => "Opus".into(),
        n => n.to_ascii_uppercase(),
    }
}

fn read_full_block(file: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

fn interleave_stride(layout: &DsdLayout) -> u64 {
    match layout.block_interleave {
        Some(block) => (block as u64) * (layout.channels.count() as u64),
        None => layout.channels.count() as u64,
    }
}

fn aligned_byte_offset(target: u64, stride: u64, data_len_bytes: u64) -> u64 {
    (target / stride.max(1) * stride.max(1)).min(data_len_bytes)
}

fn dsd_byte_offset_for_ms(layout: &DsdLayout, ms: u64) -> u64 {
    let bits_per_second = (layout.sample_rate_hz as u128) * (layout.channels.count() as u128);
    ((ms as u128) * bits_per_second / 8 / 1000).min(u64::MAX as u128) as u64
}

/// Convert interleaved `src` channels to interleaved `dst` channels — the
/// desktop decoder's rules: copy, duplicate mono, average to mono, ITU-R
/// BS.775 fold-down to stereo for ≥ 3 sources, truncate / zero-pad otherwise.
pub fn convert_channels(input: &[f32], src: usize, dst: usize, out: &mut Vec<f32>) {
    if src == 0 || dst == 0 {
        return;
    }
    if src == dst {
        out.extend_from_slice(input);
        return;
    }
    let frames = input.len() / src;
    match (src, dst) {
        (1, 2) => {
            out.reserve(frames * 2);
            for &s in input.iter().take(frames) {
                out.push(s);
                out.push(s);
            }
        }
        (2, 1) => {
            out.reserve(frames);
            for f in 0..frames {
                out.push(0.5 * (input[f * 2] + input[f * 2 + 1]));
            }
        }
        (s, 2) if s >= 3 => {
            out.reserve(frames * 2);
            for f in 0..frames {
                let (lo, ro) = downmix_frame_to_stereo(&input[f * s..f * s + s]);
                out.push(lo);
                out.push(ro);
            }
        }
        _ => {
            out.reserve(frames * dst);
            for f in 0..frames {
                for ch in 0..dst {
                    out.push(if ch < src { input[f * src + ch] } else { 0.0 });
                }
            }
        }
    }
}

#[inline]
fn downmix_frame_to_stereo(frame: &[f32]) -> (f32, f32) {
    const K: f32 = 0.707;
    let l = frame[0];
    let r = frame[1];
    match frame.len() {
        3 => (l + K * frame[2], r + K * frame[2]),
        4 => (l + K * frame[2], r + K * frame[3]),
        5 => (
            l + K * frame[2] + K * frame[3],
            r + K * frame[2] + K * frame[4],
        ),
        6 => (
            l + K * frame[2] + K * frame[4],
            r + K * frame[2] + K * frame[5],
        ),
        7 => {
            let bc = K * frame[6];
            (
                l + K * frame[2] + K * frame[4] + bc,
                r + K * frame[2] + K * frame[5] + bc,
            )
        }
        _ => {
            let mut lo = l + K * frame[2];
            let mut ro = r + K * frame[2];
            for (i, &s) in frame.iter().enumerate().skip(4) {
                if i % 2 == 0 {
                    lo += K * s;
                } else {
                    ro += K * s;
                }
            }
            (lo, ro)
        }
    }
}

/// rubato FFT resampler with `FixedSync::Input` semantics, interleaved in
/// and out, the desktop engine's parameters (1024-frame chunks, 2
/// sub-chunks, Blackman-Harris²). `Passthrough` when the rates match.
#[allow(clippy::large_enum_variant)]
pub enum Resampler {
    Passthrough,
    Fft {
        inner: Fft<f32>,
        channels: usize,
        in_scratch: Vec<f32>,
        out_scratch: Vec<f32>,
        pending: Vec<f32>,
    },
}

const CHUNK_SIZE: usize = 1024;
const SUB_CHUNKS: usize = 2;

impl Resampler {
    pub fn new(src_rate: u32, dst_rate: u32, channels: usize) -> Result<Self, String> {
        if src_rate == dst_rate || src_rate == 0 || dst_rate == 0 || channels == 0 {
            return Ok(Self::Passthrough);
        }
        let inner = Fft::<f32>::new_custom(
            src_rate as usize,
            dst_rate as usize,
            CHUNK_SIZE,
            SUB_CHUNKS,
            channels,
            WindowFunction::BlackmanHarris2,
            FixedSync::Input,
        )
        .map_err(|e| format!("rubato init ({src_rate}→{dst_rate}): {e}"))?;
        let frames_in = inner.input_frames_next();
        let frames_out_max = inner.output_frames_max();
        Ok(Self::Fft {
            inner,
            channels,
            in_scratch: vec![0.0; frames_in * channels],
            out_scratch: vec![0.0; frames_out_max * channels],
            pending: Vec::with_capacity(frames_in * channels * 2),
        })
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<(), String> {
        match self {
            Self::Passthrough => {
                out.extend_from_slice(input);
                Ok(())
            }
            Self::Fft {
                inner,
                channels,
                in_scratch,
                out_scratch,
                pending,
            } => {
                let chans = *channels;
                pending.extend_from_slice(input);
                loop {
                    let frames_in = inner.input_frames_next();
                    let in_samples = frames_in * chans;
                    if pending.len() < in_samples {
                        break;
                    }
                    if in_scratch.len() < in_samples {
                        in_scratch.resize(in_samples, 0.0);
                    }
                    in_scratch[..in_samples].copy_from_slice(&pending[..in_samples]);
                    pending.drain(..in_samples);

                    let frames_out_max = inner.output_frames_max();
                    if out_scratch.len() < frames_out_max * chans {
                        out_scratch.resize(frames_out_max * chans, 0.0);
                    }
                    let n_out = {
                        let in_buf =
                            InterleavedSlice::new(&in_scratch[..in_samples], chans, frames_in)
                                .map_err(|e| format!("rubato in adapter: {e}"))?;
                        let mut out_buf = InterleavedSlice::new_mut(
                            &mut out_scratch[..frames_out_max * chans],
                            chans,
                            frames_out_max,
                        )
                        .map_err(|e| format!("rubato out adapter: {e}"))?;
                        let (_n_in, n_out) = inner
                            .process_into_buffer(&in_buf, &mut out_buf, None)
                            .map_err(|e| format!("rubato process: {e}"))?;
                        n_out
                    };
                    out.extend_from_slice(&out_scratch[..n_out * chans]);
                }
                Ok(())
            }
        }
    }

    /// Drop frames still queued (after a seek). Costs at most one chunk.
    pub fn flush(&mut self) {
        if let Self::Fft { pending, .. } = self {
            pending.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_to_stereo_duplicates_and_stereo_to_mono_averages() {
        let mut out = Vec::new();
        convert_channels(&[0.5, -0.5], 1, 2, &mut out);
        assert_eq!(out, vec![0.5, 0.5, -0.5, -0.5]);
        out.clear();
        convert_channels(&[1.0, 0.0, 0.5, 0.5], 2, 1, &mut out);
        assert_eq!(out, vec![0.5, 0.5]);
    }

    #[test]
    fn five_one_folds_to_stereo_without_lfe() {
        let mut out = Vec::new();
        // FL FR FC LFE BL BR
        convert_channels(&[1.0, 0.0, 0.0, 9.0, 0.0, 0.0], 6, 2, &mut out);
        assert_eq!(out, vec![1.0, 0.0]);
    }

    #[test]
    fn resampler_passthrough_when_rates_match() {
        let mut r = Resampler::new(48_000, 48_000, 2).expect("ctor");
        let mut out = Vec::new();
        r.process(&[0.1, 0.2], &mut out).expect("process");
        assert_eq!(out, vec![0.1, 0.2]);
    }

    #[test]
    fn resampler_48k_to_44_1k_is_proportional() {
        let mut r = Resampler::new(48_000, 44_100, 2).expect("ctor");
        let mut out = Vec::new();
        let chunk = vec![0.0_f32; 4096 * 2];
        for _ in 0..12 {
            r.process(&chunk, &mut out).expect("process");
        }
        let produced = out.len() / 2;
        let expected = 12_u64 * 4096 * 44_100 / 48_000;
        assert!(
            (produced as i64 - expected as i64).abs() < 2_048,
            "{produced} vs {expected}"
        );
    }
}
