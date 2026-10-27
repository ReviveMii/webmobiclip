use crate::bitreader::BitReader;
use crate::tables::*;
use crate::vlc::VlcTable;

#[derive(Clone, Copy, Default)]
pub struct MotionXY {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone)]
pub struct YuvFrame {
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    pub y_stride: usize,
    pub uv_stride: usize,
    pub width: usize,
    pub height: usize,
}

impl Default for YuvFrame {
    fn default() -> Self {
        Self {
            y: Vec::new(),
            u: Vec::new(),
            v: Vec::new(),
            y_stride: 0,
            uv_stride: 0,
            width: 0,
            height: 0,
        }
    }
}

impl YuvFrame {
    pub fn new(width: usize, height: usize) -> Self {
        let y_stride = width;
        let uv_stride = width / 2;
        Self {
            y: vec![0u8; y_stride * height],
            u: vec![128u8; uv_stride * (height / 2)],
            v: vec![128u8; uv_stride * (height / 2)],
            y_stride,
            uv_stride,
            width,
            height,
        }
    }

    pub fn plane_mut(&mut self, plane: usize) -> (&mut [u8], usize) {
        match plane {
            0 => (&mut self.y, self.y_stride),
            1 => (&mut self.u, self.uv_stride),
            _ => (&mut self.v, self.uv_stride),
        }
    }

    pub fn plane(&self, plane: usize) -> (&[u8], usize) {
        match plane {
            0 => (&self.y, self.y_stride),
            1 => (&self.u, self.uv_stride),
            _ => (&self.v, self.uv_stride),
        }
    }
}

pub struct MobiClipDecoder {
    pub width: usize,
    pub height: usize,
    pics: Vec<YuvFrame>,
    current_pic: usize,
    moflex: bool,
    dct_tab_idx: usize,
    quantizer: i32,
    qtab_4x4: [i32; 16],
    qtab_8x8: [i32; 64],
    pre: [u8; 32],
    motion: Vec<MotionXY>,
    rl_vlc: [VlcTable; 2],
    mv_vlc: [[VlcTable; 16]; 2],
    last_consumed_bytes: usize,
}

#[derive(Clone, Copy)]
struct BlockXY<'a> {
    w: i32,
    h: i32,
    ax: i32,
    ay: i32,
    x: i32,
    y: i32,
    size: i32,
    block: &'a [u8],
    linesize: usize,
}

fn pget(b: BlockXY) -> u8 {
    let mut rx = b.x;
    let mut ry = b.y;

    if b.x == -1 && b.y >= b.size {
        rx = -1;
        ry = b.size - 1;
    } else if b.x >= -1 && b.y >= -1 {
    } else if b.x == -1 && b.y == -2 {
        rx = 0;
        ry = -1;
    } else if b.x == -2 && b.y == -1 {
        rx = -1;
        ry = 0;
    }

    let y = (b.ay + ry).clamp(0, b.h - 1) as usize;
    let x = (b.ax + rx).clamp(0, b.w - 1) as usize;

    b.block[y * b.linesize + x]
}

fn half(a: i32, b: i32) -> u8 {
    (((a + b) + 1) / 2) as u8
}

fn half3(a: i32, b: i32, c: i32) -> u8 {
    (((a + b + b + c) * 2 / 4 + 1) / 2) as u8
}

fn pick_above(mut bxy: BlockXY) -> u8 {
    bxy.y -= 1;
    pget(bxy)
}

fn pick_left(mut bxy: BlockXY) -> u8 {
    bxy.x -= 1;
    pget(bxy)
}

fn half_horz(bxy: BlockXY) -> u8 {
    let mut a = bxy;
    let mut c = bxy;
    a.x -= 1;
    c.x += 1;
    half3(pget(a) as i32, pget(bxy) as i32, pget(c) as i32)
}

fn half_vert(bxy: BlockXY) -> u8 {
    let mut a = bxy;
    let mut c = bxy;
    a.y -= 1;
    c.y += 1;
    half3(pget(a) as i32, pget(bxy) as i32, pget(c) as i32)
}

fn pick_4(bxy: BlockXY) -> u8 {
    if (bxy.x % 2) == 0 {
        let mut ba = bxy;
        ba.x = -1;
        ba.y = bxy.y + bxy.x / 2;
        let a = pget(ba) as i32;

        let mut bb = bxy;
        bb.x = -1;
        bb.y = bxy.y + bxy.x / 2 + 1;
        let b = pget(bb) as i32;

        half(a, b)
    } else {
        let mut ba = bxy;
        ba.x = -1;
        ba.y = bxy.y + bxy.x / 2 + 1;
        half_vert(ba)
    }
}

