//! WAV files: [`decode`] into a [`Clip`] (interleaved `f32` samples in -1..1) and [`encode`] 16-bit PCM.

/// The most samples (frames times channels) a clip may hold: about 19 minutes of mono 44.1 kHz sound, 200 MB as `f32`.
pub const MAX_SAMPLES: usize = 50_000_000;

/// Decoded sound: `data` holds `channels` (1 or 2) interleaved samples per frame, at `rate` frames per second.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub rate: u32,
    pub channels: u16,
    pub data: Vec<f32>,
}

impl Clip {
    pub fn mono(rate: u32, samples: Vec<f32>) -> Clip {
        Clip { rate, channels: 1, data: samples }
    }

    /// Number of frames (one sample per channel).
    pub fn frames(&self) -> usize {
        self.data.len() / self.channels.max(1) as usize
    }

    pub fn seconds(&self) -> f32 {
        self.frames() as f32 / self.rate.max(1) as f32
    }
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Read a WAV file held in memory. The error says what is wrong in words a person (or an agent) can act on.
pub fn decode(bytes: &[u8]) -> Result<Clip, String> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file (it has to start with RIFF....WAVE); convert it, for example with `ffmpeg -i in.mp3 out.wav`".to_string());
    }
    // (format tag, channels, rate, bits)
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    let mut data: Option<&[u8]> = None;
    let mut at = 12usize;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32_at(bytes, at + 4) as usize;
        let body_start = at + 8;
        // a damaged size is cut to what the file really holds
        let body_end = body_start.saturating_add(size).min(bytes.len());
        let body = &bytes[body_start..body_end];
        if id == b"fmt " {
            if body.len() < 16 {
                return Err("the `fmt ` chunk is too short".to_string());
            }
            let mut tag = u16_at(body, 0);
            if tag == 0xFFFE && body.len() >= 26 {
                tag = u16_at(body, 24); // WAVE_FORMAT_EXTENSIBLE: the real tag opens the sub-format GUID
            }
            fmt = Some((tag, u16_at(body, 2), u32_at(body, 4), u16_at(body, 14)));
        } else if id == b"data" {
            data = Some(body);
            if fmt.is_some() {
                break;
            }
        }
        at = body_start.saturating_add(size).saturating_add(size & 1); // chunks are padded to an even size
    }
    let (tag, channels, rate, bits) = fmt.ok_or("no `fmt ` chunk: the file is cut short or damaged")?;
    let data = data.ok_or("no `data` chunk: the file holds no sound")?;
    if channels == 0 || channels > 2 {
        return Err(format!("{channels} channels: only mono and stereo are supported; mix it down first"));
    }
    if !(1_000..=384_000).contains(&rate) {
        return Err(format!("sample rate {rate} Hz is outside 1000 to 384000"));
    }
    let width = (bits / 8) as usize;
    let supported = match (tag, bits) {
        (1, 8) | (1, 16) | (1, 24) | (1, 32) | (3, 32) | (3, 64) => true,
        _ => false,
    };
    if !supported {
        return Err(format!("format {tag} with {bits} bits per sample is not supported: use 8, 16, 24 or 32 bit PCM, or 32 bit float"));
    }
    let count = data.len() / width;
    let count = count - count % channels as usize;
    if count > MAX_SAMPLES {
        return Err(format!("the sound has {count} samples; the limit is {MAX_SAMPLES} (about 19 minutes of mono at 44.1 kHz)"));
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let p = &data[i * width..(i + 1) * width];
        out.push(match (tag, bits) {
            (1, 8) => (p[0] as f32 - 128.0) / 128.0,
            (1, 16) => i16::from_le_bytes([p[0], p[1]]) as f32 / 32768.0,
            (1, 24) => (((p[0] as i32) << 8 | (p[1] as i32) << 16 | (p[2] as i32) << 24) >> 8) as f32 / 8_388_608.0,
            (1, _) => i32::from_le_bytes([p[0], p[1], p[2], p[3]]) as f32 / 2_147_483_648.0,
            (_, 32) => f32::from_le_bytes([p[0], p[1], p[2], p[3]]),
            _ => f64::from_le_bytes([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]]) as f32,
        });
    }
    // a damaged float file may hold NaN or infinity: silence is the safe reading
    for s in out.iter_mut() {
        if !s.is_finite() {
            *s = 0.0;
        }
    }
    Ok(Clip { rate, channels, data: out })
}

