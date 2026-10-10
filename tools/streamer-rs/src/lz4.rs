//! LZ4 block format compressor (greedy, one hash probe per position).

const MIN_MATCH: usize = 4;
const LAST_LITERALS: usize = 5;
const MF_LIMIT: usize = 12;
const MAX_OFFSET: usize = 65535;
const HASH_BITS: u32 = 16;

pub fn max_compressed(size: usize) -> usize {
    size + size / 255 + 16
}

fn read_u32(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

fn hash(value: u32) -> usize {
    (value.wrapping_mul(2654435761) >> (32 - HASH_BITS)) as usize
}

fn push_length(out: &mut Vec<u8>, mut length: usize) {
    while length >= 255 {
        out.push(255);
        length -= 255;
    }
    out.push(length as u8);
}

fn push_sequence(out: &mut Vec<u8>, literals: &[u8], offset: usize, match_length: usize) {
    let lit = literals.len();
    let ml = match_length - MIN_MATCH;
    out.push(((lit.min(15) as u8) << 4) | ml.min(15) as u8);
    if lit >= 15 {
        push_length(out, lit - 15);
    }
    out.extend_from_slice(literals);
    out.extend_from_slice(&(offset as u16).to_le_bytes());
    if ml >= 15 {
        push_length(out, ml - 15);
    }
}

fn push_last(out: &mut Vec<u8>, literals: &[u8]) {
    let lit = literals.len();
    out.push((lit.min(15) as u8) << 4);
    if lit >= 15 {
        push_length(out, lit - 15);
    }
    out.extend_from_slice(literals);
}

pub fn compress(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(max_compressed(src.len()) / 4);
    let mut anchor = 0;
    if src.len() > MF_LIMIT {
        let mut table = vec![usize::MAX; 1 << HASH_BITS];
        let limit = src.len() - MF_LIMIT;
        let match_end = src.len() - LAST_LITERALS;
        let mut i = 0;
        while i < limit {
            let value = read_u32(src, i);
            let slot = &mut table[hash(value)];
            let candidate = *slot;
            *slot = i;
            if candidate == usize::MAX
                || i - candidate > MAX_OFFSET
                || read_u32(src, candidate) != value
            {
                i += 1;
                continue;
            }
            let mut length = MIN_MATCH;
            while i + length < match_end && src[candidate + length] == src[i + length] {
                length += 1;
            }
            push_sequence(&mut out, &src[anchor..i], i - candidate, length);
            i += length;
            anchor = i;
        }
    }
    push_last(&mut out, &src[anchor..]);
    out
}