fn pick_5(bxy: BlockXY) -> u8 {
    if bxy.x == 0 {
        let mut a = bxy;
        let mut b = bxy;
        a.x = -1;
        a.y -= 1;
        b.x = -1;
        half(pget(a) as i32, pget(b) as i32)
    } else if bxy.y == 0 {
        let mut a = bxy;
        a.x -= 2;
        a.y -= 1;
        half_horz(a)
    } else if bxy.x == 1 {
        let mut a = bxy;
        a.x -= 2;
        a.y -= 1;
        half_vert(a)
    } else {
        let mut a = bxy;
        a.x -= 2;
        a.y -= 1;
        pget(a)
    }
}

fn pick_6(bxy: BlockXY) -> u8 {
    if bxy.y == 0 {
        let mut a = bxy;
        let mut b = bxy;
        a.x -= 1;
        a.y = -1;
        b.y = -1;
        half(pget(a) as i32, pget(b) as i32)
    } else if bxy.x == 0 {
        let mut a = bxy;
        a.x -= 1;
        a.y -= 2;
        half_vert(a)
    } else if bxy.y == 1 {
        let mut a = bxy;
        a.x -= 1;
        a.y -= 2;
        half_horz(a)
    } else {
        let mut a = bxy;
        a.x -= 1;
        a.y -= 2;
        pget(a)
    }
}

fn pick_7(bxy: BlockXY) -> u8 {
    let mut a = bxy;
    a.x -= 1;
    a.y -= 1;
    let clr = pget(a) as i32;
    if bxy.x != 0 && bxy.y != 0 {
        return clr as u8;
    }

    if bxy.x == 0 {
        a.x = -1;
        a.y = bxy.y;
    } else {
        a.x = bxy.x - 2;
        a.y = -1;
    }
    let acc1 = pget(a) as i32;

    if bxy.y == 0 {
        a.x = bxy.x;
        a.y = -1;
    } else {
        a.x = -1;
        a.y = bxy.y - 2;
    }
    let acc2 = pget(a) as i32;

    half3(acc1, clr, acc2)
}

fn pick_8(bxy: BlockXY) -> u8 {
    let mut ba = bxy;
    let mut bb = bxy;
    if bxy.y == 0 {
        ba.y = -1;
        let a = pget(ba) as i32;
        bb.x += 1;
        bb.y = -1;
        let b = pget(bb) as i32;
        half(a, b)
    } else if bxy.y == 1 {
        ba.x += 1;
        ba.y -= 2;
        half_horz(ba)
    } else if bxy.x < bxy.size - 1 {
        ba.x += 1;
        ba.y -= 2;
        pget(ba)
    } else if (bxy.y % 2) == 0 {
        ba.x = bxy.y / 2 + bxy.size - 1;
        ba.y = -1;
        let a = pget(ba) as i32;
        bb.x = bxy.y / 2 + bxy.size;
        bb.y = -1;
        let b = pget(bb) as i32;
        half(a, b)
    } else {
        ba.x = bxy.y / 2 + bxy.size;
        ba.y = -1;
        half_horz(ba)
    }
}

fn inverse4(rs: &mut [i32]) {
    let a = rs[0].wrapping_add(rs[2]);
    let b = rs[0].wrapping_sub(rs[2]);
    let c = rs[1].wrapping_add(rs[3] >> 1);
    let d = (rs[1] >> 1).wrapping_sub(rs[3]);

    rs[0] = a.wrapping_add(c);
    rs[1] = b.wrapping_add(d);
    rs[2] = b.wrapping_sub(d);
    rs[3] = a.wrapping_sub(c);
}

fn idct(arr: &mut [i32], size: usize) {
    if size == 4 {
        inverse4(arr);
        return;
    }

    let mut tmp = [arr[0], arr[2], arr[4], arr[6]];
    inverse4(&mut tmp);

    let e = arr[7].wrapping_add(arr[1]).wrapping_sub(arr[3]).wrapping_sub(arr[3] >> 1);
    let f = arr[7].wrapping_sub(arr[1]).wrapping_add(arr[5]).wrapping_add(arr[5] >> 1);
    let g = arr[5].wrapping_sub(arr[3]).wrapping_sub(arr[7]).wrapping_sub(arr[7] >> 1);
    let h = arr[5].wrapping_add(arr[3]).wrapping_add(arr[1]).wrapping_add(arr[1] >> 1);
    let x3 = g.wrapping_add(h >> 2);
    let x2 = e.wrapping_add(f >> 2);
    let x1 = (e >> 2).wrapping_sub(f);
    let x0 = h.wrapping_sub(g >> 2);

    arr[0] = tmp[0].wrapping_add(x0);
    arr[1] = tmp[1].wrapping_add(x1);
    arr[2] = tmp[2].wrapping_add(x2);
    arr[3] = tmp[3].wrapping_add(x3);
    arr[4] = tmp[3].wrapping_sub(x3);
    arr[5] = tmp[2].wrapping_sub(x2);
    arr[6] = tmp[1].wrapping_sub(x1);
    arr[7] = tmp[0].wrapping_sub(x0);
}

