use super::bytes::{Reader, fourcc_text};
use super::demux::{AudioTrack, Codec, ColorInfo, Info, Packet, Track, TrackKind, VideoTrack};
use super::io::Source;
use crate::bail;
use crate::error::{Context, Result};

const TOP_LEVEL: [&[u8; 4]; 8] = [
    b"ftyp", b"moov", b"mdat", b"free", b"skip", b"wide", b"pnot", b"uuid",
];
const MAX_MOOV: u64 = 512 << 20;

pub fn is_mp4(head: &[u8; 8]) -> bool {
    TOP_LEVEL.iter().any(|t| head[4..8] == t[..])
}

struct Boxes<'a> {
    reader: Reader<'a>,
}

fn boxes(data: &[u8]) -> Boxes<'_> {
    Boxes {
        reader: Reader::new(data),
    }
}

impl<'a> Iterator for Boxes<'a> {
    type Item = Result<([u8; 4], &'a [u8])>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.reader.remaining() < 8 {
            return None;
        }
        Some(self.read())
    }
}

impl<'a> Boxes<'a> {
    fn read(&mut self) -> Result<([u8; 4], &'a [u8])> {
        let size = self.reader.u32()? as u64;
        let kind = self.reader.fourcc()?;
        let body = match size {
            0 => self.reader.remaining() as u64,
            1 => self.reader.u64()?.checked_sub(16).context("bad box size")?,
            _ => size.checked_sub(8).context("bad box size")?,
        };
        let body = usize::try_from(body).context("box too large")?;
        Ok((kind, self.reader.take(body)?))
    }
}

fn find<'a>(data: &'a [u8], kind: &[u8; 4]) -> Result<Option<&'a [u8]>> {
    for item in boxes(data) {
        let (k, body) = item?;
        if &k == kind {
            return Ok(Some(body));
        }
    }
    Ok(None)
}

fn require<'a>(data: &'a [u8], kind: &[u8; 4]) -> Result<&'a [u8]> {
    find(data, kind)?.with_context(|| format!("missing {} box", fourcc_text(*kind)))
}

fn full_box(body: &[u8]) -> Result<(u8, Reader<'_>)> {
    let mut r = Reader::new(body);
    let version = r.u8()?;
    r.skip(3)?;
    Ok((version, r))
}

#[derive(Clone, Copy)]
struct Sample {
    offset: u64,
    size: u32,
    dts: i64,
    cts: i32,
    duration: u32,
    sync: bool,
}

struct Samples {
    timescale: u32,
    shift: i64,
    delay_us: i64,
    list: Vec<Sample>,
    next: usize,
    selected: bool,
}

impl Samples {
    fn us(&self, value: i64) -> i64 {
        (value as i128 * 1_000_000 / self.timescale as i128) as i64
    }

    fn pts(&self, sample: &Sample) -> i64 {
        self.us(sample.dts + sample.cts as i64 - self.shift) + self.delay_us
    }
}

pub struct Demuxer<S> {
    source: S,
    info: Info,
    tracks: Vec<Samples>,
}

struct Descriptor<'a> {
    tag: u8,
    body: &'a [u8],
}

fn descriptor<'a>(r: &mut Reader<'a>) -> Result<Descriptor<'a>> {
    let tag = r.u8()?;
    let mut len = 0usize;
    for _ in 0..4 {
        let b = r.u8()?;
        len = (len << 7) | (b & 0x7F) as usize;
        if b & 0x80 == 0 {
            break;
        }
    }
    Ok(Descriptor {
        tag,
        body: r.take(len.min(r.remaining()))?,
    })
}

struct Esds {
    object_type: u8,
    avg_bitrate: u32,
    specific: Vec<u8>,
}

