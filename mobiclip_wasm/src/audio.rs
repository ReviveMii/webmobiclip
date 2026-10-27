const IMA_INDEX_TABLE: [i8; 16] = [
    -1, -1, -1, -1, 2, 4, 6, 8,
    -1, -1, -1, -1, 2, 4, 6, 8,
];

const IMA_STEP_TABLE: [i16; 89] = [
        7,     8,     9,    10,    11,    12,    13,    14,    16,    17,
       19,    21,    23,    25,    28,    31,    34,    37,    41,    45,
       50,    55,    60,    66,    73,    80,    88,    97,   107,   118,
      130,   143,   157,   173,   190,   209,   230,   253,   279,   307,
      337,   371,   408,   449,   494,   544,   598,   658,   724,   796,
      876,   963,  1060,  1166,  1282,  1411,  1552,  1707,  1878,  2066,
     2272,  2499,  2749,  3024,  3327,  3660,  4026,  4428,  4871,  5358,
     5894,  6484,  7132,  7845,  8630,  9493, 10442, 11487, 12635, 13899,
    15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767
];

#[derive(Default, Clone, Copy)]
pub struct AdpcmChannelStatus {
    pub predictor: i32,
    pub step_index: i16,
}

impl AdpcmChannelStatus {
    pub fn expand_nibble(&mut self, nibble: u8) -> i16 {
        let step = IMA_STEP_TABLE[self.step_index as usize] as i32;
        let mut next_index = self.step_index as i32 + IMA_INDEX_TABLE[(nibble & 0xF) as usize] as i32;
        next_index = next_index.clamp(0, 88);

        let sign = (nibble & 8) != 0;
        let delta = (nibble & 7) as i32;

        let diff = ((2 * delta + 1) * step) >> 3;
        let predictor = if sign {
            self.predictor - diff
        } else {
            self.predictor + diff
        };

        self.predictor = predictor.clamp(-32768, 32767);
        self.step_index = next_index as i16;
        self.predictor as i16
    }
}

pub struct AdpcmDecoder {
    pub channels: usize,
    status: Vec<AdpcmChannelStatus>,
}

impl AdpcmDecoder {
    pub fn new(channels: usize) -> Self {
        Self {
            channels,
            status: vec![AdpcmChannelStatus::default(); channels],
        }
    }

    pub fn decode_moflex(&mut self, data: &[u8]) -> Vec<f32> {
        if self.channels == 0 || data.len() < self.channels * 4 {
            return Vec::new();
        }

        let mut read_idx = 0;
        for ch in 0..self.channels {
            let step_index = i16::from_le_bytes([data[read_idx], data[read_idx + 1]]);
            let predictor = i16::from_le_bytes([data[read_idx + 2], data[read_idx + 3]]);
            read_idx += 4;
            self.status[ch].step_index = step_index.clamp(0, 88);
            self.status[ch].predictor = predictor as i32;
        }

        let subframes = (data.len() - read_idx) / (128 * self.channels);
        let nb_samples = subframes * 256;
        let mut out = vec![0.0f32; nb_samples * self.channels];

        for sf in 0..subframes {
            for ch in 0..self.channels {
                for n in (0..256).step_by(2) {
                    if read_idx >= data.len() {
                        break;
                    }
                    let b = data[read_idx];
                    read_idx += 1;
                    let s0 = self.status[ch].expand_nibble(b & 0x0F);
                    let s1 = self.status[ch].expand_nibble(b >> 4);

                    let out_idx0 = (sf * 256 + n) * self.channels + ch;
                    let out_idx1 = (sf * 256 + n + 1) * self.channels + ch;
                    if out_idx1 < out.len() {
                        out[out_idx0] = s0 as f32 / 32768.0;
                        out[out_idx1] = s1 as f32 / 32768.0;
                    }
                }
            }
        }

        out
    }