fn adjust(x: i32, size: i32) -> i32 {
    if size == 16 {
        (x + 1) >> 1
    } else {
        x
    }
}

fn mid_pred(a: i32, b: i32, c: i32) -> i32 {
    let mut min = a;
    let mut max = b;
    if min > max {
        std::mem::swap(&mut min, &mut max);
    }
    if c < min {
        min
    } else if c > max {
        max
    } else {
        c
    }
}

fn get_index(x: i32) -> usize {
    match x {
        16 => 0,
        8 => 1,
        4 => 2,
        2 => 3,
        _ => 0,
    }
}

impl MobiClipDecoder {
    pub fn new(width: usize, height: usize) -> Result<Self, String> {
        if width % 16 != 0 || height % 16 != 0 {
            return Err("Resolution must be multiple of 16".into());
        }

        let rl_vlc0 = VlcTable::build16(&BITS0, &SYMS0);
        let rl_vlc1 = VlcTable::build16(&BITS0, &SYMS1);

        let mv_vlc: [[VlcTable; 16]; 2] = [
            std::array::from_fn(|i| VlcTable::build(&MV_BITS[0][i][..MV_LEN[i] as usize], &MV_SYMS[0][i][..MV_LEN[i] as usize])),
            std::array::from_fn(|i| VlcTable::build(&MV_BITS[1][i][..MV_LEN[i] as usize], &MV_SYMS[1][i][..MV_LEN[i] as usize])),
        ];

        let motion_len = width / 16 + 3;
        let pics = (0..6).map(|_| YuvFrame::new(width, height)).collect();

        Ok(Self {
            width,
            height,
            pics,
            current_pic: 0,
            moflex: false,
            dct_tab_idx: 0,
            quantizer: 12,
            qtab_4x4: [0; 16],
            qtab_8x8: [0; 64],
            pre: [9; 32],
            motion: vec![MotionXY::default(); motion_len],
            rl_vlc: [rl_vlc0, rl_vlc1],
            mv_vlc,
            last_consumed_bytes: 0,
        })
    }

    pub fn last_consumed_bytes(&self) -> usize {
        self.last_consumed_bytes
    }

    pub fn is_moflex(&self) -> bool {
        self.moflex
    }

    pub fn get_pic(&self, idx: usize) -> &YuvFrame {
        &self.pics[idx]
    }

    fn setup_qtables(&mut self, quantizer: i32) -> Result<(), String> {
        if quantizer < 12 || quantizer > 161 {
            return Err(format!("Invalid quantizer: {}", quantizer));
        }
        self.quantizer = quantizer;
        let qx = (quantizer % 6) as usize;
        let qy = (quantizer / 6) as usize;

        for i in 0..16 {
            self.qtab_4x4[i] = (QUANT4X4_TAB[qx][i] as i32) << qy;
        }
        for i in 0..64 {
            self.qtab_8x8[i] = (QUANT8X8_TAB[qx][i] as i32) << (qy - 2);
        }
        for i in 0..20 {
            self.pre[i] = 9;
        }
        Ok(())
    }

    fn read_run_encoding(&self, br: &mut BitReader) -> (bool, usize, i32) {
        let n = self.rl_vlc[self.dct_tab_idx].get_symbol(br);
        let last = (n >> 11) == 1;
        let run = ((n >> 5) & 0x3F) as usize;
        let level = (n & 0x1F) as i32;
        (last, run, level)
    }

    fn add_coefficients(
        &self,
        br: &mut BitReader,
        frame: &mut YuvFrame,
        bx: usize,
        by: usize,
        size: usize,
        plane: usize,
    ) -> Result<(), String> {
        let mut mat = [0i32; 64];
        let ztab: &[u8] = if size == 8 { &FF_ZIGZAG_DIRECT } else { &ZIGZAG4X4_TAB };
        let qtab: &[i32] = if size == 8 { &self.qtab_8x8 } else { &self.qtab_4x4 };

        let mut pos = 0usize;
        while br.bits_left() > 0 {
            let (mut last, mut run, mut level) = self.read_run_encoding(br);

            if level != 0 {
                if br.get_bits1() != 0 {
                    level = -level;
                }
            } else if br.get_bits1() == 0 {
                let (l, r, lev) = self.read_run_encoding(br);
                last = l;
                run = r;
                level = lev + RUN_RESIDUE[self.dct_tab_idx][(if last { 64 } else { 0 }) + run] as i32;
                if br.get_bits1() != 0 {
                    level = -level;
                }
            } else if br.get_bits1() == 0 {
                let (l, r, lev) = self.read_run_encoding(br);
                last = l;
                run = r + RUN_RESIDUE[self.dct_tab_idx][128 + (if last { 64 } else { 0 }) + lev as usize] as usize;
                level = lev;
                if br.get_bits1() != 0 {
                    level = -level;
                }
            } else {
                last = br.get_bits1() != 0;
                run = br.get_bits(6) as usize;
                level = br.get_sbits(12);
            }

            pos += run;
            if pos >= size * size {
                return Err("Residual pos out of bounds".into());
            }
            let qval = qtab[pos];
            mat[ztab[pos] as usize] = qval.wrapping_mul(level);

            if last {
                break;
            }
            pos += 1;
        }

        mat[0] = mat[0].wrapping_add(32);
        for y in 0..size {
            idct(&mut mat[y * size..(y + 1) * size], size);
        }

        for y in 0..size {
            for x in (y + 1)..size {
                let a = mat[x * size + y];
                let b = mat[y * size + x];
                mat[y * size + x] = a;
                mat[x * size + y] = b;
            }
            idct(&mut mat[y * size..(y + 1) * size], size);
        }

        let (dst, linesize) = frame.plane_mut(plane);
        for y in 0..size {
            let row_off = (by + y) * linesize + bx;
            for x in 0..size {
                let delta = mat[y * size + x] >> 6;
                let val = (dst[row_off + x] as i32 + delta).clamp(0, 255) as u8;
                dst[row_off + x] = val;
            }
        }

        Ok(())
    }

