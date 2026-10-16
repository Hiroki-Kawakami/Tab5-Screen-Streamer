use super::io::Source;
use super::{h264, mkv, mp4};
use crate::bail;
use crate::error::Result;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Codec {
    H264,
    Hevc,
    Vp8,
    Vp9,
    Av1,
    Mpeg4Part2,
    Mpeg1Video,
    Mpeg2Video,
    Mjpeg,
    Raw,
    Aac,
    Mp3,
    Opus,
    Vorbis,
    Flac,
    Ac3,
    Eac3,
    Pcm,
    Unknown,
}

impl Codec {
    pub fn name(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::Hevc => "hevc",
            Self::Vp8 => "vp8",
            Self::Vp9 => "vp9",
            Self::Av1 => "av1",
            Self::Mpeg4Part2 => "mpeg4",
            Self::Mpeg1Video => "mpeg1video",
            Self::Mpeg2Video => "mpeg2video",
            Self::Mjpeg => "mjpeg",
            Self::Raw => "rawvideo",
            Self::Aac => "aac",
            Self::Mp3 => "mp3",
            Self::Opus => "opus",
            Self::Vorbis => "vorbis",
            Self::Flac => "flac",
            Self::Ac3 => "ac3",
            Self::Eac3 => "eac3",
            Self::Pcm => "pcm",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_mpeg4_object_type(oti: u8) -> Self {
        match oti {
            0x20 => Self::Mpeg4Part2,
            0x21 => Self::H264,
            0x40 | 0x66..=0x68 => Self::Aac,
            0x60..=0x65 => Self::Mpeg2Video,
            0x69 | 0x6B => Self::Mp3,
            0x6A => Self::Mpeg1Video,
            0x6C => Self::Mjpeg,
            0xA5 => Self::Ac3,
            0xA6 => Self::Eac3,
            0xAD => Self::Opus,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorInfo {
    pub primaries: u8,
    pub transfer: u8,
    pub matrix: u8,
    pub full_range: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VideoTrack {
    pub width: u32,
    pub height: u32,
    pub sample_aspect: Option<(u32, u32)>,
    pub rotation: u32,
    pub color: Option<ColorInfo>,
    /// ColourSpace of uncompressed video, such as `I420`.
    pub fourcc: Option<[u8; 4]>,
}

impl VideoTrack {
    pub fn display_size(&self) -> (f64, f64) {
        let sar = self
            .sample_aspect
            .filter(|&(h, v)| h > 0 && v > 0)
            .map_or(1.0, |(h, v)| h as f64 / v as f64);
        let (w, h) = (self.width as f64 * sar, self.height as f64);
        if self.rotation % 180 == 90 {
            (h, w)
        } else {
            (w, h)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioTrack {
    pub channels: u32,
    pub sample_rate: u32,
    pub bits: Option<u32>,
    pub bit_rate: Option<u64>,
    pub priming: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TrackKind {
    Video(VideoTrack),
    Audio(AudioTrack),
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub index: u32,
    pub codec: Codec,
    pub tag: String,
    pub extradata: Vec<u8>,
    pub kind: TrackKind,
    pub frames: Option<u64>,
}

fn hevc_string(hvcc: &[u8]) -> Option<String> {
    let profile = *hvcc.get(1)?;
    let space = ["", "A", "B", "C"][(profile >> 6) as usize];
    let tier = if profile & 0x20 != 0 { 'H' } else { 'L' };
    let compat = u32::from_be_bytes(hvcc.get(2..6)?.try_into().ok()?).reverse_bits();
    let constraints = hvcc.get(6..12)?;
    let used = constraints
        .iter()
        .rposition(|&b| b != 0)
        .map_or(0, |i| i + 1);
    let mut text = format!(
        "hvc1.{space}{}.{compat:X}.{tier}{}",
        profile & 0x1F,
        hvcc.get(12)?
    );
    for b in &constraints[..used] {
        text.push_str(&format!(".{b:X}"));
    }
    Some(text)
}

fn av1_string(av1c: &[u8]) -> Option<String> {
    let (a, b) = (*av1c.get(1)?, *av1c.get(2)?);
    let depth = match (b & 0x40 != 0, b & 0x20 != 0) {
        (true, true) => 12,
        (true, false) => 10,
        _ => 8,
    };
    Some(format!(
        "av01.{}.{:02}{}.{depth:02}",
        a >> 5,
        a & 0x1F,
        if b & 0x80 != 0 { 'H' } else { 'M' }
    ))
}

impl Track {
    pub fn audio(&self) -> Option<&AudioTrack> {
        match &self.kind {
            TrackKind::Audio(audio) => Some(audio),
            _ => None,
        }
    }

    /// The WebCodecs codec string.
    pub fn codec_string(&self) -> Option<String> {
        let e = &self.extradata;
        Some(match self.codec {
            Codec::H264 => format!("avc1.{:02X}{:02X}{:02X}", e.get(1)?, e.get(2)?, e.get(3)?),
            Codec::Hevc => hevc_string(e)?,
            Codec::Vp8 => "vp8".into(),
            Codec::Vp9 => match e.get(4..6) {
                Some(&[profile, level]) if e.len() >= 7 => {
                    format!("vp09.{profile:02}.{level:02}.{:02}", e[6] >> 4)
                }
                _ => "vp09.00.10.08".into(),
            },
            Codec::Av1 => av1_string(e)?,
            Codec::Aac => format!("mp4a.40.{}", e.first().map_or(2, |b| b >> 3)),
            Codec::Mp3 => "mp3".into(),
            Codec::Opus => "opus".into(),
            Codec::Vorbis => "vorbis".into(),
            Codec::Flac => "flac".into(),
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub duration: Option<f64>,
    pub tracks: Vec<Track>,
}

impl Info {
    pub fn video(&self) -> Option<(&Track, &VideoTrack)> {
        self.tracks.iter().find_map(|t| match &t.kind {
            TrackKind::Video(v) => Some((t, v)),
            _ => None,
        })
    }

    pub fn audio(&self) -> Option<(&Track, &AudioTrack)> {
        self.tracks.iter().find_map(|t| match &t.kind {
            TrackKind::Audio(a) => Some((t, a)),
            _ => None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Packet {
    pub track: u32,
    pub data: Vec<u8>,
    pub pts: i64,
    pub dts: i64,
    pub duration: i64,
    pub key: bool,
}

pub enum Demuxer<S> {
    Mp4(mp4::Demuxer<S>),
    Mkv(mkv::Demuxer<S>),
}

/// An MP4/MOV or Matroska/WebM file.
pub fn open<S: Source>(mut source: S) -> Result<Demuxer<S>> {
    let mut head = [0u8; 8];
    if source.size() < 8 {
        bail!("file is too short");
    }
    source.read_at(0, &mut head)?;
    let mut demuxer = if head[..4] == [0x1A, 0x45, 0xDF, 0xA3] {
        Demuxer::Mkv(mkv::Demuxer::open(source)?)
    } else if mp4::is_mp4(&head) {
        Demuxer::Mp4(mp4::Demuxer::open(source)?)
    } else {
        bail!("unsupported container (only MP4, MOV, MKV and WebM can be read)")
    };
    let info = match &mut demuxer {
        Demuxer::Mp4(d) => d.info_mut(),
        Demuxer::Mkv(d) => d.info_mut(),
    };
    for track in &mut info.tracks {
        if let TrackKind::Video(video) = &mut track.kind
            && video.color.is_none()
            && track.codec == Codec::H264
        {
            video.color = h264::avcc_color(&track.extradata);
        }
    }
    Ok(demuxer)
}

impl<S: Source> Demuxer<S> {
    pub fn info(&self) -> &Info {
        match self {
            Self::Mp4(d) => d.info(),
            Self::Mkv(d) => d.info(),
        }
    }

    pub fn select(&mut self, tracks: &[u32]) {
        match self {
            Self::Mp4(d) => d.select(tracks),
            Self::Mkv(d) => d.select(tracks),
        }
    }

    pub fn next_packet(&mut self) -> Result<Option<Packet>> {
        match self {
            Self::Mp4(d) => d.next_packet(),
            Self::Mkv(d) => d.next_packet(),
        }
    }

    /// Goes back to a key frame of the selected video at or before `us`, or
    /// as near as the file allows. Packets before `us` follow; dropping them
    /// after decoding is the caller's.
    pub fn seek(&mut self, us: i64) -> Result<()> {
        match self {
            Self::Mp4(d) => d.seek(us),
            Self::Mkv(d) => d.seek(us),
        }
    }
}
