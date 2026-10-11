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

impl Track {
    pub fn audio(&self) -> Option<&AudioTrack> {
        match &self.kind {
            TrackKind::Audio(audio) => Some(audio),
            _ => None,
        }
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