    fn add_pframe_coefficients(
        &self,
        br: &mut BitReader,
        frame: &mut YuvFrame,
        bx: usize,
        by: usize,
        size: usize,
        plane: usize,
    ) -> Result<(), String> {
        let idx = br.get_ue_golomb_31() as usize;
        if idx == 0 {
            self.add_coefficients(br, frame, bx, by, size, plane)
        } else if idx < PFRAME_BLOCK4X4_COEFFICIENTS_TAB.len() {
            let mut flags = PFRAME_BLOCK4X4_COEFFICIENTS_TAB[idx];
            for y in (by..by + 8).step_by(4) {
                for x in (bx..bx + 8).step_by(4) {
                    if (flags & 1) != 0 {
                        self.add_coefficients(br, frame, x, y, 4, plane)?;
                    }
                    flags >>= 1;
                }
            }
            Ok(())
        } else {
            Err("Invalid pframe coeff idx".into())
        }
    }

    fn block_fill<F: Fn(BlockXY) -> u8>(
        block: &mut [u8],
        size: usize,
        linesize: usize,
        w: usize,
        h: usize,
        ax: usize,
        ay: usize,
        pick: F,
    ) {
        for y in 0..size {
            for x in 0..size {
                let bxy = BlockXY {
                    w: w as i32,
                    h: h as i32,
                    ax: ax as i32,
                    ay: ay as i32,
                    x: x as i32,
                    y: y as i32,
                    size: size as i32,
                    block,
                    linesize,
                };
                let val = pick(bxy);
                block[ax + x + (ay + y) * linesize] = val;
            }
        }
    }

    fn block_sum(block: &[u8], w: usize, h: usize, linesize: usize) -> i32 {
        let mut sum = 0i32;
        let mut off = 0;
        for _ in 0..h {
            for x in 0..w {
                sum += block[off + x] as i32;
            }
            off += linesize;
        }
        sum
    }

