use std::collections::VecDeque;

use super::bytes::Reader;
use super::demux::{AudioTrack, Codec, ColorInfo, Info, Packet, Track, TrackKind, VideoTrack};
use super::io::Source;
use crate::bail;
use crate::error::{Context, Result};

const EBML: u32 = 0x1A45DFA3;
const DOC_TYPE: u32 = 0x4282;
const SEGMENT: u32 = 0x18538067;
const SEEK_HEAD: u32 = 0x114D9B74;
const SEEK: u32 = 0x4DBB;
const SEEK_ID: u32 = 0x53AB;
const SEEK_POSITION: u32 = 0x53AC;
const CUES: u32 = 0x1C53BB6B;
const CUE_POINT: u32 = 0xBB;
const CUE_TIME: u32 = 0xB3;
const CUE_TRACK_POSITIONS: u32 = 0xB7;
const CUE_CLUSTER_POSITION: u32 = 0xF1;
const INFO: u32 = 0x1549A966;
const TIMESTAMP_SCALE: u32 = 0x2AD7B1;
const DURATION: u32 = 0x4489;
const TRACKS: u32 = 0x1654AE6B;
const TRACK_ENTRY: u32 = 0xAE;
const TRACK_NUMBER: u32 = 0xD7;
const TRACK_TYPE: u32 = 0x83;
const CODEC_ID: u32 = 0x86;
const CODEC_PRIVATE: u32 = 0x63A2;
const DEFAULT_DURATION: u32 = 0x23E383;
const CODEC_DELAY: u32 = 0x56AA;
const VIDEO: u32 = 0xE0;
const PIXEL_WIDTH: u32 = 0xB0;
const PIXEL_HEIGHT: u32 = 0xBA;
const DISPLAY_WIDTH: u32 = 0x54B0;
const DISPLAY_HEIGHT: u32 = 0x54BA;
const DISPLAY_UNIT: u32 = 0x54B2;
const COLOUR_SPACE: u32 = 0x2EB524;
const COLOUR: u32 = 0x55B0;
const MATRIX_COEFFICIENTS: u32 = 0x55B1;
const RANGE: u32 = 0x55B9;
const TRANSFER_CHARACTERISTICS: u32 = 0x55BA;
const PRIMARIES: u32 = 0x55BB;
const AUDIO: u32 = 0xE1;
const SAMPLING_FREQUENCY: u32 = 0xB5;
const CHANNELS: u32 = 0x9F;
const BIT_DEPTH: u32 = 0x6264;
const CLUSTER: u32 = 0x1F43B675;
const TIMESTAMP: u32 = 0xE7;
const SIMPLE_BLOCK: u32 = 0xA3;
const BLOCK_GROUP: u32 = 0xA0;
const BLOCK: u32 = 0xA1;
const REFERENCE_BLOCK: u32 = 0xFB;
const BLOCK_DURATION: u32 = 0x9B;

const READ_CHUNK: usize = 1 << 20;
const MAX_ELEMENT: u64 = 256 << 20;
const AAC_RATES: [u32; 13] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
];

struct Input<S> {
    source: S,
    len: u64,
    buf: Vec<u8>,
    buf_start: u64,
}

impl<S: Source> Input<S> {
    fn fill(&mut self, pos: u64, len: usize) -> Result<usize> {
        let buf_end = self.buf_start + self.buf.len() as u64;
        if pos >= self.buf_start && pos + len as u64 <= buf_end {
            return Ok(len);
        }
        let size = (self.len - pos).min(len.max(READ_CHUNK) as u64) as usize;
        self.buf.resize(size, 0);
        let filled = self.source.read_upto(pos, &mut self.buf)?;
        self.buf.truncate(filled);
        self.buf_start = pos;
        if filled < size {
            self.len = pos + filled as u64;
        }
        Ok(filled.min(len))
    }

    fn read(&mut self, pos: u64, len: usize) -> Result<&[u8]> {
        if pos + len as u64 > self.len || self.fill(pos, len)? < len {
            bail!("truncated file");
        }
        let start = (pos - self.buf_start) as usize;
        Ok(&self.buf[start..start + len])
    }