    pub fn decode_wii(&mut self, data: &[u8]) -> Vec<f32> {
        let block_size = self.channels * 132;
        if block_size == 0 {
            return Vec::new();
        }
        let subframes = data.len() / block_size;
        let mut out = vec![0.0f32; subframes * 256 * self.channels];
        let mut read_idx = 0;

        for sf in 0..subframes {
            for ch in 0..self.channels {
                if read_idx + 4 > data.len() {
                    break;
                }
                let step_index = i16::from_le_bytes([data[read_idx], data[read_idx + 1]]);
                let predictor = i16::from_le_bytes([data[read_idx + 2], data[read_idx + 3]]);
                read_idx += 4;
                self.status[ch].step_index = step_index.clamp(0, 88);
                self.status[ch].predictor = predictor as i32;

                for n in (0..256).step_by(2) {
                    if read_idx >= data.len() {
                        break;
                    }
                    let b = data[read_idx];
                    read_idx += 1;
                    let s0 = self.status[ch].expand_nibble(b & 0x0F);
                    let s1 = self.status[ch].expand_nibble(b >> 4);

                    let out_idx0 = (sf * 256 + n) * self.channels + ch;
                    let out_idx1 = (sf * 256 + n + 1) * self.channels + ch;
                    if out_idx1 < out.len() {
                        out[out_idx0] = s0 as f32 / 32768.0;
                        out[out_idx1] = s1 as f32 / 32768.0;
                    }
                }
            }
        }

        out
    }

    pub fn decode_nds(&mut self, data: &[u8], is_key: bool) -> Vec<f32> {
        let mut read_idx = 0;
        if is_key {
            if data.len() < 4 + 4 * self.channels {
                return Vec::new();
            }
            read_idx += 4;
            for ch in 0..self.channels {
                let step_index = i16::from_le_bytes([data[read_idx], data[read_idx + 1]]);
                let predictor = i16::from_le_bytes([data[read_idx + 2], data[read_idx + 3]]);
                read_idx += 4;
                self.status[ch].step_index = step_index.clamp(0, 88);
                self.status[ch].predictor = predictor as i32;
            }
        }

        let block = self.channels * 128;
        if block == 0 || data.len() < read_idx + block {
            return Vec::new();
        }
        let blocks = (data.len() - read_idx) / block;
        let mut out = vec![0.0f32; blocks * 256 * self.channels];

        for blk in 0..blocks {
            for ch in 0..self.channels {
                for n in (0..256).step_by(2) {
                    let b = data[read_idx];
                    read_idx += 1;
                    let s0 = self.status[ch].expand_nibble(b & 0x0F);
                    let s1 = self.status[ch].expand_nibble(b >> 4);

                    let out_idx0 = (blk * 256 + n) * self.channels + ch;
                    let out_idx1 = (blk * 256 + n + 1) * self.channels + ch;
                    out[out_idx0] = s0 as f32 / 32768.0;
                    out[out_idx1] = s1 as f32 / 32768.0;
                }
            }
        }

        out
    }
}

pub struct DspAdpcmDecoder {
    pub channels: usize,
}

impl DspAdpcmDecoder {
    pub fn new(channels: usize) -> Self {
        Self { channels }
    }

    pub fn decode(&mut self, data: &[u8]) -> Vec<f32> {
        if self.channels == 0 {
            return Vec::new();
        }
        let size = data.len() / self.channels;
        if size <= 38 {
            return Vec::new();
        }

        let mut per_channel: Vec<Vec<i16>> = Vec::with_capacity(self.channels);
        let mut min_len = usize::MAX;

        for ch in 0..self.channels {
            let base = size * ch;
            let mut coefs = [0i16; 19];
            for j in 0..19 {
                coefs[j] = i16::from_le_bytes([data[base + 2 * j], data[base + 2 * j + 1]]);
            }
            let (mut h1, mut h2) = (coefs[17] as i32, coefs[18] as i32);
            let (mut scale, mut c1, mut c2) = (0i32, 0i32, 0i32);
            let mut out = Vec::with_capacity(size - 38);

            for (i, &b) in data[base + 38..base + size].iter().enumerate() {
                if i % 8 == 0 {
                    scale = 1 << (b & 0xF);
                    c1 = coefs[(b as usize / 16) * 2] as i32;
                    c2 = coefs[(b as usize / 16) * 2 + 1] as i32;
                } else {
                    for nibble in [b >> 4, b & 0xF] {
                        let pred = (1024 + c1 * h1 + c2 * h2) >> 11;
                        let sample = (((nibble ^ 8) as i32 - 8) * scale + pred).clamp(-32768, 32767);
                        h2 = h1;
                        h1 = sample;
                        out.push(sample as i16);
                    }
                }
            }

            out.drain(0..out.len().min(32));
            min_len = min_len.min(out.len());
            per_channel.push(out);
        }

        let mut result = vec![0.0f32; min_len * self.channels];
        for i in 0..min_len {
            for (ch, samples) in per_channel.iter().enumerate() {
                result[i * self.channels + ch] = samples[i] as f32 / 32768.0;
            }
        }
        result
    }
}