    fn predict_intra(
        &self,
        br: &mut BitReader,
        frame: &mut YuvFrame,
        ax: usize,
        ay: usize,
        pmode: i32,
        add_coeffs: bool,
        size: usize,
        plane: usize,
    ) -> Result<(), String> {
        let (w, h) = if plane == 0 {
            (self.width, self.height)
        } else {
            (self.width >> 1, self.height >> 1)
        };

        let linesize = if plane == 0 { frame.y_stride } else { frame.uv_stride };

        match pmode {
            0 => {
                let (dst, ls) = frame.plane_mut(plane);
                Self::block_fill(dst, size, ls, w, h, ax, ay, pick_above);
            }
            1 => {
                let (dst, ls) = frame.plane_mut(plane);
                Self::block_fill(dst, size, ls, w, h, ax, ay, pick_left);
            }
            2 => {
                let mut arr1 = [0i32; 16];
                let mut arr2 = [0i32; 16];
                let dst = &mut frame.plane_mut(plane).0;
                let top_y = if ay > 0 { ay - 1 } else { 0 };
                let left_x = if ax > 0 { ax - 1 } else { 0 };
                let bottommost = dst[(ay + size - 1) * linesize + left_x] as i32;
                let rightmost = dst[top_y * linesize + ax + size - 1] as i32;

                let se = br.get_se_golomb().clamp(-(1 << 16), 1 << 16);
                let avg = (bottommost + rightmost + 1) / 2 + 2 * se;
                let r6 = adjust(avg - bottommost, size as i32);
                let r9 = adjust(avg - rightmost, size as i32);
                let shift = if adjust(size as i32, size as i32) == 8 { 3 } else { 2 };

                for x in 0..size {
                    let val = dst[top_y * linesize + ax + x] as i32;
                    arr1[x] = adjust(((bottommost - val) * (1 << shift)) + r6 * (x as i32 + 1), size as i32);
                }

                for y in 0..size {
                    let val = dst[(ay + y) * linesize + left_x] as i32;
                    arr2[y] = adjust(((rightmost - val) * (1 << shift)) + r9 * (y as i32 + 1), size as i32);
                }

                for y in 0..size {
                    let top_row = top_y * linesize + ax;
                    let left_val = dst[(ay + y) * linesize + left_x] as i32;
                    let block_row = (ay + y) * linesize + ax;
                    for x in 0..size {
                        let top_val = dst[top_row + x] as i32;
                        let val = (((top_val + left_val + ((arr1[x] * (y as i32 + 1) + arr2[y] * (x as i32 + 1)) >> (2 * shift))) + 1) / 2) & 0xFF;
                        dst[block_row + x] = val as u8;
                    }
                }
            }
            3 => {
                let fill = {
                    let dst = frame.plane(plane).0;
                    if ax == 0 && ay == 0 {
                        0x80u8
                    } else if ax >= 1 && ay >= 1 {
                        let left = Self::block_sum(&dst[ay * linesize + ax - 1..], 1, size, linesize);
                        let top = Self::block_sum(&dst[(ay - 1) * linesize + ax..], size, 1, linesize);
                        (((left + top) * 2 / (2 * size as i32) + 1) / 2) as u8
                    } else if ax >= 1 {
                        let left = Self::block_sum(&dst[ay * linesize + ax - 1..], 1, size, linesize);
                        ((left * 2 / size as i32 + 1) / 2) as u8
                    } else {
                        let top = Self::block_sum(&dst[(ay - 1) * linesize + ax..], size, 1, linesize);
                        ((top * 2 / size as i32 + 1) / 2) as u8
                    }
                };

                let dst = frame.plane_mut(plane).0;
                for y in 0..size {
                    let off = (ay + y) * linesize + ax;
                    dst[off..off + size].fill(fill);
                }
            }
            4 => {
                let (dst, ls) = frame.plane_mut(plane);
                Self::block_fill(dst, size, ls, w, h, ax, ay, pick_4);
            }
            5 => {
                let (dst, ls) = frame.plane_mut(plane);
                Self::block_fill(dst, size, ls, w, h, ax, ay, pick_5);
            }
            6 => {
                let (dst, ls) = frame.plane_mut(plane);
                Self::block_fill(dst, size, ls, w, h, ax, ay, pick_6);
            }
            7 => {
                let (dst, ls) = frame.plane_mut(plane);
                Self::block_fill(dst, size, ls, w, h, ax, ay, pick_7);
            }
            8 => {
                let (dst, ls) = frame.plane_mut(plane);
                Self::block_fill(dst, size, ls, w, h, ax, ay, pick_8);
            }
            9 => {
            }
            _ => return Err(format!("Invalid intra prediction mode: {}", pmode)),
        }

        if add_coeffs {
            self.add_coefficients(br, frame, ax, ay, size, plane)?;
        }

        Ok(())
    }

    fn get_prediction(&mut self, br: &mut BitReader, x: usize, y: usize, size: usize) -> i32 {
        let index = (y & 0xC) | ((x / 4) % 4);
        let mut val = self.pre[index].min(if index % 4 == 0 { 9 } else { self.pre[index + 3] });
        if val == 9 {
            val = 3;
        }

        if br.get_bits1() == 0 {
            let x_b = br.get_bits(3) as u8;
            val = x_b + if x_b >= val { 1 } else { 0 };
        }

        self.pre[index + 4] = val;
        if size == 8 {
            self.pre[index + 5] = val;
            self.pre[index + 8] = val;
            self.pre[index + 9] = val;
        }

        val as i32
    }

    fn process_block(
        &mut self,
        br: &mut BitReader,
        frame: &mut YuvFrame,
        x: usize,
        y: usize,
        mut pmode: i32,
        has_coeffs: bool,
        plane: usize,
    ) -> Result<(), String> {
        if !has_coeffs {
            if pmode < 0 {
                pmode = self.get_prediction(br, x, y, 8);
            }
            return self.predict_intra(br, frame, x, y, pmode, false, 8, plane);
        }

        let tmp = br.get_ue_golomb_31() as usize;
        if tmp > BLOCK4X4_COEFFICIENTS_TAB.len() {
            return Err("Invalid block4x4 coeff index".into());
        }

        if tmp == 0 {
            if pmode < 0 {
                pmode = self.get_prediction(br, x, y, 8);
            }
            self.predict_intra(br, frame, x, y, pmode, true, 8, plane)
        } else {
            let mut flags = BLOCK4X4_COEFFICIENTS_TAB[tmp - 1];
            for by in (y..y + 8).step_by(4) {
                for bx in (x..x + 8).step_by(4) {
                    let mut new_pmode = pmode;
                    if new_pmode < 0 {
                        new_pmode = self.get_prediction(br, bx, by, 4);
                    }
                    self.predict_intra(br, frame, bx, by, new_pmode, (flags & 1) != 0, 4, plane)?;
                    flags >>= 1;
                }
            }
            Ok(())
        }
    }