    /// Reads at most `len` bytes at `pos` without replacing the buffer, for
    /// reads scattered over the file.
    fn peek(&mut self, pos: u64, len: usize) -> Result<Vec<u8>> {
        let mut out = vec![0; self.len.saturating_sub(pos).min(len as u64) as usize];
        let filled = self.source.read_upto(pos, &mut out)?;
        out.truncate(filled);
        Ok(out)
    }

    fn header(&mut self, pos: u64) -> Result<Option<(u32, Option<u64>, u64)>> {
        let available = (self.len - pos).min(12) as usize;
        let available = self.fill(pos, available)?;
        if available == 0 {
            return Ok(None);
        }
        let start = (pos - self.buf_start) as usize;
        let mut r = Reader::new(&self.buf[start..start + available]);
        let id = element_id(&mut r)?;
        let size = element_size(&mut r)?;
        Ok(Some((id, size, pos + (available - r.remaining()) as u64)))
    }
}

fn vint_length(first: u8) -> Result<usize> {
    if first == 0 {
        bail!("bad EBML number");
    }
    Ok(first.leading_zeros() as usize + 1)
}

fn element_id(r: &mut Reader) -> Result<u32> {
    let first = r.u8()?;
    let len = vint_length(first)?;
    if len > 4 {
        bail!("bad EBML element ID");
    }
    let mut id = first as u32;
    for _ in 1..len {
        id = (id << 8) | r.u8()? as u32;
    }
    Ok(id)
}

fn element_size(r: &mut Reader) -> Result<Option<u64>> {
    let first = r.u8()?;
    let len = vint_length(first)?;
    let mut value = (first as u64) & (0xFF >> len);
    let mut all_ones = value == (0xFF >> len) as u64;
    for _ in 1..len {
        let b = r.u8()?;
        all_ones &= b == 0xFF;
        value = (value << 8) | b as u64;
    }
    Ok((!all_ones).then_some(value))
}

fn signed_size(r: &mut Reader) -> Result<i64> {
    let start = r.remaining();
    let value = element_size(r)?.context("bad EBML lace size")? as i64;
    let len = start - r.remaining();
    Ok(value - ((1i64 << (7 * len - 1)) - 1))
}

struct Elements<'a> {
    reader: Reader<'a>,
}

fn elements(data: &[u8]) -> Elements<'_> {
    Elements {
        reader: Reader::new(data),
    }
}

impl<'a> Iterator for Elements<'a> {
    type Item = Result<(u32, &'a [u8])>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.reader.remaining() == 0 {
            return None;
        }
        Some((|| {
            let id = element_id(&mut self.reader)?;
            let size = element_size(&mut self.reader)?.map_or(self.reader.remaining(), |s| {
                (s as usize).min(self.reader.remaining())
            });
            Ok((id, self.reader.take(size)?))
        })())
    }
}

fn uint(data: &[u8]) -> u64 {
    data.iter().fold(0, |v, &b| (v << 8) | b as u64)
}

fn float(data: &[u8]) -> f64 {
    match data.len() {
        4 => f32::from_bits(uint(data) as u32) as f64,
        8 => f64::from_bits(uint(data)),
        _ => 0.0,
    }
}

fn aac_config(codec_id: &str, sample_rate: u32, channels: u32) -> Vec<u8> {
    let object_type: u8 = if codec_id.ends_with("MAIN") {
        1
    } else if codec_id.ends_with("SSR") {
        3
    } else if codec_id.ends_with("LTP") {
        4
    } else {
        2
    };
    let index = AAC_RATES
        .iter()
        .position(|&r| r == sample_rate)
        .unwrap_or(4) as u8;
    let channels = channels.min(7) as u8;
    vec![
        (object_type << 3) | (index >> 1),
        ((index & 1) << 7) | (channels << 3),
    ]
}

