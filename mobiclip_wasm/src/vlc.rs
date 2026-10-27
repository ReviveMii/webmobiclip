pub struct VlcTable {
    table: Vec<(u16, u8)>,
    max_bits: u32,
}

impl VlcTable {
    pub fn build(lens: &[u8], syms: &[u8]) -> Self {
        let mut syms_u16 = Vec::with_capacity(syms.len());
        for &s in syms {
            syms_u16.push(s as u16);
        }
        Self::build16(lens, &syms_u16)
    }

    pub fn build16(lens: &[u8], syms: &[u16]) -> Self {
        let nb_codes = lens.len();
        let max_len = lens.iter().cloned().max().unwrap_or(0).max(1);
        let max_bits = max_len as u32;
        let size = 1usize << max_bits;
        let mut table = vec![(0u16, 0u8); size];

        let mut code: u64 = 0;
        for i in 0..nb_codes {
            let len = lens[i];
            if len > 0 {
                let symbol = if !syms.is_empty() { syms[i] } else { i as u16 };
                let c_val = (code >> (32 - len as u32)) as u32;
                let shift = max_bits - len as u32;
                let base = (c_val as usize) << shift;
                let fill_count = 1usize << shift;
                for f in 0..fill_count {
                    table[base + f] = (symbol, len);
                }

                code += 1u64 << (32 - len as u32);
            }
        }

        Self { table, max_bits }
    }

    #[inline]
    pub fn get_symbol(&self, br: &mut crate::bitreader::BitReader) -> u32 {
        let peek = br.peek_bits(self.max_bits as usize) as usize;
        let (symbol, bits) = self.table[peek];
        if bits == 0 {
            br.skip_bits(1);
            return 0;
        }
        br.skip_bits(bits as usize);
        symbol as u32
    }
}