const PULSE_DATA_LEN: [usize; 4] = [8, 5, 4, 3];
const PULSE_DISTANCE: [i32; 4] = [3, 3, 4, 5];

#[derive(Clone)]
pub struct VxAudioExtra {
    lpc_codebooks: [[[i16; 8]; 64]; 3],
    scale_modifiers: [u16; 8],
    lpc_base: [i32; 8],
    scale_initial: u32,
}

impl VxAudioExtra {
    fn parse(p: &[u8]) -> Self {
        let mut idx = 0;
        let mut lpc_codebooks = [[[0i16; 8]; 64]; 3];
        for i in 0..3 {
            for j in 0..64 {
                for k in 0..8 {
                    lpc_codebooks[i][j][k] = i16::from_le_bytes([p[idx], p[idx + 1]]);
                    idx += 2;
                }
            }
        }
        let mut scale_modifiers = [0u16; 8];
        for i in 0..8 {
            scale_modifiers[i] = u16::from_le_bytes([p[idx], p[idx + 1]]);
            idx += 2;
        }
        let mut lpc_base = [0i32; 8];
        for i in 0..8 {
            lpc_base[i] = i32::from_le_bytes(p[idx..idx + 4].try_into().unwrap());
            idx += 4;
        }
        let scale_initial = u32::from_le_bytes(p[idx..idx + 4].try_into().unwrap());
        Self { lpc_codebooks, scale_modifiers, lpc_base, scale_initial }
    }
}

struct VxChannelState {
    have_prev: bool,
    have_prev2: bool,
    pulses_prev: [i32; 128],
    pulses_prev2: [i32; 128],
    samples_prev: [i32; 8],
    scale_prev: i32,
    lpc_filter_prev: [i32; 8],
    influence_prev: [i32; 8],
}

impl VxChannelState {
    fn new() -> Self {
        Self {
            have_prev: false,
            have_prev2: false,
            pulses_prev: [0; 128],
            pulses_prev2: [0; 128],
            samples_prev: [0; 8],
            scale_prev: 0,
            lpc_filter_prev: [0; 8],
            influence_prev: [0; 8],
        }
    }