    fn decode_macroblock(
        &mut self,
        br: &mut BitReader,
        frame: &mut YuvFrame,
        x: usize,
        y: usize,
        predict: bool,
    ) -> Result<(), String> {
        let idx = br.get_ue_golomb() as usize;
        if idx >= BLOCK8X8_COEFFICIENTS_TAB.len() {
            return Err("Invalid mb block8x8 coeff index".into());
        }
        let mut flags = BLOCK8X8_COEFFICIENTS_TAB[idx];

        let (u_plane, v_plane) = if self.moflex { (1, 2) } else { (2, 1) };

        if predict {
            self.process_block(br, frame, x, y, -1, (flags & 1) != 0, 0)?;
            flags >>= 1;
            self.process_block(br, frame, x + 8, y, -1, (flags & 1) != 0, 0)?;
            flags >>= 1;
            self.process_block(br, frame, x, y + 8, -1, (flags & 1) != 0, 0)?;
            flags >>= 1;
            self.process_block(br, frame, x + 8, y + 8, -1, (flags & 1) != 0, 0)?;
            flags >>= 1;
        } else {
            let mut pmode = br.get_bits(3) as i32;
            if pmode == 2 {
                self.predict_intra(br, frame, x, y, pmode, false, 16, 0)?;
                pmode = 9;
            }

            self.process_block(br, frame, x, y, pmode, (flags & 1) != 0, 0)?;
            flags >>= 1;
            self.process_block(br, frame, x + 8, y, pmode, (flags & 1) != 0, 0)?;
            flags >>= 1;
            self.process_block(br, frame, x, y + 8, pmode, (flags & 1) != 0, 0)?;
            flags >>= 1;
            self.process_block(br, frame, x + 8, y + 8, pmode, (flags & 1) != 0, 0)?;
            flags >>= 1;
        }

        let mut pmode_uv = br.get_bits(3) as i32;
        if pmode_uv == 2 {
            self.predict_intra(br, frame, x >> 1, y >> 1, pmode_uv, false, 8, u_plane)?;
            self.predict_intra(br, frame, x >> 1, y >> 1, pmode_uv, false, 8, v_plane)?;
            pmode_uv = 9;
        }

        self.process_block(br, frame, x >> 1, y >> 1, pmode_uv, (flags & 1) != 0, u_plane)?;
        flags >>= 1;
        self.process_block(br, frame, x >> 1, y >> 1, pmode_uv, (flags & 1) != 0, v_plane)?;

        Ok(())
    }

