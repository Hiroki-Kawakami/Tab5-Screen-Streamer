use super::demux::ColorInfo;

const HIGH_PROFILES: [u8; 12] = [100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134];
const UNSPECIFIED: u8 = 2;

struct Bits {
    data: Vec<u8>,
    pos: usize,
}

impl Bits {
    fn new(nal: &[u8]) -> Self {
        let mut data = Vec::with_capacity(nal.len());
        let mut zeros = 0;
        for &b in nal {
            if zeros >= 2 && b == 3 {
                zeros = 0;
                continue;
            }
            zeros = if b == 0 { zeros + 1 } else { 0 };
            data.push(b);
        }
        Self { data, pos: 0 }
    }

    fn bit(&mut self) -> Option<u32> {
        let byte = *self.data.get(self.pos / 8)?;
        let bit = (byte >> (7 - self.pos % 8)) & 1;
        self.pos += 1;
        Some(bit as u32)
    }

    fn bits(&mut self, n: u32) -> Option<u32> {
        (0..n).try_fold(0, |v, _| Some((v << 1) | self.bit()?))
    }

    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.bit()? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        Some((1u32 << zeros) - 1 + self.bits(zeros)?)
    }

    fn se(&mut self) -> Option<i32> {
        let v = self.ue()?;
        Some(if v & 1 == 1 {
            v.div_ceil(2) as i32
        } else {
            -((v / 2) as i32)
        })
    }
}

fn skip_scaling_list(b: &mut Bits, size: usize) -> Option<()> {
    let (mut last, mut next) = (8i32, 8i32);
    for _ in 0..size {
        if next != 0 {
            next = (last + b.se()? + 256) % 256;
        }
        last = if next == 0 { last } else { next };
    }
    Some(())
}

fn sps_color(nal: &[u8]) -> Option<ColorInfo> {
    let mut b = Bits::new(nal);
    b.bits(8)?;
    let profile = b.bits(8)? as u8;
    b.bits(16)?;
    b.ue()?;
    if HIGH_PROFILES.contains(&profile) {
        let chroma = b.ue()?;
        if chroma == 3 {
            b.bit()?;
        }
        b.ue()?;
        b.ue()?;
        b.bit()?;
        if b.bit()? == 1 {
            for i in 0..if chroma == 3 { 12 } else { 8 } {
                if b.bit()? == 1 {
                    skip_scaling_list(&mut b, if i < 6 { 16 } else { 64 })?;
                }
            }
        }
    }
    b.ue()?;
    match b.ue()? {
        0 => {
            b.ue()?;
        }
        1 => {
            b.bit()?;
            b.se()?;
            b.se()?;
            for _ in 0..b.ue()? {
                b.se()?;
            }
        }
        _ => {}
    }
    b.ue()?;
    b.bit()?;
    b.ue()?;
    b.ue()?;
    if b.bit()? == 0 {
        b.bit()?;
    }
    b.bit()?;
    if b.bit()? == 1 {
        for _ in 0..4 {
            b.ue()?;
        }
    }
    if b.bit()? == 0 {
        return None;
    }
    if b.bit()? == 1 && b.bits(8)? == 255 {
        b.bits(32)?;
    }
    if b.bit()? == 1 {
        b.bit()?;
    }
    if b.bit()? == 0 {
        return None;
    }
    b.bits(3)?;
    let full_range = b.bit()? == 1;
    let (primaries, transfer, matrix) = if b.bit()? == 1 {
        (b.bits(8)? as u8, b.bits(8)? as u8, b.bits(8)? as u8)
    } else {
        (UNSPECIFIED, UNSPECIFIED, UNSPECIFIED)
    };
    Some(ColorInfo {
        primaries,
        transfer,
        matrix,
        full_range: Some(full_range),
    })
}

pub fn avcc_color(avcc: &[u8]) -> Option<ColorInfo> {
    let count = *avcc.get(5)? & 0x1F;
    let mut at = 6;
    for _ in 0..count {
        let len = u16::from_be_bytes([*avcc.get(at)?, *avcc.get(at + 1)?]) as usize;
        let nal = avcc.get(at + 2..at + 2 + len)?;
        if let Some(color) = sps_color(nal) {
            return Some(color);
        }
        at += 2 + len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const BT709: [u8; 28] = [
        0x67, 0x64, 0x00, 0x0C, 0xAC, 0xD9, 0x41, 0x41, 0x9F, 0x9F, 0x01, 0x6A, 0x04, 0x04, 0x02,
        0x80, 0x00, 0x00, 0x03, 0x00, 0x80, 0x00, 0x00, 0x19, 0x07, 0x8A, 0x14, 0xCB,
    ];
    const BT470BG_FULL: [u8; 28] = [
        0x67, 0x4D, 0x40, 0x0C, 0xEC, 0xA0, 0xA0, 0xCF, 0xCF, 0x80, 0xB7, 0x02, 0x02, 0x05, 0x40,
        0x00, 0x00, 0x03, 0x00, 0x40, 0x00, 0x00, 0x0C, 0x83, 0xC5, 0x0A, 0x65, 0x80,
    ];
    const UNTAGGED: [u8; 26] = [
        0x67, 0x64, 0x00, 0x0C, 0xAC, 0xD9, 0x41, 0x41, 0x9F, 0x9F, 0x01, 0x10, 0x00, 0x00, 0x03,
        0x00, 0x10, 0x00, 0x00, 0x03, 0x03, 0x20, 0xF1, 0x42, 0x99, 0x60,
    ];

    fn avcc(sps: &[u8]) -> Vec<u8> {
        let mut out = vec![1, sps[1], sps[2], sps[3], 0xFF, 0xE1];
        out.extend((sps.len() as u16).to_be_bytes());
        out.extend(sps);
        out
    }

    #[test]
    fn reads_colour_from_the_vui() {
        let c = avcc_color(&avcc(&BT709)).unwrap();
        assert_eq!((c.matrix, c.full_range), (1, Some(false)));
        let c = avcc_color(&avcc(&BT470BG_FULL)).unwrap();
        assert_eq!((c.matrix, c.full_range), (5, Some(true)));
        assert_eq!(avcc_color(&avcc(&UNTAGGED)), None);
        assert_eq!(avcc_color(&[1, 2, 3]), None);
    }

    #[test]
    fn exponential_golomb() {
        let mut b = Bits::new(&[0xA6, 0x70]);
        assert_eq!(b.ue(), Some(0));
        assert_eq!(b.ue(), Some(1));
        assert_eq!(b.ue(), Some(2));
        assert_eq!(b.se(), Some(-3));
    }
}
