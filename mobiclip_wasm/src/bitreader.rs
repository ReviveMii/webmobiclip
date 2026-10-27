pub struct BitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, bit_pos: 0 }
    }

    pub fn bits_left(&self) -> isize {
        (self.data.len() * 8) as isize - self.bit_pos as isize
    }

    pub fn bit_pos(&self) -> usize {
        self.bit_pos
    }

    pub fn get_bits1(&mut self) -> u32 {
        let byte_idx = self.bit_pos >> 3;
        if byte_idx >= self.data.len() {
            return 0;
        }
        let bit_in_byte = 7 - (self.bit_pos & 7);
        self.bit_pos += 1;
        ((self.data[byte_idx] >> bit_in_byte) & 1) as u32
    }

    pub fn get_bits(&mut self, n: usize) -> u32 {
        if n == 0 {
            return 0;
        }
        let mut res = 0u32;
        for _ in 0..n {
            res = (res << 1) | self.get_bits1();
        }
        res
    }

    pub fn get_sbits(&mut self, n: usize) -> i32 {
        let u = self.get_bits(n);
        let sign_bit = 1 << (n - 1);
        if (u & sign_bit) != 0 {
            (u as i32) - (1 << n)
        } else {
            u as i32
        }
    }

    pub fn peek_bits(&self, n: usize) -> u32 {
        let mut save_pos = self.bit_pos;
        let mut res = 0u32;
        for _ in 0..n {
            let byte_idx = save_pos >> 3;
            if byte_idx < self.data.len() {
                let bit_in_byte = 7 - (save_pos & 7);
                res = (res << 1) | (((self.data[byte_idx] >> bit_in_byte) & 1) as u32);
            } else {
                res <<= 1;
            }
            save_pos += 1;
        }
        res
    }

    pub fn skip_bits(&mut self, n: usize) {
        self.bit_pos += n;
    }

    pub fn get_ue_golomb(&mut self) -> u32 {
        let mut zeros = 0;
        while self.get_bits1() == 0 && zeros < 32 {
            zeros += 1;
        }
        if zeros == 0 {
            0
        } else if zeros >= 32 {
            0
        } else {
            let info = self.get_bits(zeros);
            (1 << zeros) - 1 + info
        }
    }

    pub fn get_se_golomb(&mut self) -> i32 {
        let ue = self.get_ue_golomb();
        if (ue & 1) != 0 {
            ((ue + 1) >> 1) as i32
        } else {
            -((ue >> 1) as i32)
        }
    }

    pub fn get_ue_golomb_31(&mut self) -> u32 {
        self.get_ue_golomb()
    }
}
