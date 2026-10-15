//! Sound files of every kind a game ships: [`decode_any`] reads a WAV, an Ogg Vorbis (`.ogg`) or an MP3 held in memory into a [`Clip`], telling them apart by
//! their first bytes. Vorbis is read by `lewton` and MP3 by `puremp3` (both pure Rust); a clip with more than two channels keeps its first two.
use crate::wav::{self, Clip, MAX_SAMPLES};
use std::io::Cursor;

/// Does this look like an MP3 (an ID3 tag, or the 11-bit frame sync that every MPEG audio frame starts with)?
fn looks_like_mp3(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ID3") || (bytes.len() > 2 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0)
}

pub fn decode_any(bytes: &[u8]) -> Result<Clip, String> {
    if bytes.starts_with(b"OggS") {
        decode_ogg(bytes)
    } else if bytes.starts_with(b"RIFF") {
        wav::decode(bytes)
    } else if looks_like_mp3(bytes) {
        decode_mp3(bytes)
    } else {
        // the WAV reader's own message says what is expected
        wav::decode(bytes).map_err(|e| format!("{e} (WAV, Ogg Vorbis and MP3 files are read)"))
    }
}

/// An Ogg Vorbis file.
pub fn decode_ogg(bytes: &[u8]) -> Result<Clip, String> {
    let mut reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes)).map_err(|e| format!("Ogg Vorbis: {e}"))?;
    let (channels, rate) = (reader.ident_hdr.audio_channels as usize, reader.ident_hdr.audio_sample_rate);
    if channels == 0 || rate == 0 {
        return Err("Ogg Vorbis: no audio in the file".to_string());
    }
    let keep = channels.min(2);
    let mut data: Vec<f32> = Vec::new();
    while let Some(packet) = reader.read_dec_packet_itl().map_err(|e| format!("Ogg Vorbis: {e}"))? {
        for frame in packet.chunks(channels) {
            data.extend(frame.iter().take(keep).map(|s| *s as f32 / 32768.0));
        }
        if data.len() > MAX_SAMPLES {
            return Err(format!("the sound is longer than the {} million samples a clip may hold", MAX_SAMPLES / 1_000_000));
        }
    }
    Ok(Clip { rate, channels: keep as u16, data })
}

/// An MP3 file (MPEG Layer III).
pub fn decode_mp3(bytes: &[u8]) -> Result<Clip, String> {
    let (header, frames) = puremp3::read_mp3(Cursor::new(bytes)).map_err(|e| format!("MP3: {e}"))?;
    let rate = header.sample_rate.hz();
    let mut data: Vec<f32> = Vec::new();
    let stereo = !matches!(header.channels, puremp3::Channels::Mono);
    for (l, r) in frames {
        data.push(l);
        if stereo {
            data.push(r);
        }
        if data.len() > MAX_SAMPLES {
            return Err(format!("the sound is longer than the {} million samples a clip may hold", MAX_SAMPLES / 1_000_000));
        }
    }
    if data.is_empty() {
        return Err("MP3: no audio in the file".to_string());
    }
    Ok(Clip { rate, channels: if stereo { 2 } else { 1 }, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(name: &str) -> Vec<u8> {
        std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata").join(name)).unwrap()
    }

    /// A 440 Hz sine of half a second, written by ffmpeg in each format: after decoding it must still be a sine at that pitch with the right length.
    fn is_the_tone(c: &Clip) {
        assert!((c.seconds() - 0.5).abs() < 0.12, "{} s", c.seconds());
        let ch = c.channels as usize;
        let left: Vec<f32> = c.data.chunks(ch).map(|f| f[0]).collect();
        let peak = left.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.05 && peak <= 1.01, "peak {peak}");
        // count the upward zero crossings in the middle of the clip: about 440 a second
        let (a, b) = (left.len() / 5, left.len() * 4 / 5);
        let crossings = left[a..b].windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f32;
        let hz = crossings / ((b - a) as f32 / c.rate as f32);
        assert!((hz - 440.0).abs() < 25.0, "{hz} Hz");
    }

    #[test]
    fn ogg_vorbis_and_mp3_files_decode_to_the_tone_they_hold() {
        let ogg = decode_any(&data("tone.ogg")).unwrap();
        is_the_tone(&ogg);
        let mp3 = decode_any(&data("tone.mp3")).unwrap();
        is_the_tone(&mp3);
        assert!(matches!(mp3.channels, 1 | 2) && matches!(ogg.channels, 1 | 2));
        // a WAV still reads, and anything else is a clear error
        let wav_bytes = wav::encode(22050, 1, &[0.0, 0.25, -0.25, 0.0]);
        assert_eq!(decode_any(&wav_bytes).unwrap().rate, 22050);
        let e = decode_any(b"hello, this is not sound").unwrap_err();
        assert!(e.contains("Ogg Vorbis and MP3"), "{e}");
        assert!(decode_any(b"OggS\0\0\0\0garbage").is_err());
    }
}