    fn decode_aframe(&mut self, ed: &VxAudioExtra, data: &[u8], word_idx: &mut usize) -> Option<[i16; 128]> {
        let read_u16 = |data: &[u8], word_idx: &mut usize| -> Option<u16> {
            let byte_idx = *word_idx * 2;
            if byte_idx + 2 > data.len() {
                return None;
            }
            *word_idx += 1;
            Some(u16::from_le_bytes([data[byte_idx], data[byte_idx + 1]]))
        };

        let word1 = read_u16(data, word_idx)? as i32;
        let word2 = read_u16(data, word_idx)? as i32;

        let prev_frame_offset = (word1 >> 9) & 0x7f;
        let scale_modifier_index = ((word1 >> 6) & 0x7) as usize;
        let pulse_start_position = (word2 >> 14) & 0x3;
        let pulse_packing_mode = ((word2 >> 12) & 0x3) as usize;
        let lpc_idx = [
            (word1 & 0x3f) as usize,
            ((word2 >> 6) & 0x3f) as usize,
            (word2 & 0x3f) as usize,
        ];

        let pdl = PULSE_DATA_LEN[pulse_packing_mode];
        let mut pulse_data = [0u16; 8];
        for i in 0..pdl {
            pulse_data[i] = read_u16(data, word_idx)?;
        }
        let (pulse_values, pulse_qty) = unpack_pulse_values(pulse_packing_mode, &pulse_data);

        if prev_frame_offset != 0x7f && !self.have_prev {
            return None;
        }

        let mut scale = if self.have_prev { self.scale_prev } else { ed.scale_initial as i32 };
        if prev_frame_offset == 0x7f {
            scale = ed.scale_initial as i32;
        }
        let scale64 = (scale as i64 * ed.scale_modifiers[scale_modifier_index] as i64) >> 13;
        scale = scale64.clamp(-(i32::MAX as i64 / 16), i32::MAX as i64 / 16) as i32;

        let mut lpc_filter = if prev_frame_offset == 0x7f { ed.lpc_base } else { self.lpc_filter_prev };
        for k in 0..8 {
            let mut v = lpc_filter[k] as i64;
            for i in 0..3 {
                v += ed.lpc_codebooks[i][lpc_idx[i]][k] as i64;
            }
            if v < i32::MIN as i64 || v > i32::MAX as i64 {
                return None;
            }
            lpc_filter[k] = v as i32;
        }

        let distance = PULSE_DISTANCE[pulse_packing_mode];

        let mut pulses = [0i32; 128];
        if prev_frame_offset < 0x7e {
            let mut concat = [0i32; 256];
            if self.have_prev2 {
                concat[0..128].copy_from_slice(&self.pulses_prev2);
            }
            if self.have_prev {
                concat[128..256].copy_from_slice(&self.pulses_prev);
            }
            for i in 0..128 {
                let volume = (i as i32 + 1).min(128 - i as i32).min(8);
                let idx = (i as i32 + 0x7f - prev_frame_offset) as usize;
                pulses[i] = (concat[idx] * volume) >> 4;
            }
        }

        for i in 0..128 {
            let diff = i as i32 - pulse_start_position;
            if diff >= 0 && diff % distance == 0 {
                let idx = (diff / distance) as usize;
                if idx < pulse_qty {
                    pulses[i] += pulse_values[idx] * scale;
                }
            }
        }

        let mut influence = [0i32; 8];
        {
            let mut psi = [0i32; 8];
            for i in 0..8 {
                let coeff = lpc_filter[i];
                let old = psi;
                for j in 0..i {
                    let v = old[j] as i64 + ((old[i - j - 1] as i64 * coeff as i64) >> 15);
                    if v < i32::MIN as i64 || v > i32::MAX as i64 {
                        return None;
                    }
                    psi[j] = v as i32;
                }
                psi[i] = coeff;
            }
            for i in 0..8 {
                influence[i] = -(psi[i] >> 1);
            }
        }

        let mut quarters = [[0i32; 8]; 4];
        if prev_frame_offset != 0x7f {
            quarters[3] = influence;
            for j in 0..8 {
                quarters[1][j] = (self.influence_prev[j] + quarters[3][j]) >> 1;
            }
            for j in 0..8 {
                quarters[0][j] = (self.influence_prev[j] + quarters[1][j]) >> 1;
            }
            for j in 0..8 {
                quarters[2][j] = (quarters[1][j] + quarters[3][j]) >> 1;
            }
        } else {
            for q in 0..4 {
                quarters[q] = influence;
            }
        }

        let mut samples = [0i32; 128];
        for i in 0..128 {
            let inf = &quarters[i * 4 / 128];
            let mut sample = pulses[i] as i64 * 0x4000;
            for j in 0..8 {
                let si = i as i32 - 1 - j as i32;
                let prev_sample = if si < 0 { self.samples_prev[(8 + si) as usize] } else { samples[si as usize] };
                sample += prev_sample as i64 * inf[j] as i64;
            }
            sample >>= 14;
            if sample < i32::MIN as i64 || sample > i32::MAX as i64 {
                return None;
            }
            samples[i] = sample as i32;
        }

        let mut out = [0i16; 128];
        for i in 0..128 {
            out[i] = samples[i].clamp(-32768, 32767) as i16;
        }

        self.pulses_prev2 = self.pulses_prev;
        self.have_prev2 = self.have_prev;
        self.pulses_prev = pulses;
        self.samples_prev.copy_from_slice(&samples[120..128]);
        self.scale_prev = scale;
        self.lpc_filter_prev = lpc_filter;
        self.influence_prev = influence;
        self.have_prev = true;

        Some(out)
    }
}