fn codec(codec_id: &str) -> Codec {
    match codec_id {
        "V_MPEG4/ISO/AVC" => Codec::H264,
        "V_MPEGH/ISO/HEVC" => Codec::Hevc,
        "V_VP8" => Codec::Vp8,
        "V_VP9" => Codec::Vp9,
        "V_AV1" => Codec::Av1,
        "V_MPEG1" => Codec::Mpeg1Video,
        "V_MPEG2" => Codec::Mpeg2Video,
        "V_MJPEG" => Codec::Mjpeg,
        "V_UNCOMPRESSED" => Codec::Raw,
        "A_MPEG/L3" => Codec::Mp3,
        "A_OPUS" => Codec::Opus,
        "A_VORBIS" => Codec::Vorbis,
        "A_FLAC" => Codec::Flac,
        "A_AC3" => Codec::Ac3,
        "A_EAC3" => Codec::Eac3,
        id if id.starts_with("V_MPEG4/ISO/") => Codec::Mpeg4Part2,
        id if id.starts_with("A_AAC") => Codec::Aac,
        id if id.starts_with("A_PCM/") => Codec::Pcm,
        _ => Codec::Unknown,
    }
}

fn parse_colour(body: &[u8]) -> Result<ColorInfo> {
    let mut color = ColorInfo {
        primaries: 2,
        transfer: 2,
        matrix: 2,
        full_range: None,
    };
    for item in elements(body) {
        let (id, value) = item?;
        let v = uint(value) as u8;
        match id {
            MATRIX_COEFFICIENTS => color.matrix = v,
            TRANSFER_CHARACTERISTICS => color.transfer = v,
            PRIMARIES => color.primaries = v,
            RANGE => color.full_range = matches!(v, 1 | 2).then_some(v == 2),
            _ => {}
        }
    }
    Ok(color)
}

struct TrackState {
    number: u64,
    default_duration: Option<u64>,
    codec_delay: u64,
    selected: bool,
}