    fn predict_motion(
        &mut self,
        br: &mut BitReader,
        mut width: usize,
        mut height: usize,
        index: usize,
        offsetm: usize,
        offsetx: usize,
        offsety: usize,
    ) -> Result<(), String> {
        let fheight = self.height;
        let fwidth = self.width;

        if index <= 5 {
            let sidx = (self.current_pic + 6 - index.max(1)) % 6;
            let mut mv = self.motion[0];

            if index > 0 {
                mv.x = mv.x.wrapping_add(br.get_se_golomb());
                mv.y = mv.y.wrapping_add(br.get_se_golomb());
            }

            self.motion[offsetm] = mv;

            let mut cur_w = width;
            let mut cur_h = height;
            let mut cur_fw = fwidth;
            let mut cur_fh = fheight;
            let mut cur_ox = offsetx;
            let mut cur_oy = offsety;
            let mut cur_mv_x = mv.x;
            let mut cur_mv_y = mv.y;

            for plane in 0..3 {
                if plane == 1 {
                    cur_ox >>= 1;
                    cur_oy >>= 1;
                    cur_mv_x >>= 1;
                    cur_mv_y >>= 1;
                    cur_w >>= 1;
                    cur_h >>= 1;
                    cur_fw >>= 1;
                    cur_fh >>= 1;
                }

                let method = (cur_mv_x & 1) | ((cur_mv_y & 1) << 1);

                if (cur_ox as i32) + (cur_mv_x >> 1) < 0
                    || (cur_oy as i32) + (cur_mv_y >> 1) < 0
                    || (cur_ox as i32) + (cur_w as i32) + ((cur_mv_x + 1) >> 1) > (cur_fw as i32)
                    || (cur_oy as i32) + (cur_h as i32) + ((cur_mv_y + 1) >> 1) > (cur_fh as i32)
                {
                    return Err("Motion vector out of bounds".into());
                }

                let cur_pic = self.current_pic;
                let (src_linesize, dst_linesize) = {
                    let ls_src = self.pics[sidx].plane(plane).1;
                    let ls_dst = self.pics[cur_pic].plane(plane).1;
                    (ls_src, ls_dst)
                };

                let src_base = ((cur_ox as i32 + (cur_mv_x >> 1)) as usize)
                    + ((cur_oy as i32 + (cur_mv_y >> 1)) as usize) * src_linesize;
                let dst_base = cur_ox + cur_oy * dst_linesize;

                let (src_slice, dst_slice) = if sidx < cur_pic {
                    let (left, right) = self.pics.split_at_mut(cur_pic);
                    (left[sidx].plane(plane).0, right[0].plane_mut(plane).0)
                } else if sidx > cur_pic {
                    let (left, right) = self.pics.split_at_mut(sidx);
                    (right[0].plane(plane).0, left[cur_pic].plane_mut(plane).0)
                } else {
                    return Err("Same picture index in motion prediction".into());
                };

                match method {
                    0 => {
                        for y in 0..cur_h {
                            let src_row = src_base + y * src_linesize;
                            let dst_row = dst_base + y * dst_linesize;
                            dst_slice[dst_row..dst_row + cur_w].copy_from_slice(&src_slice[src_row..src_row + cur_w]);
                        }
                    }
                    1 => {
                        for y in 0..cur_h {
                            let src_row = src_base + y * src_linesize;
                            let dst_row = dst_base + y * dst_linesize;
                            for x in 0..cur_w {
                                dst_slice[dst_row + x] = ((src_slice[src_row + x] >> 1)
                                    + (src_slice[src_row + x + 1] >> 1)) as u8;
                            }
                        }
                    }
                    2 => {
                        for y in 0..cur_h {
                            let src_row = src_base + y * src_linesize;
                            let dst_row = dst_base + y * dst_linesize;
                            for x in 0..cur_w {
                                dst_slice[dst_row + x] = ((src_slice[src_row + x] >> 1)
                                    + (src_slice[src_row + x + src_linesize] >> 1)) as u8;
                            }
                        }
                    }
                    3 => {
                        for y in 0..cur_h {
                            let src_row = src_base + y * src_linesize;
                            let dst_row = dst_base + y * dst_linesize;
                            for x in 0..cur_w {
                                let top = (src_slice[src_row + x] >> 1) + (src_slice[src_row + x + 1] >> 1);
                                let bot = (src_slice[src_row + x + src_linesize] >> 1)
                                    + (src_slice[src_row + x + 1 + src_linesize] >> 1);
                                dst_slice[dst_row + x] = ((top >> 1) + (bot >> 1)) as u8;
                            }
                        }
                    }
                    _ => unreachable!(),
                }
            }
        } else {
            let adjx = if index == 8 { 0 } else { width / 2 };
            let adjy = if index == 8 { height / 2 } else { 0 };

            width -= adjx;
            height -= adjy;
            let tidx = get_index(height as i32) * 4 + get_index(width as i32);
            let moflex_idx = self.moflex as usize;

            for i in 0..2 {
                let idx2 = self.mv_vlc[moflex_idx][tidx].get_symbol(br) as usize;
                self.predict_motion(
                    br,
                    width,
                    height,
                    idx2,
                    offsetm,
                    offsetx + i * adjx,
                    offsety + i * adjy,
                )?;
            }
        }

        Ok(())
    }

