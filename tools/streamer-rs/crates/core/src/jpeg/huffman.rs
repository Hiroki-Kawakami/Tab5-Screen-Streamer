use super::tables::HuffmanSpec;

#[derive(Clone, Copy, Default)]
pub struct Code {
    pub bits: u16,
    pub len: u8,
}

pub struct Table {
    pub spec: HuffmanSpec,
    pub codes: [Code; 256],
}

impl Table {
    pub fn new(spec: HuffmanSpec) -> Self {
        let mut codes = [Code::default(); 256];
        let mut code = 0u16;
        let mut k = 0;
        for len in 1..=16u8 {
            for _ in 0..spec.bits[len as usize - 1] {
                codes[spec.values[k] as usize] = Code { bits: code, len };
                code += 1;
                k += 1;
            }
            code <<= 1;
        }
        Self { spec, codes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jpeg::tables::standard_ac_luma;

    #[test]
    fn standard_codes_are_prefix_free() {
        let table = Table::new(standard_ac_luma());
        let used: Vec<Code> = table
            .spec
            .values
            .iter()
            .map(|&v| table.codes[v as usize])
            .collect();
        for (i, a) in used.iter().enumerate() {
            for b in &used[i + 1..] {
                let (short, long) = if a.len <= b.len { (a, b) } else { (b, a) };
                assert_ne!(long.bits >> (long.len - short.len), short.bits);
            }
        }
    }
}