fn unpack_pulse_values(mode: usize, pulse_data: &[u16; 8]) -> ([i32; 42], usize) {
    let mut out = [0i32; 42];
    let mut n = 0;
    if mode == 0 {
        for i in 0..8 {
            let mut j: i32 = 13;
            while j >= 0 {
                out[n] = ((pulse_data[i] >> j) & 0x7) as i32;
                n += 1;
                j -= 3;
            }
        }
        out[n] = (((pulse_data[0] & 1) << 2) | ((pulse_data[1] & 1) << 1) | (pulse_data[2] & 1)) as i32;
        n += 1;
        out[n] = (((pulse_data[3] & 1) << 2) | ((pulse_data[4] & 1) << 1) | (pulse_data[5] & 1)) as i32;
        n += 1;
        for v in out.iter_mut().take(n) {
            *v = *v * 2 - 7;
        }
    } else {
        let words = PULSE_DATA_LEN[mode];
        for i in 0..words {
            let mut j: i32 = 14;
            while j >= 0 {
                out[n] = ((pulse_data[i] >> j) & 0x3) as i32;
                n += 1;
                j -= 2;
            }
        }
        for v in out.iter_mut().take(n) {
            *v = *v * 2 - 3;
        }
    }
    (out, n)
}

pub struct VxAudioDecoder {
    channels: usize,
    ed: Vec<VxAudioExtra>,
    state: Vec<VxChannelState>,
}

impl VxAudioDecoder {
    pub fn new(channels: usize, codebook: &[u8]) -> Option<Self> {
        if channels == 0 || codebook.len() < channels * 0xC34 {
            return None;
        }
        let ed = (0..channels)
            .map(|c| VxAudioExtra::parse(&codebook[c * 0xC34..(c + 1) * 0xC34]))
            .collect();
        let state = (0..channels).map(|_| VxChannelState::new()).collect();
        Some(Self { channels, ed, state })
    }

    pub fn decode(&mut self, pkt: &[u8]) -> Vec<f32> {
        if pkt.len() < 4 {
            return Vec::new();
        }
        let nb_aframes = u32::from_le_bytes(pkt[0..4].try_into().unwrap()) as usize;
        if nb_aframes == 0 || nb_aframes % self.channels != 0 {
            return Vec::new();
        }

        let bitstream = &pkt[4..];
        let mut word_idx = 0usize;
        let blocks = nb_aframes / self.channels;
        let mut out = vec![0.0f32; blocks * 128 * self.channels];

        for i in 0..nb_aframes {
            let c = i % self.channels;
            let b = i / self.channels;
            match self.state[c].decode_aframe(&self.ed[c], bitstream, &mut word_idx) {
                Some(samples) => {
                    for (s, sample) in samples.iter().enumerate() {
                        out[(b * 128 + s) * self.channels + c] = *sample as f32 / 32768.0;
                    }
                }
                None => return out,
            }
        }

        out
    }
}

pub struct FastAudioChannel {
    pub f: [f32; 8],
    pub last: f32,
}

pub struct FastAudioDecoder {
    pub channels: usize,
    table: [[f32; 64]; 8],
    ch_state: Vec<FastAudioChannel>,
}