/// 16-bit PCM WAV bytes of interleaved `samples` (values outside -1..1 are clipped).
pub fn encode(rate: u32, channels: u16, samples: &[f32]) -> Vec<u8> {
    let mut pcm: Vec<u8> = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        let v = if s.is_nan() { 0.0 } else { s.clamp(-1.0, 1.0) };
        pcm.extend_from_slice(&((v * 32767.0).round() as i16).to_le_bytes());
    }
    let block = channels * 2;
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * block as u32).to_le_bytes());
    out.extend_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(&pcm);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav_with(tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&tag.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * (bits as u32 / 8) * channels as u32).to_le_bytes());
        b.extend_from_slice(&(channels * bits / 8).to_le_bytes());
        b.extend_from_slice(&bits.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(data);
        b
    }

    #[test]
    fn what_encode_writes_decode_reads_back_within_one_step() {
        let samples: Vec<f32> = (0..200).map(|i| ((i as f32) * 0.07).sin() * 0.8).collect();
        let c = decode(&encode(22_050, 1, &samples)).unwrap();
        assert_eq!((c.rate, c.channels, c.frames()), (22_050, 1, 200));
        for (a, b) in samples.iter().zip(&c.data) {
            assert!((a - b).abs() < 1.0 / 16_000.0, "{a} {b}");
        }
        let stereo = decode(&encode(44_100, 2, &[0.5, -0.5, 0.25, -0.25])).unwrap();
        assert_eq!((stereo.channels, stereo.frames()), (2, 2));
        assert!((stereo.data[1] + 0.5).abs() < 1e-3, "the right channel keeps its sign");
        // out-of-range and NaN samples are made safe, not written as noise
        let wild = decode(&encode(8_000, 1, &[3.0, -3.0, f32::NAN])).unwrap();
        assert_eq!(wild.data.len(), 3);
        assert!(wild.data[0] > 0.99 && wild.data[1] < -0.99 && wild.data[2] == 0.0);
    }

    #[test]
    fn every_supported_sample_format_is_read() {
        let u8c = decode(&wav_with(1, 1, 8_000, 8, &[128, 255, 0])).unwrap();
        assert!(u8c.data[0].abs() < 1e-6 && u8c.data[1] > 0.99 && u8c.data[2] <= -0.99);
        let i24 = decode(&wav_with(1, 1, 8_000, 24, &[0, 0, 0x40, 0, 0, 0xC0])).unwrap();
        assert!((i24.data[0] - 0.5).abs() < 1e-6 && (i24.data[1] + 0.5).abs() < 1e-6, "{:?}", i24.data);
        let i32c = decode(&wav_with(1, 1, 8_000, 32, &i32::MIN.to_le_bytes())).unwrap();
        assert_eq!(i32c.data, vec![-1.0]);
        let f32c = decode(&wav_with(3, 1, 8_000, 32, &0.25f32.to_le_bytes())).unwrap();
        assert_eq!(f32c.data, vec![0.25]);
        let f64c = decode(&wav_with(3, 1, 8_000, 64, &(-0.125f64).to_le_bytes())).unwrap();
        assert_eq!(f64c.data, vec![-0.125]);
        let nan = decode(&wav_with(3, 1, 8_000, 32, &f32::NAN.to_le_bytes())).unwrap();
        assert_eq!(nan.data, vec![0.0], "NaN reads as silence");
    }

    #[test]
    fn damaged_files_are_refused_with_a_reason_and_never_panic() {
        assert!(decode(b"").unwrap_err().contains("not a WAV"));
        assert!(decode(b"RIFF\0\0\0\0WAVE").unwrap_err().contains("fmt"));
        assert!(decode(&wav_with(2, 1, 8_000, 4, &[0, 0])).unwrap_err().contains("not supported"), "ADPCM");
        assert!(decode(&wav_with(1, 6, 8_000, 16, &[0; 12])).unwrap_err().contains("channels"));
        assert!(decode(&wav_with(1, 1, 5, 16, &[0; 4])).unwrap_err().contains("sample rate"));
        // a size field larger than the file is cut to the file; a half sample at the end is dropped
        let mut cut = wav_with(1, 1, 8_000, 16, &[1, 0, 2, 0, 3]);
        let n = cut.len();
        cut[n - 5 - 4..n - 5].copy_from_slice(&1_000_000u32.to_le_bytes());
        assert_eq!(decode(&cut).unwrap().frames(), 2);
        // a fuzz-like sweep: truncate a good file at every length
        let good = wav_with(1, 2, 8_000, 16, &[7; 64]);
        for len in 0..good.len() {
            let _ = decode(&good[..len]);
        }
        // odd chunks before the data are skipped (and padded to an even size)
        let mut with_list = Vec::new();
        with_list.extend_from_slice(&good[..36]);
        with_list.extend_from_slice(b"LIST\x03\0\0\0abc\0");
        with_list.extend_from_slice(&good[36..]);
        assert_eq!(decode(&with_list).unwrap().frames(), 16);
    }
}