fn parse_esds(body: &[u8]) -> Result<Esds> {
    let (_, mut r) = full_box(body)?;
    let es = descriptor(&mut r)?;
    if es.tag != 0x03 {
        bail!("esds without ES_Descriptor");
    }
    let mut r = Reader::new(es.body);
    r.skip(2)?;
    let flags = r.u8()?;
    if flags & 0x80 != 0 {
        r.skip(2)?;
    }
    if flags & 0x40 != 0 {
        let len = r.u8()? as usize;
        r.skip(len)?;
    }
    if flags & 0x20 != 0 {
        r.skip(2)?;
    }
    while r.remaining() > 0 {
        let d = descriptor(&mut r)?;
        if d.tag != 0x04 {
            continue;
        }
        let mut c = Reader::new(d.body);
        let object_type = c.u8()?;
        c.skip(1 + 3 + 4)?;
        let avg_bitrate = c.u32()?;
        let mut specific = Vec::new();
        while c.remaining() > 0 {
            let s = descriptor(&mut c)?;
            if s.tag == 0x05 {
                specific = s.body.to_vec();
                break;
            }
        }
        return Ok(Esds {
            object_type,
            avg_bitrate,
            specific,
        });
    }
    bail!("esds without DecoderConfigDescriptor")
}

fn find_esds(children: &[u8]) -> Result<Option<Esds>> {
    for item in boxes(children) {
        let (kind, body) = item?;
        match &kind {
            b"esds" => return parse_esds(body).map(Some),
            b"wave" => {
                if let Some(esds) = find_esds(body)? {
                    return Ok(Some(esds));
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

fn opus_head(dops: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(dops);
    r.skip(1)?;
    let channels = r.u8()?;
    let pre_skip = r.u16()?;
    let rate = r.u32()?;
    let gain = r.u16()?;
    let family = r.u8()?;
    let mut head = b"OpusHead".to_vec();
    head.extend([1, channels]);
    head.extend(pre_skip.to_le_bytes());
    head.extend(rate.to_le_bytes());
    head.extend(gain.to_le_bytes());
    head.push(family);
    head.extend(r.rest());
    Ok(head)
}

struct Entry {
    codec: Codec,
    tag: String,
    extradata: Vec<u8>,
    width: u32,
    height: u32,
    sample_aspect: Option<(u32, u32)>,
    color: Option<ColorInfo>,
    channels: u32,
    sample_rate: u32,
    avg_bitrate: u32,
}

fn video_entry(kind: [u8; 4], body: &[u8]) -> Result<Entry> {
    let mut r = Reader::new(body);
    r.skip(24)?;
    let width = r.u16()? as u32;
    let height = r.u16()? as u32;
    r.skip(50)?;
    let children = r.rest();
    let mut codec = match &kind {
        b"avc1" | b"avc3" => Codec::H264,
        b"hvc1" | b"hev1" => Codec::Hevc,
        b"vp08" => Codec::Vp8,
        b"vp09" => Codec::Vp9,
        b"av01" => Codec::Av1,
        b"jpeg" | b"mjpa" | b"mjpb" => Codec::Mjpeg,
        b"m2v1" => Codec::Mpeg2Video,
        _ => Codec::Unknown,
    };
    let mut extradata = Vec::new();
    let mut sample_aspect = None;
    let mut color = None;
    let mut avg_bitrate = 0;
    for item in boxes(children) {
        let Ok((child, data)) = item else { break };
        match &child {
            b"avcC" | b"hvcC" | b"av1C" | b"vpcC" => extradata = data.to_vec(),
            b"esds" => {
                let esds = parse_esds(data)?;
                if kind == *b"mp4v" {
                    codec = Codec::from_mpeg4_object_type(esds.object_type);
                }
                avg_bitrate = esds.avg_bitrate;
                if extradata.is_empty() {
                    extradata = esds.specific;
                }
            }
            b"pasp" => {
                let mut p = Reader::new(data);
                sample_aspect = Some((p.u32()?, p.u32()?));
            }
            b"colr" => {
                let mut c = Reader::new(data);
                let kind = c.fourcc()?;
                if &kind == b"nclx" || &kind == b"nclc" {
                    color = Some(ColorInfo {
                        primaries: c.u16()? as u8,
                        transfer: c.u16()? as u8,
                        matrix: c.u16()? as u8,
                        full_range: (&kind == b"nclx")
                            .then(|| c.u8().map(|b| b & 0x80 != 0))
                            .transpose()?,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(Entry {
        codec,
        tag: fourcc_text(kind),
        extradata,
        width,
        height,
        sample_aspect,
        color,
        channels: 0,
        sample_rate: 0,
        avg_bitrate,
    })
}

fn audio_entry(kind: [u8; 4], body: &[u8]) -> Result<Entry> {
    let mut r = Reader::new(body);
    r.skip(8)?;
    let version = r.u16()?;
    r.skip(6)?;
    let mut channels = r.u16()? as u32;
    r.skip(6)?;
    let mut sample_rate = r.u32()? >> 16;
    match version {
        1 => r.skip(16)?,
        2 => {
            r.skip(4)?;
            sample_rate = f64::from_bits(r.u64()?) as u32;
            channels = r.u32()?;
            r.skip(20)?;
        }
        _ => {}
    }
    let children = r.rest();
    let mut codec = match &kind {
        b".mp3" => Codec::Mp3,
        b"Opus" => Codec::Opus,
        b"fLaC" => Codec::Flac,
        b"ac-3" => Codec::Ac3,
        b"ec-3" => Codec::Eac3,
        b"sowt" | b"twos" | b"lpcm" | b"in24" | b"in32" | b"fl32" | b"fl64" => Codec::Pcm,
        _ => Codec::Unknown,
    };
    let mut extradata = Vec::new();
    let mut avg_bitrate = 0;
    if let Some(esds) = find_esds(children)? {
        if kind == *b"mp4a" {
            codec = Codec::from_mpeg4_object_type(esds.object_type);
        }
        avg_bitrate = esds.avg_bitrate;
        extradata = esds.specific;
    }
    for item in boxes(children) {
        let Ok((child, data)) = item else { break };
        match &child {
            b"dOps" => extradata = opus_head(data)?,
            b"dfLa" => {
                extradata = b"fLaC".to_vec();
                extradata.extend(data.get(4..).unwrap_or_default());
            }
            _ => {}
        }
    }
    Ok(Entry {
        codec,
        tag: fourcc_text(kind),
        extradata,
        width: 0,
        height: 0,
        sample_aspect: None,
        color: None,
        channels,
        sample_rate,
        avg_bitrate,
    })
}

fn rotation(matrix: [i32; 4]) -> u32 {
    let [a, b, _, _] = matrix.map(|v| v as f64);
    let degrees = b.atan2(a).to_degrees();
    ((degrees / 90.0).round() as i64 * 90).rem_euclid(360) as u32
}

struct Edit {
    delay: i64,
    media_time: i64,
}

fn parse_elst(body: &[u8], movie_timescale: u32, timescale: u32) -> Result<Edit> {
    let (version, mut r) = full_box(body)?;
    let count = r.u32()?;
    let mut delay = 0i64;
    for _ in 0..count {
        let (duration, media_time) = if version == 1 {
            (r.u64()? as i64, r.u64()? as i64)
        } else {
            (r.u32()? as i64, r.i32()? as i64)
        };
        r.skip(4)?;
        if media_time == -1 {
            delay += duration;
            continue;
        }
        let delay = (delay as i128 * timescale as i128 / movie_timescale.max(1) as i128) as i64;
        return Ok(Edit { delay, media_time });
    }
    Ok(Edit {
        delay: 0,
        media_time: 0,
    })
}

fn sizes(stbl: &[u8]) -> Result<Vec<u32>> {
    if let Some(stsz) = find(stbl, b"stsz")? {
        let (_, mut r) = full_box(stsz)?;
        let fixed = r.u32()?;
        let count = r.u32()?;
        return (0..count)
            .map(|_| if fixed != 0 { Ok(fixed) } else { r.u32() })
            .collect();
    }
    let (_, mut r) = full_box(require(stbl, b"stz2")?)?;
    r.skip(3)?;
    let field = r.u8()?;
    let count = r.u32()? as usize;
    let mut sizes = Vec::with_capacity(count);
    while sizes.len() < count {
        match field {
            4 => {
                let b = r.u8()?;
                sizes.push((b >> 4) as u32);
                if sizes.len() < count {
                    sizes.push((b & 0x0F) as u32);
                }
            }
            8 => sizes.push(r.u8()? as u32),
            16 => sizes.push(r.u16()? as u32),
            _ => bail!("bad stz2 field size {field}"),
        }
    }
    Ok(sizes)
}

fn sample_table(stbl: &[u8]) -> Result<Vec<Sample>> {
    let sizes = sizes(stbl)?;
    let offsets: Vec<u64> = if let Some(stco) = find(stbl, b"stco")? {
        let (_, mut s) = full_box(stco)?;
        let count = s.u32()?;
        (0..count)
            .map(|_| s.u32().map(u64::from))
            .collect::<Result<_>>()?
    } else {
        let (_, mut s) = full_box(require(stbl, b"co64")?)?;
        let count = s.u32()?;
        (0..count).map(|_| s.u64()).collect::<Result<_>>()?
    };

    let (_, mut s) = full_box(require(stbl, b"stsc")?)?;
    let runs: Vec<(u32, u32)> = (0..s.u32()?)
        .map(|_| -> Result<(u32, u32)> {
            let first = s.u32()?;
            let per_chunk = s.u32()?;
            s.skip(4)?;
            Ok((first, per_chunk))
        })
        .collect::<Result<_>>()?;

    let mut samples = Vec::with_capacity(sizes.len());
    let mut index = 0;
    'chunks: for (chunk, &offset) in offsets.iter().enumerate() {
        let number = chunk as u32 + 1;
        let per_chunk = runs
            .iter()
            .take_while(|(first, _)| *first <= number)
            .last()
            .map_or(0, |r| r.1);
        let mut offset = offset;
        for _ in 0..per_chunk {
            let Some(&size) = sizes.get(index) else {
                break 'chunks;
            };
            samples.push(Sample {
                offset,
                size,
                dts: 0,
                cts: 0,
                duration: 0,
                sync: true,
            });
            offset += size as u64;
            index += 1;
        }
    }

    let (_, mut s) = full_box(require(stbl, b"stts")?)?;
    let mut dts = 0i64;
    let mut it = samples.iter_mut();
    for _ in 0..s.u32()? {
        let count = s.u32()?;
        let delta = s.u32()?;
        for _ in 0..count {
            let Some(sample) = it.next() else { break };
            sample.dts = dts;
            sample.duration = delta;
            dts += delta as i64;
        }
    }

    if let Some(ctts) = find(stbl, b"ctts")? {
        let (_, mut s) = full_box(ctts)?;
        let mut it = samples.iter_mut();
        for _ in 0..s.u32()? {
            let count = s.u32()?;
            let offset = s.i32()?;
            for _ in 0..count {
                let Some(sample) = it.next() else { break };
                sample.cts = offset;
            }
        }
    }

    if let Some(stss) = find(stbl, b"stss")? {
        let (_, mut s) = full_box(stss)?;
        samples.iter_mut().for_each(|sample| sample.sync = false);
        for _ in 0..s.u32()? {
            if let Some(sample) = samples.get_mut((s.u32()? as usize).wrapping_sub(1)) {
                sample.sync = true;
            }
        }
    }
    Ok(samples)
}

fn parse_track(trak: &[u8], index: u32, movie_timescale: u32) -> Result<Option<(Track, Samples)>> {
    let (version, mut t) = full_box(require(trak, b"tkhd")?)?;
    t.skip(if version == 1 { 32 } else { 20 })?;
    t.skip(16)?;
    let mut matrix = [0i32; 4];
    for (i, m) in matrix.iter_mut().enumerate() {
        *m = t.i32()?;
        if i == 1 {
            t.skip(4)?;
        }
    }

    let mdia = require(trak, b"mdia")?;
    let (version, mut m) = full_box(require(mdia, b"mdhd")?)?;
    m.skip(if version == 1 { 16 } else { 8 })?;
    let timescale = m.u32()?;
    if timescale == 0 {
        bail!("track {index} has a zero timescale");
    }
    let (_, mut h) = full_box(require(mdia, b"hdlr")?)?;
    h.skip(4)?;
    let handler = h.fourcc()?;
    let stbl = require(require(mdia, b"minf")?, b"stbl")?;
    let (_, mut d) = full_box(require(stbl, b"stsd")?)?;
    d.skip(4)?;
    let Some(entry) = boxes(d.rest()).next() else {
        return Ok(None);
    };
    let (kind, body) = entry?;
    let entry = match &handler {
        b"vide" => video_entry(kind, body)?,
        b"soun" => audio_entry(kind, body)?,
        _ => return Ok(None),
    };

    let list = sample_table(stbl)?;
    let edit = match find(trak, b"edts")?
        .map(|edts| find(edts, b"elst"))
        .transpose()?
        .flatten()
    {
        Some(elst) => parse_elst(elst, movie_timescale, timescale)?,
        None => Edit {
            delay: 0,
            media_time: 0,
        },
    };
    let span: i64 = list.iter().map(|s| s.duration as i64).sum();
    let bytes: u64 = list.iter().map(|s| s.size as u64).sum();
    let frames = list.len() as u64;
    let kind = match &handler {
        b"vide" => TrackKind::Video(VideoTrack {
            width: entry.width,
            height: entry.height,
            sample_aspect: entry.sample_aspect,
            rotation: rotation(matrix),
            color: entry.color,
            fourcc: None,
        }),
        _ => TrackKind::Audio(AudioTrack {
            channels: entry.channels,
            sample_rate: entry.sample_rate,
            bits: None,
            priming: u32::try_from(edit.media_time).unwrap_or(0),
            bit_rate: (span > 0)
                .then(|| (bytes as u128 * 8 * timescale as u128 / span as u128) as u64)
                .filter(|&b| b > 0)
                .or((entry.avg_bitrate > 0).then_some(entry.avg_bitrate as u64)),
        }),
    };
    let track = Track {
        index,
        codec: entry.codec,
        tag: entry.tag,
        extradata: entry.extradata,
        kind,
        frames: Some(frames),
    };
    let samples = Samples {
        timescale,
        shift: edit.media_time,
        delay_us: 0,
        list,
        next: 0,
        selected: true,
    };
    let delay_us = samples.us(edit.delay);
    Ok(Some((
        track,
        Samples {
            delay_us,
            ..samples
        },
    )))
}

impl<S: Source> Demuxer<S> {
    pub fn open(mut source: S) -> Result<Self> {
        let len = source.size();
        let mut pos = 0u64;
        let mut moov = None;
        while pos + 8 <= len {
            let mut head = [0u8; 16];
            let n = (len - pos).min(16) as usize;
            source.read_at(pos, &mut head[..n])?;
            let mut r = Reader::new(&head[..n]);
            let size32 = r.u32()?;
            let kind = r.fourcc()?;
            let (size, header) = match size32 {
                0 => (len - pos, 8),
                1 => (r.u64()?, 16),
                s => (s as u64, 8),
            };
            if size < header {
                bail!("bad box size at offset {pos}");
            }
            match &kind {
                b"moov" => {
                    if size > MAX_MOOV {
                        bail!("moov box is too large");
                    }
                    let mut body = vec![0u8; (size - header) as usize];
                    source.read_at(pos + header, &mut body)?;
                    moov = Some(body);
                }
                b"moof" => bail!("fragmented MP4 is not supported"),
                _ => {}
            }
            pos += size;
        }
        let moov = moov.context("no moov box")?;
        let (version, mut mvhd) = full_box(require(&moov, b"mvhd")?)?;
        mvhd.skip(if version == 1 { 16 } else { 8 })?;
        let movie_timescale = mvhd.u32()?;
        let movie_duration = if version == 1 {
            mvhd.u64()?
        } else {
            mvhd.u32()? as u64
        };

        let mut tracks = Vec::new();
        let mut samples = Vec::new();
        let mut index = 0;
        for item in boxes(&moov) {
            let (kind, body) = item?;
            if &kind != b"trak" {
                continue;
            }
            match parse_track(body, index, movie_timescale)? {
                Some((track, list)) => {
                    tracks.push(track);
                    samples.push(list);
                }
                None => {
                    tracks.push(Track {
                        index,
                        codec: Codec::Unknown,
                        tag: String::new(),
                        extradata: Vec::new(),
                        kind: TrackKind::Other,
                        frames: None,
                    });
                    samples.push(Samples {
                        timescale: 1,
                        shift: 0,
                        delay_us: 0,
                        list: Vec::new(),
                        next: 0,
                        selected: false,
                    });
                }
            }
            index += 1;
        }
        let duration = (movie_timescale > 0 && movie_duration > 0)
            .then(|| movie_duration as f64 / movie_timescale as f64);
        Ok(Self {
            source,
            info: Info { duration, tracks },
            tracks: samples,
        })
    }

    pub fn info(&self) -> &Info {
        &self.info
    }

    pub(super) fn info_mut(&mut self) -> &mut Info {
        &mut self.info
    }

    pub fn select(&mut self, tracks: &[u32]) {
        for (i, samples) in self.tracks.iter_mut().enumerate() {
            samples.selected = tracks.contains(&(i as u32));
        }
    }

    pub fn next_packet(&mut self) -> Result<Option<Packet>> {
        let next = self
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.selected)
            .filter_map(|(i, t)| t.list.get(t.next).map(|s| (i, s.offset)))
            .min_by_key(|&(_, offset)| offset);
        let Some((i, _)) = next else {
            return Ok(None);
        };
        let track = &mut self.tracks[i];
        let sample = track.list[track.next];
        track.next += 1;
        let mut data = vec![0u8; sample.size as usize];
        self.source.read_at(sample.offset, &mut data)?;
        let track = &self.tracks[i];
        Ok(Some(Packet {
            track: i as u32,
            data,
            pts: track.pts(&sample),
            dts: track.us(sample.dts - track.shift) + track.delay_us,
            duration: track.us(sample.duration as i64),
            key: sample.sync,
        }))
    }

    pub fn seek(&mut self, us: i64) -> Result<()> {
        let video = self.info.tracks.iter().position(|t| {
            matches!(t.kind, TrackKind::Video(_)) && self.tracks[t.index as usize].selected
        });
        let start = match video {
            Some(i) => {
                let t = &mut self.tracks[i];
                let keys = (0..t.list.len()).filter(|&n| t.list[n].sync);
                let key = keys
                    .take_while(|&n| t.pts(&t.list[n]) <= us)
                    .last()
                    .or_else(|| t.list.iter().position(|s| s.sync))
                    .unwrap_or(0);
                t.next = key;
                t.list.get(key).map_or(us, |s| t.pts(s).min(us))
            }
            None => us,
        };
        for (i, t) in self.tracks.iter_mut().enumerate() {
            if Some(i) == video || !t.selected {
                continue;
            }
            t.next = t
                .list
                .iter()
                .position(|s| t.pts(s) + t.us(s.duration as i64) > start)
                .unwrap_or(t.list.len());
        }
        Ok(())
    }
}