impl FastAudioDecoder {
    pub fn new(channels: usize) -> Self {
        let mut table = [[0.0f32; 64]; 8];

        for i in 0..8 {
            table[0][i] = (i as f32 - 159.5) / 160.0;
        }
        for i in 0..11 {
            table[0][i + 8] = (i as f32 - 37.5) / 40.0;
        }
        for i in 0..27 {
            table[0][i + 8 + 11] = (i as f32 - 13.0) / 20.0;
        }
        for i in 0..11 {
            table[0][i + 8 + 11 + 27] = (i as f32 + 27.5) / 40.0;
        }
        for i in 0..7 {
            table[0][i + 8 + 11 + 27 + 11] = (i as f32 + 152.5) / 160.0;
        }

        table[1] = table[0];

        for i in 0..7 {
            table[2][i] = (i as f32 - 33.5) / 40.0;
        }
        for i in 0..25 {
            table[2][i + 7] = (i as f32 - 13.0) / 20.0;
        }

        for i in 0..32 {
            table[3][i] = -table[2][31 - i];
        }

        for i in 0..16 {
            table[4][i] = i as f32 * 0.22 / 3.0 - 0.6;
            table[5][i] = i as f32 * 0.20 / 3.0 - 0.3;
        }

        for i in 0..8 {
            table[6][i] = i as f32 * 0.36 / 3.0 - 0.4;
            table[7][i] = i as f32 * 0.34 / 3.0 - 0.2;
        }

        let ch_state = (0..channels)
            .map(|_| FastAudioChannel {
                f: [0.0; 8],
                last: 0.0,
            })
            .collect();

        Self {
            channels,
            table,
            ch_state,
        }
    }

    fn read_bits(bits: usize, pos: &mut usize, src: &[u32]) -> u32 {
        let p = *pos + bits;
        let r = src[(p - 1) / 32] >> ((- (p as isize)) & 31);
        *pos = p;
        r & ((1 << bits) - 1)
    }

    pub fn decode(&mut self, pkt: &[u8]) -> Vec<f32> {
        let subframes = pkt.len() / (40 * self.channels);
        if subframes == 0 {
            return Vec::new();
        }

        let total_samples = subframes * 256;
        let mut out = vec![0.0f32; total_samples * self.channels];
        let bits_tab: [usize; 8] = [6, 6, 5, 5, 4, 0, 3, 3];

        let mut byte_offset = 0;
        for sf in 0..subframes {
            for ch in 0..self.channels {
                let mut src = [0u32; 10];
                for i in 0..10 {
                    if byte_offset + 4 <= pkt.len() {
                        src[i] = u32::from_le_bytes([
                            pkt[byte_offset],
                            pkt[byte_offset + 1],
                            pkt[byte_offset + 2],
                            pkt[byte_offset + 3],
                        ]);
                        byte_offset += 4;
                    }
                }

                let mut pos = 0usize;
                let mut m = [0.0f32; 8];
                for i in 0..8 {
                    let b = Self::read_bits(bits_tab[i], &mut pos, &src);
                    m[7 - i] = self.table[i][b as usize];
                }

                let mut inds = [0u32; 4];
                for i in 0..4 {
                    inds[3 - i] = Self::read_bits(6, &mut pos, &src);
                }

                let mut pads = [0usize; 4];
                for i in 0..4 {
                    pads[3 - i] = Self::read_bits(2, &mut pos, &src) as usize;
                }

                let mut result = [0.0f32; 256];
                let mut index5 = 0usize;

                for i in 0..4 {
                    let value = f32::from_bits((inds[i] + 1) << 20) * 2.0f32.powf(116.0);
                    let mut tmp = 0u32;

                    for j in 0..21 {
                        let v = if j == 20 {
                            tmp / 2
                        } else {
                            Self::read_bits(3, &mut pos, &src)
                        };
                        result[i * 64 + pads[i] + j * 3] = value * (2.0 * v as f32 - 7.0);

                        if j % 10 == 9 {
                            tmp = 4 * tmp + Self::read_bits(2, &mut pos, &src);
                        }
                        if j == 20 {
                            index5 = (2 * index5 + (tmp % 2) as usize).min(63);
                        }
                    }

                    m[2] = self.table[5][index5];
                }

                let state = &mut self.ch_state[ch];
                for i in 0..256 {
                    let mut x = result[i];
                    for j in 0..8 {
                        x -= m[j] * state.f[j];
                        state.f[j] += m[j] * x;
                    }

                    for j in 0..7 {
                        state.f[j] = state.f[j + 1];
                    }
                    state.f[7] = x;
                    state.last = x + state.last * 0.86;
                    out[(sf * 256 + i) * self.channels + ch] = state.last * 2.0;
                }
            }
        }

        out
    }
}