    pub fn decode_frame(&mut self, pkt: &[u8]) -> Result<usize, String> {
        let aligned_len = (pkt.len() + 1) & !1;
        let mut swapped = vec![0u8; aligned_len];

        for i in 0..(pkt.len() / 2) {
            swapped[i * 2] = pkt[i * 2 + 1];
            swapped[i * 2 + 1] = pkt[i * 2];
        }
        if pkt.len() % 2 != 0 {
            swapped[pkt.len() - 1] = 0;
            swapped[pkt.len()] = pkt[pkt.len() - 1];
        }

        let mut br = BitReader::new(&swapped);

        let is_keyframe = br.get_bits1() != 0;
        let cur_pic = self.current_pic;

        if is_keyframe {
            self.moflex = br.get_bits1() != 0;
            self.dct_tab_idx = br.get_bits1() as usize;
            let q = br.get_bits(6) as i32;
            self.setup_qtables(q)?;

            for y in (0..self.height).step_by(16) {
                for x in (0..self.width).step_by(16) {
                    let predict = br.get_bits1() != 0;
                    let mut frame = std::mem::take(&mut self.pics[cur_pic]);
                    self.decode_macroblock(&mut br, &mut frame, x, y, predict)?;
                    self.pics[cur_pic] = frame;
                }
            }
        } else {
            self.motion.fill(MotionXY::default());
            self.dct_tab_idx = 0;
            let delta_q = br.get_se_golomb();
            self.setup_qtables(self.quantizer + delta_q)?;

            let (u_plane, v_plane) = if self.moflex { (1, 2) } else { (2, 1) };
            let moflex_idx = self.moflex as usize;

            for y in (0..self.height).step_by(16) {
                for x in (0..self.width).step_by(16) {
                    let col = x / 16;
                    let m1 = self.motion[col + 1];
                    let m2 = self.motion[col + 2];
                    let m3 = self.motion[col + 3];

                    self.motion[0].x = mid_pred(m1.x, m2.x, m3.x);
                    self.motion[0].y = mid_pred(m1.y, m2.y, m3.y);
                    self.motion[col + 2] = MotionXY::default();

                    let idx = self.mv_vlc[moflex_idx][0].get_symbol(&mut br) as usize;

                    if idx == 6 || idx == 7 {
                        let mut frame = std::mem::take(&mut self.pics[cur_pic]);
                        self.decode_macroblock(&mut br, &mut frame, x, y, idx == 7)?;
                        self.pics[cur_pic] = frame;
                    } else {
                        self.predict_motion(&mut br, 16, 16, idx, col + 2, x, y)?;

                        let idx2 = br.get_ue_golomb() as usize;
                        if idx2 >= PFRAME_BLOCK8X8_COEFFICIENTS_TAB.len() {
                            return Err("Invalid pframe coeff idx2".into());
                        }
                        let mut flags = PFRAME_BLOCK8X8_COEFFICIENTS_TAB[idx2];

                        let mut frame = std::mem::take(&mut self.pics[cur_pic]);
                        for sy in (y..y + 16).step_by(8) {
                            for sx in (x..x + 16).step_by(8) {
                                if (flags & 1) != 0 {
                                    self.add_pframe_coefficients(&mut br, &mut frame, sx, sy, 8, 0)?;
                                }
                                flags >>= 1;
                            }
                        }

                        if (flags & 1) != 0 {
                            self.add_pframe_coefficients(&mut br, &mut frame, x >> 1, y >> 1, 8, u_plane)?;
                        }
                        flags >>= 1;
                        if (flags & 1) != 0 {
                            self.add_pframe_coefficients(&mut br, &mut frame, x >> 1, y >> 1, 8, v_plane)?;
                        }
                        self.pics[cur_pic] = frame;
                    }
                }
            }
        }

        let result_pic = self.current_pic;
        self.current_pic = (self.current_pic + 1) % 6;
        self.last_consumed_bytes = ((br.bit_pos() + 15) / 16) * 2;
        Ok(result_pic)
    }

    pub fn render_to_rgba(&self, pic_idx: usize, out_rgba: &mut [u8]) {
        self.yuv_to_rgba(&self.pics[pic_idx], out_rgba);
    }

    pub fn yuv_to_rgba(&self, frame: &YuvFrame, out_rgba: &mut [u8]) {
        let width = self.width;
        let height = self.height;

        if !self.moflex {
            for y in 0..height {
                let y_off = y * frame.y_stride;
                let uv_off = (y >> 1) * frame.uv_stride;
                let rgba_row = y * width * 4;

                for x in 0..width {
                    let py = frame.y[y_off + x] as i32;
                    let cg = frame.u[uv_off + (x >> 1)] as i32 - 128;
                    let co = frame.v[uv_off + (x >> 1)] as i32 - 128;

                    let t = py - cg;
                    let r = (t + co).clamp(0, 255) as u8;
                    let g = (py + cg).clamp(0, 255) as u8;
                    let b = (t - co).clamp(0, 255) as u8;

                    let px = rgba_row + x * 4;
                    out_rgba[px] = r;
                    out_rgba[px + 1] = g;
                    out_rgba[px + 2] = b;
                    out_rgba[px + 3] = 255;
                }
            }
        } else {
            for y in 0..height {
                let y_off = y * frame.y_stride;
                let uv_off = (y >> 1) * frame.uv_stride;
                let rgba_row = y * width * 4;

                for x in 0..width {
                    let py = frame.y[y_off + x] as i32;
                    let cb = frame.u[uv_off + (x >> 1)] as i32 - 128;
                    let cr = frame.v[uv_off + (x >> 1)] as i32 - 128;

                    let r = (py + ((1402 * cr) >> 10)).clamp(0, 255) as u8;
                    let g = (py - ((344 * cb + 714 * cr) >> 10)).clamp(0, 255) as u8;
                    let b = (py + ((1772 * cb) >> 10)).clamp(0, 255) as u8;

                    let px = rgba_row + x * 4;
                    out_rgba[px] = r;
                    out_rgba[px + 1] = g;
                    out_rgba[px + 2] = b;
                    out_rgba[px + 3] = 255;
                }
            }
        }
    }
}