fn parse_track(entry: &[u8], index: u32) -> Result<(Track, TrackState)> {
    let mut number = 0;
    let mut kind = 0;
    let mut codec_id = String::new();
    let mut private = Vec::new();
    let mut default_duration = None;
    let mut codec_delay = 0;
    let (mut width, mut height) = (0u32, 0u32);
    let (mut display_width, mut display_height, mut display_unit) = (None, None, 0);
    let mut color = None;
    let mut fourcc = None;
    let (mut sample_rate, mut channels, mut bits) = (8000.0, 1u32, None);
    for item in elements(entry) {
        let (id, body) = item?;
        match id {
            TRACK_NUMBER => number = uint(body),
            TRACK_TYPE => kind = uint(body),
            CODEC_ID => codec_id = String::from_utf8_lossy(body).trim_end_matches('\0').into(),
            CODEC_PRIVATE => private = body.to_vec(),
            DEFAULT_DURATION => default_duration = Some(uint(body)).filter(|&d| d > 0),
            CODEC_DELAY => codec_delay = uint(body),
            VIDEO => {
                for item in elements(body) {
                    let (id, body) = item?;
                    match id {
                        PIXEL_WIDTH => width = uint(body) as u32,
                        PIXEL_HEIGHT => height = uint(body) as u32,
                        DISPLAY_WIDTH => display_width = Some(uint(body) as u32),
                        DISPLAY_HEIGHT => display_height = Some(uint(body) as u32),
                        DISPLAY_UNIT => display_unit = uint(body),
                        COLOUR => color = Some(parse_colour(body)?),
                        COLOUR_SPACE => fourcc = body.try_into().ok(),
                        _ => {}
                    }
                }
            }
            AUDIO => {
                for item in elements(body) {
                    let (id, body) = item?;
                    match id {
                        SAMPLING_FREQUENCY => sample_rate = float(body),
                        CHANNELS => channels = uint(body) as u32,
                        BIT_DEPTH => bits = Some(uint(body) as u32),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    let codec = codec(&codec_id);
    let track_kind = match kind {
        1 => TrackKind::Video(VideoTrack {
            width,
            height,
            sample_aspect: match (display_width, display_height, display_unit) {
                (Some(dw), Some(dh), 0..=3) if dw > 0 && dh > 0 => {
                    Some((dw * height, dh * width)).filter(|(h, v)| h != v)
                }
                _ => None,
            },
            rotation: 0,
            color,
            fourcc,
        }),
        2 => TrackKind::Audio(AudioTrack {
            channels,
            sample_rate: sample_rate as u32,
            bits,
            bit_rate: None,
            priming: (codec_delay as f64 * sample_rate / 1e9).round() as u32,
        }),
        _ => TrackKind::Other,
    };
    if codec == Codec::Aac && private.is_empty() {
        private = aac_config(&codec_id, sample_rate as u32, channels);
    }
    Ok((
        Track {
            index,
            codec,
            tag: codec_id,
            extradata: private,
            kind: track_kind,
            frames: None,
        },
        TrackState {
            number,
            default_duration,
            codec_delay,
            selected: true,
        },
    ))
}

pub struct Demuxer<S> {
    input: Input<S>,
    info: Info,
    tracks: Vec<TrackState>,
    timestamp_scale: u64,
    segment: u64,
    first_cluster: u64,
    cues: Option<u64>,
    /// Cluster times, in timestamp_scale units, and positions; read at the
    /// first seek.
    index: Option<Vec<(i64, u64)>>,
    pos: u64,
    end: u64,
    cluster_time: i64,
    queue: VecDeque<Packet>,
}

fn parse_cues(data: &[u8], segment: u64) -> Result<Vec<(i64, u64)>> {
    let mut index = Vec::new();
    for item in elements(data) {
        let (id, point) = item?;
        if id != CUE_POINT {
            continue;
        }
        let mut time = None;
        let mut position = None;
        for item in elements(point) {
            let (id, value) = item?;
            match id {
                CUE_TIME => time = Some(uint(value) as i64),
                CUE_TRACK_POSITIONS => {
                    for item in elements(value) {
                        let (id, value) = item?;
                        if id == CUE_CLUSTER_POSITION {
                            position = Some(segment + uint(value));
                        }
                    }
                }
                _ => {}
            }
        }
        if let (Some(time), Some(position)) = (time, position) {
            index.push((time, position));
        }
    }
    index.sort_unstable();
    index.dedup_by_key(|entry| entry.1);
    Ok(index)
}

impl<S: Source> Demuxer<S> {
    pub fn open(source: S) -> Result<Self> {
        let len = source.size();
        let mut input = Input {
            source,
            len,
            buf: Vec::new(),
            buf_start: 0,
        };
        let (id, size, body) = input.header(0)?.context("empty input")?;
        if id != EBML {
            bail!("not an EBML file");
        }
        let size = size.context("EBML header of unknown size")?;
        let header = input.read(body, size as usize)?.to_vec();
        for item in elements(&header) {
            let (id, value) = item?;
            if id == DOC_TYPE {
                let doc_type = String::from_utf8_lossy(value);
                let doc_type = doc_type.trim_end_matches('\0');
                if doc_type != "matroska" && doc_type != "webm" {
                    bail!("unsupported EBML document type '{doc_type}'");
                }
            }
        }
        let (id, size, mut pos) = input.header(body + size)?.context("no Matroska segment")?;
        if id != SEGMENT {
            bail!("no Matroska segment");
        }
        let end = size.map_or(len, |s| (pos + s).min(len));
        let segment = pos;
        let mut cues = None;

        let mut timestamp_scale = 1_000_000u64;
        let mut duration = None;
        let mut tracks = Vec::new();
        let mut states = Vec::new();
        while pos < end {
            let Some((id, size, body)) = input.header(pos)? else {
                break;
            };
            if id == CLUSTER {
                break;
            }
            let size = size.context("element of unknown size before the first cluster")?;
            match id {
                SEEK_HEAD if size <= MAX_ELEMENT => {
                    let data = input.read(body, size as usize)?.to_vec();
                    for item in elements(&data) {
                        let (id, seek) = item?;
                        if id != SEEK {
                            continue;
                        }
                        let (mut target, mut position) = (None, None);
                        for item in elements(seek) {
                            let (id, value) = item?;
                            match id {
                                SEEK_ID => target = Some(uint(value) as u32),
                                SEEK_POSITION => position = Some(uint(value)),
                                _ => {}
                            }
                        }
                        if let (Some(CUES), Some(position)) = (target, position) {
                            cues = Some(segment + position);
                        }
                    }
                }
                CUES => cues = Some(pos),
                INFO | TRACKS => {
                    if size > MAX_ELEMENT {
                        bail!("element too large");
                    }
                    let data = input.read(body, size as usize)?.to_vec();
                    if id == INFO {
                        for item in elements(&data) {
                            let (id, value) = item?;
                            match id {
                                TIMESTAMP_SCALE => timestamp_scale = uint(value).max(1),
                                DURATION => duration = Some(float(value)),
                                _ => {}
                            }
                        }
                    } else {
                        for item in elements(&data) {
                            let (id, entry) = item?;
                            if id == TRACK_ENTRY {
                                let (track, state) = parse_track(entry, tracks.len() as u32)?;
                                tracks.push(track);
                                states.push(state);
                            }
                        }
                    }
                }
                _ => {}
            }
            pos = body + size;
        }
        let duration = duration
            .map(|d| d * timestamp_scale as f64 / 1e9)
            .filter(|d| d.is_finite() && *d > 0.0);
        Ok(Self {
            input,
            info: Info { duration, tracks },
            tracks: states,
            timestamp_scale,
            segment,
            first_cluster: pos,
            cues,
            index: None,
            pos,
            end,
            cluster_time: 0,
            queue: VecDeque::new(),
        })
    }

    pub fn info(&self) -> &Info {
        &self.info
    }

    pub(super) fn info_mut(&mut self) -> &mut Info {
        &mut self.info
    }

    pub fn select(&mut self, tracks: &[u32]) {
        for (i, state) in self.tracks.iter_mut().enumerate() {
            state.selected = tracks.contains(&(i as u32));
        }
    }

    fn us(&self, time: i64) -> i64 {
        (time as i128 * self.timestamp_scale as i128 / 1000) as i64
    }

    fn read_cues(&mut self, pos: u64) -> Result<Option<Vec<(i64, u64)>>> {
        let Some((CUES, Some(size), body)) = self.input.header(pos)? else {
            return Ok(None);
        };
        if size > MAX_ELEMENT {
            bail!("cues too large");
        }
        let data = self.input.peek(body, size as usize)?;
        let index = parse_cues(&data, self.segment)?;
        Ok((!index.is_empty()).then_some(index))
    }

    /// Every cluster's time, from their headers, for files without cues.
    fn scan_clusters(&mut self) -> Result<Vec<(i64, u64)>> {
        let mut index = Vec::new();
        let mut pos = self.first_cluster;
        while pos < self.end {
            let head = self.input.peek(pos, 32)?;
            let mut r = Reader::new(&head);
            let (Ok(id), Ok(size)) = (element_id(&mut r), element_size(&mut r)) else {
                break;
            };
            let body = pos + (head.len() - r.remaining()) as u64;
            if id == CLUSTER
                && let Ok(TIMESTAMP) = element_id(&mut r)
                && let Ok(Some(length)) = element_size(&mut r)
                && let Ok(value) = r.take(length as usize)
            {
                index.push((uint(value) as i64, pos));
            }
            let Some(size) = size else {
                bail!("cannot seek: element of unknown size");
            };
            pos = body + size;
        }
        Ok(index)
    }

    pub fn seek(&mut self, us: i64) -> Result<()> {
        if self.index.is_none() {
            let cues = match self.cues {
                Some(pos) => self.read_cues(pos)?,
                None => None,
            };
            let index = match cues {
                Some(index) => index,
                None => self.scan_clusters()?,
            };
            self.index = Some(index);
        }
        let index = self.index.as_deref().unwrap_or_default();
        let time = (us as i128 * 1000 / self.timestamp_scale as i128) as i64;
        let at = index.partition_point(|&(t, _)| t <= time).saturating_sub(1);
        self.pos = index.get(at).map_or(self.first_cluster, |&(_, pos)| pos);
        self.cluster_time = 0;
        self.queue.clear();
        Ok(())
    }

    pub fn next_packet(&mut self) -> Result<Option<Packet>> {
        while self.queue.is_empty() {
            if self.pos >= self.end {
                return Ok(None);
            }
            let Some((id, size, body)) = self.input.header(self.pos)? else {
                return Ok(None);
            };
            match id {
                CLUSTER => {
                    self.pos = body;
                    continue;
                }
                TIMESTAMP | SIMPLE_BLOCK | BLOCK_GROUP => {}
                _ => {
                    let size = size.with_context(|| format!("element {id:#x} of unknown size"))?;
                    self.pos = body + size;
                    continue;
                }
            }
            let size = size.context("block of unknown size")?;
            if size > MAX_ELEMENT {
                bail!("block too large");
            }
            let data = self.input.read(body, size as usize)?.to_vec();
            self.pos = body + size;
            match id {
                TIMESTAMP => self.cluster_time = uint(&data) as i64,
                SIMPLE_BLOCK => {
                    let key = data.get(3).is_some_and(|flags| flags & 0x80 != 0);
                    self.block(&data, key, None)?;
                }
                _ => {
                    let mut block = None;
                    let mut key = true;
                    let mut duration = None;
                    for item in elements(&data) {
                        let (id, value) = item?;
                        match id {
                            BLOCK => block = Some(value),
                            REFERENCE_BLOCK => key = false,
                            BLOCK_DURATION => duration = Some(uint(value) as i64),
                            _ => {}
                        }
                    }
                    if let Some(block) = block {
                        self.block(block, key, duration)?;
                    }
                }
            }
        }
        Ok(self.queue.pop_front())
    }

    fn block(&mut self, data: &[u8], key: bool, duration: Option<i64>) -> Result<()> {
        let mut r = Reader::new(data);
        let number = element_size(&mut r)?.context("bad block track number")?;
        let Some(index) = self.tracks.iter().position(|t| t.number == number) else {
            return Ok(());
        };
        if !self.tracks[index].selected {
            return Ok(());
        }
        let relative = r.u16()? as i16 as i64;
        let flags = r.u8()?;
        let sizes = match (flags >> 1) & 3 {
            0 => vec![r.remaining()],
            lacing => {
                let count = r.u8()? as usize + 1;
                let mut sizes = Vec::with_capacity(count);
                match lacing {
                    1 => {
                        for _ in 1..count {
                            let mut size = 0;
                            loop {
                                let b = r.u8()?;
                                size += b as usize;
                                if b != 255 {
                                    break;
                                }
                            }
                            sizes.push(size);
                        }
                    }
                    3 => {
                        let mut size = element_size(&mut r)?.context("bad lace size")? as i64;
                        sizes.push(size as usize);
                        for _ in 2..count {
                            size += signed_size(&mut r)?;
                            sizes.push(usize::try_from(size).context("bad lace size")?);
                        }
                    }
                    _ => {
                        let each = r.remaining() / count;
                        sizes.resize(count - 1, each);
                    }
                }
                let used: usize = sizes.iter().sum();
                sizes.push(r.remaining().checked_sub(used).context("bad lace sizes")?);
                sizes
            }
        };
        let state = &self.tracks[index];
        let frame_duration = state.default_duration.map(|d| d as i64 / 1000);
        let delay = (state.codec_delay + self.timestamp_scale / 2) / self.timestamp_scale;
        let start = self.us(self.cluster_time + relative - delay as i64);
        let duration = duration
            .map(|d| self.us(d) / sizes.len() as i64)
            .or(frame_duration)
            .unwrap_or(0);
        for (i, size) in sizes.into_iter().enumerate() {
            let pts = start + i as i64 * duration;
            self.queue.push_back(Packet {
                track: index as u32,
                data: r.take(size)?.to_vec(),
                pts,
                dts: pts,
                duration,
                key,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(id: u32, body: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = id
            .to_be_bytes()
            .into_iter()
            .skip_while(|&b| b == 0)
            .collect();
        out.push(0x01);
        out.extend((body.len() as u64).to_be_bytes()[1..].iter());
        out.extend(body);
        out
    }

    fn unknown_size(id: u32, body: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = id
            .to_be_bytes()
            .into_iter()
            .skip_while(|&b| b == 0)
            .collect();
        out.push(0xFF);
        out.extend(body);
        out
    }

    fn file(blocks: &[Vec<u8>]) -> Vec<u8> {
        let track = [
            element(TRACK_NUMBER, &[1]),
            element(TRACK_TYPE, &[2]),
            element(CODEC_ID, b"A_MPEG/L3"),
            element(DEFAULT_DURATION, &20_000_000u32.to_be_bytes()),
            element(AUDIO, &element(CHANNELS, &[2])),
        ]
        .concat();
        let mut cluster = element(TIMESTAMP, &[100]);
        for block in blocks {
            cluster.extend(element(SIMPLE_BLOCK, block));
        }
        let segment = [
            element(INFO, &element(TIMESTAMP_SCALE, &[0x0F, 0x42, 0x40])),
            element(TRACKS, &element(TRACK_ENTRY, &track)),
            unknown_size(CLUSTER, &cluster),
            element(0x1C53BB6B, &[0; 4]),
        ]
        .concat();
        [
            element(EBML, &element(DOC_TYPE, b"matroska")),
            unknown_size(SEGMENT, &segment),
        ]
        .concat()
    }

    fn block(lacing: u8, header: &[u8], frames: &[&[u8]]) -> Vec<u8> {
        let mut out = vec![0x81, 0x00, 0x05, 0x80 | (lacing << 1)];
        if lacing != 0 {
            out.push(frames.len() as u8 - 1);
        }
        out.extend(header);
        for frame in frames {
            out.extend(*frame);
        }
        out
    }

    fn packets(data: &[u8]) -> Vec<(i64, Vec<u8>)> {
        let mut demuxer = Demuxer::open(data).unwrap();
        let mut out = Vec::new();
        while let Some(p) = demuxer.next_packet().unwrap() {
            assert!(p.key);
            out.push((p.pts, p.data));
        }
        out
    }

    #[test]
    fn lacing() {
        let (a, b, c) = (&[1u8; 3][..], &[2u8; 300][..], &[3u8; 5][..]);
        let xiph = block(1, &[3, 255, 45], &[a, b, c]);
        let ebml = block(3, &[0x83, 0x61, 0x28], &[a, b, c]);
        let fixed = block(2, &[], &[a, a, a]);
        let plain = block(0, &[], &[c]);
        let got = packets(&file(&[xiph, ebml, fixed, plain]));
        let times: Vec<i64> = got.iter().map(|p| p.0).collect();
        assert_eq!(
            times,
            [
                105_000, 125_000, 145_000, 105_000, 125_000, 145_000, 105_000, 125_000, 145_000,
                105_000
            ]
            .map(|t| t as i64)
        );
        let sizes: Vec<usize> = got.iter().map(|p| p.1.len()).collect();
        assert_eq!(sizes, [3, 300, 5, 3, 300, 5, 3, 3, 3, 5]);
        assert!(got[1].1.iter().all(|&x| x == 2));
    }

    #[test]
    fn seeks_to_a_cluster() {
        let track = [
            element(TRACK_NUMBER, &[1]),
            element(TRACK_TYPE, &[2]),
            element(CODEC_ID, b"A_MPEG/L3"),
            element(AUDIO, &element(CHANNELS, &[2])),
        ]
        .concat();
        let cluster = |time: u8| {
            let blocks = [
                element(TIMESTAMP, &[time]),
                element(SIMPLE_BLOCK, &block(0, &[], &[&[time]])),
            ];
            element(CLUSTER, &blocks.concat())
        };
        let segment = [
            element(INFO, &element(TIMESTAMP_SCALE, &[0x0F, 0x42, 0x40])),
            element(TRACKS, &element(TRACK_ENTRY, &track)),
            cluster(0),
            cluster(10),
            cluster(20),
        ]
        .concat();
        let data = [
            element(EBML, &element(DOC_TYPE, b"matroska")),
            element(SEGMENT, &segment),
        ]
        .concat();
        let mut demuxer = Demuxer::open(&data[..]).unwrap();
        demuxer.seek(15_000).unwrap();
        let next = |d: &mut Demuxer<&[u8]>| d.next_packet().unwrap().map(|p| p.data[0]);
        assert_eq!(next(&mut demuxer), Some(10));
        assert_eq!(next(&mut demuxer), Some(20));
        assert_eq!(next(&mut demuxer), None);
        demuxer.seek(0).unwrap();
        assert_eq!(next(&mut demuxer), Some(0));
    }
}
