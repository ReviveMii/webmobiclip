#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerType {
    Mods,
    Moflex,
    Moc5,
    Unknown,
}

#[derive(Clone)]
pub struct VideoPacket {
    pub data: Vec<u8>,
    pub is_key: bool,
    pub timestamp: f64,
    pub decoded: Option<crate::mobiclip::YuvFrame>,
}

#[derive(Debug, Clone)]
pub struct AudioPacket {
    pub data: Vec<u8>,
    pub timestamp: f64,
    pub is_key: bool,
}

use crate::mobiclip::MobiClipDecoder;

const MODS_AUDIO_FLAG: u32 = 0x2000;

pub struct ModsDemuxer<'a> {
    data: &'a [u8],
    pub width: usize,
    pub height: usize,
    pub fps: f64,
    pub nb_frames: usize,
    pub has_audio: bool,
    pub embedded_audio: bool,
    pub audio_codec: u16,
    pub sample_rate: u32,
    pub channels: usize,
    pub has_sx_audio: bool,
    pub sx_codebook: Vec<u8>,
    sx_vdec: Option<MobiClipDecoder>,
    audio_block: usize,
    frame_data_end: usize,
    current_frame: usize,
    cursor: usize,
}

const SX_CODEBOOK_SIZE: usize = 0xC34;

impl<'a> ModsDemuxer<'a> {
    pub fn open(data: &'a [u8]) -> Result<Self, String> {
        if data.len() < 0x34 {
            return Err("MODS file too small".into());
        }
        if &data[0..8] != b"MODSN3\n\0" {
            return Err("Invalid MODS magic".into());
        }

        let nb_frames = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
        let width = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
        let height = u32::from_le_bytes(data[16..20].try_into().unwrap()) as usize;
        let fps_num = u32::from_le_bytes(data[20..24].try_into().unwrap()) as f64;
        let fps = fps_num / 16777216.0;

        let acodec = u16::from_le_bytes(data[0x18..0x1A].try_into().unwrap());
        let ach_raw = u16::from_le_bytes(data[0x1A..0x1C].try_into().unwrap()) as usize;
        let arate = u32::from_le_bytes(data[0x1C..0x20].try_into().unwrap());
        let acodec_info_off = u32::from_le_bytes(data[0x24..0x28].try_into().unwrap()) as usize;
        let kf_table_off = u32::from_le_bytes(data[0x28..0x2C].try_into().unwrap()) as usize;

        let embedded_audio = acodec > 1 && acodec != 0xFF;
        let channels = if ach_raw > 0 { ach_raw } else { 2 };
        let (has_audio, audio_codec, audio_block, has_sx_audio, sx_codebook) = if embedded_audio {
            let (ha, ac, ab) = match acodec {
                2 => (true, 2u16, channels * 40),
                3 => (true, 1u16, channels * 128),
                4 => (true, 3u16, channels * 512),
                _ => (false, 0u16, 0),
            };
            (ha, ac, ab, false, Vec::new())
        } else if acodec == 1 && ach_raw >= 1 && ach_raw <= 2 && arate > 0 && acodec_info_off > 0
            && acodec_info_off + channels * SX_CODEBOOK_SIZE <= data.len()
        {
            let cbk = data[acodec_info_off..acodec_info_off + channels * SX_CODEBOOK_SIZE].to_vec();
            (true, 4u16, 0, true, cbk)
        } else {
            (false, 0u16, 0, false, Vec::new())
        };

        let frame_data_end = if acodec_info_off > kf_table_off { acodec_info_off } else { 0 };

        let pos1 = kf_table_off + 4;
        if pos1 + 4 > data.len() {
            return Err("Invalid frame offset table".into());
        }
        let start_pos = u32::from_le_bytes(data[pos1..pos1 + 4].try_into().unwrap()) as usize;
        if start_pos >= data.len() {
            return Err("Invalid start pos".into());
        }

        Ok(Self {
            data,
            width,
            height,
            fps: if fps > 0.0 { fps } else { 30.0 },
            nb_frames,
            has_audio,
            embedded_audio,
            audio_codec,
            sample_rate: if arate > 0 { arate } else { 32000 },
            channels,
            has_sx_audio,
            sx_codebook,
            sx_vdec: None,
            audio_block,
            frame_data_end,
            current_frame: 0,
            cursor: start_pos,
        })
    }

    pub fn next_chunk(&mut self) -> Option<(VideoPacket, Option<AudioPacket>)> {
        if self.current_frame >= self.nb_frames || self.cursor + 4 > self.data.len() {
            return None;
        }
        if self.frame_data_end > 0 && self.cursor >= self.frame_data_end {
            return None;
        }

        let chunk_word = u32::from_le_bytes(self.data[self.cursor..self.cursor + 4].try_into().unwrap());
        self.cursor += 4;
        let size = (chunk_word >> 14) as usize;
        let flags = chunk_word & 0x3FFF;

        if self.cursor + size > self.data.len() {
            return None;
        }

        let raw = &self.data[self.cursor..self.cursor + size];
        self.cursor += size;
        let ts = self.current_frame as f64 / self.fps;
        self.current_frame += 1;

        if self.has_audio && !self.embedded_audio && (flags & MODS_AUDIO_FLAG) != 0 {
            return Some((
                VideoPacket { data: Vec::new(), is_key: false, timestamp: ts, decoded: None },
                Some(AudioPacket { data: raw.to_vec(), timestamp: ts, is_key: false }),
            ));
        }

        let is_key = raw.len() >= 2 && (raw[1] & 0x80) != 0;

        if self.has_sx_audio && size >= 4 {
            let stored_asize = u16::from_le_bytes([raw[size - 2], raw[size - 1]]) as usize;
            let spad = raw[size - 4] as usize;
            if stored_asize > 0 && spad <= 3 && stored_asize + spad + 4 <= size {
                let vfield = size - 4 - spad - stored_asize;
                let mut audio_start = vfield;
                let mut audio_size = stored_asize;
                if is_key && audio_size >= 4 {
                    audio_start += 4;
                    audio_size -= 4;
                }
                let video = VideoPacket { data: raw[..vfield].to_vec(), is_key, timestamp: ts, decoded: None };
                if audio_size > 0 {
                    let periods = flags as usize;
                    let total_afr = (periods * self.channels) as u32;
                    let mut apkt = Vec::with_capacity(4 + audio_size);
                    apkt.extend_from_slice(&total_afr.to_le_bytes());
                    apkt.extend_from_slice(&raw[audio_start..audio_start + audio_size]);
                    return Some((video, Some(AudioPacket { data: apkt, timestamp: ts, is_key })));
                }
                return Some((video, None));
            }

            let periods = flags as usize;
            let total_afr = periods * self.channels;
            if total_afr > 0 {
                if self.sx_vdec.is_none() {
                    let dec_w = (self.width + 15) & !15;
                    let dec_h = (self.height + 15) & !15;
                    self.sx_vdec = MobiClipDecoder::new(dec_w, dec_h).ok();
                }
                if let Some(vdec) = &mut self.sx_vdec {
                    if let Ok(pic_idx) = vdec.decode_frame(raw) {
                        let vfield = vdec.last_consumed_bytes().min(size);
                        let mut audio_start = vfield;
                        if is_key && audio_start + 4 <= size {
                            audio_start += 4;
                        }
                        let audio_size = size - audio_start;
                        let video = VideoPacket {
                            data: raw[..vfield].to_vec(),
                            is_key,
                            timestamp: ts,
                            decoded: Some(vdec.get_pic(pic_idx).clone()),
                        };
                        if audio_size > 0 {
                            let mut apkt = Vec::with_capacity(4 + audio_size);
                            apkt.extend_from_slice(&(total_afr as u32).to_le_bytes());
                            apkt.extend_from_slice(&raw[audio_start..audio_start + audio_size]);
                            return Some((video, Some(AudioPacket { data: apkt, timestamp: ts, is_key })));
                        }
                        return Some((video, None));
                    }
                }
            }
        }

        if self.embedded_audio && self.audio_block > 0 && size >= 4 {
            let audio_blocks = flags as usize;
            let mut audio_size = audio_blocks * self.audio_block;
            if self.audio_codec == 1 && is_key && audio_blocks > 0 {
                audio_size += 4 + 4 * self.channels;
            }
            if audio_size > 0 && audio_size <= size {
                let video_len = size - audio_size;
                let a_pkt = AudioPacket {
                    data: raw[video_len..].to_vec(),
                    timestamp: ts,
                    is_key,
                };
                return Some((
                    VideoPacket { data: raw[..video_len].to_vec(), is_key, timestamp: ts, decoded: None },
                    Some(a_pkt),
                ));
            }
        }

        Some((VideoPacket { data: raw.to_vec(), is_key, timestamp: ts, decoded: None }, None))
    }
}

pub struct Moc5Demuxer<'a> {
    data: &'a [u8],
    pub width: usize,
    pub height: usize,
    pub fps: f64,
    pub nb_frames: usize,
    pub has_audio: bool,
    pub audio_codec: u16,
    pub sample_rate: u32,
    pub channels: usize,
    cursor: usize,
    current_frame: usize,
}

impl<'a> Moc5Demuxer<'a> {
    pub fn open(data: &'a [u8]) -> Result<Self, String> {
        if data.len() < 16 || &data[0..4] != b"MOC5" {
            return Err("Invalid MOC5 file".into());
        }

        let tag_section_size = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
        let mut offset = 8;
        let header_end = 8 + tag_section_size;
        if header_end > data.len() {
            return Err("Corrupted MOC5 header".into());
        }

        let mut width = 0;
        let mut height = 0;
        let mut fps = 30.0;
        let mut nb_frames = 0;
        let mut audio_codec = 0;
        let mut sample_rate = 32000;
        let mut channels = 2;
        let mut has_audio = false;

        while offset + 4 <= header_end {
            let tag = &data[offset..offset + 2];
            let word_len = u16::from_le_bytes([data[offset + 2], data[offset + 3]]) as usize;
            let payload_len = word_len * 4;
            let payload_start = offset + 4;
            let next_offset = payload_start + payload_len;

            if next_offset > header_end {
                break;
            }

            match tag {
                b"V2" => {
                    if payload_len >= 8 {
                        width = u32::from_le_bytes(data[payload_start..payload_start + 4].try_into().unwrap()) as usize;
                        height = u32::from_le_bytes(data[payload_start + 4..payload_start + 8].try_into().unwrap()) as usize;
                    }
                }
                b"TL" => {
                    if payload_len >= 8 {
                        let fps_den = u32::from_le_bytes(data[payload_start..payload_start + 4].try_into().unwrap()) as f64;
                        nb_frames = u32::from_le_bytes(data[payload_start + 4..payload_start + 8].try_into().unwrap()) as usize;
                        if fps_den > 0.0 {
                            fps = fps_den / 256.0;
                        }
                    }
                }
                b"A8" | b"A9" => {
                    has_audio = true;
                    audio_codec = 1;
                    channels = if tag == b"A9" { 2 } else { 1 };
                    if payload_len >= 4 {
                        sample_rate = u32::from_le_bytes(data[payload_start..payload_start + 4].try_into().unwrap());
                    }
                }
                b"A2" | b"A3" => {
                    has_audio = true;
                    audio_codec = 2;
                    channels = if tag == b"A3" { 2 } else { 1 };
                    if payload_len >= 4 {
                        sample_rate = u32::from_le_bytes(data[payload_start..payload_start + 4].try_into().unwrap());
                    }
                }
                b"AP" => {
                    has_audio = true;
                    audio_codec = 3;
                    if payload_len >= 8 {
                        sample_rate = u32::from_le_bytes(data[payload_start..payload_start + 4].try_into().unwrap());
                        let ch = u32::from_le_bytes(data[payload_start + 4..payload_start + 8].try_into().unwrap()) as usize;
                        channels = if ch == 1 { 1 } else { 2 };
                    }
                }
                b"AV" | b"AA" => {
                    has_audio = true;
                    audio_codec = 4;
                    if payload_len >= 4 {
                        let p1_len = u32::from_le_bytes(data[payload_start..payload_start + 4].try_into().unwrap()) as usize;
                        if payload_len >= 4 + p1_len && p1_len >= 16 {
                            let p1 = &data[payload_start + 4..payload_start + 4 + p1_len];
                            channels = p1[11] as usize;
                            sample_rate = u32::from_le_bytes(p1[12..16].try_into().unwrap());
                        }
                    }
                }
                b"HE" => {
                    offset = next_offset;
                    break;
                }
                _ => {}
            }

            offset = next_offset;
        }

        Ok(Self {
            data,
            width,
            height,
            fps,
            nb_frames,
            has_audio,
            audio_codec,
            sample_rate,
            channels,
            cursor: offset,
            current_frame: 0,
        })
    }

    pub fn next_chunk(&mut self) -> Option<(VideoPacket, Option<AudioPacket>)> {
        if self.cursor + 8 > self.data.len() {
            return None;
        }

        let chunk_pos = self.cursor;
        let chunk_size = u32::from_le_bytes(self.data[chunk_pos..chunk_pos + 4].try_into().unwrap()) as usize;
        let video_size = u32::from_le_bytes(self.data[chunk_pos + 4..chunk_pos + 8].try_into().unwrap()) as usize;

        if chunk_size < 8 || video_size > chunk_size.saturating_sub(4) {
            return None;
        }

        let raw_end = chunk_pos + chunk_size;
        let pad = (4 - (raw_end % 4)) as usize;
        let next_chunk_pos = (raw_end + pad).min(self.data.len());

        let v_data = if video_size > 0 && chunk_pos + 8 + video_size <= self.data.len() {
            self.data[chunk_pos + 8..chunk_pos + 8 + video_size].to_vec()
        } else {
            Vec::new()
        };

        let audio_len = if chunk_size >= video_size + 8 {
            chunk_size - video_size - 8
        } else {
            0
        };

        let a_data = if audio_len > 0 {
            let a_start = chunk_pos + 8 + video_size;
            let read_size = if self.audio_codec == 1 {
                let block_size = self.channels * 132;
                if block_size > 0 {
                    let total_available = audio_len + pad;
                    let rounded = (total_available / block_size) * block_size;
                    if rounded > audio_len { rounded.min(self.data.len() - a_start) } else { audio_len }
                } else {
                    audio_len
                }
            } else {
                audio_len
            };

            if a_start + read_size <= self.data.len() {
                Some(AudioPacket {
                    data: self.data[a_start..a_start + read_size].to_vec(),
                    timestamp: self.current_frame as f64 / self.fps,
                    is_key: false,
                })
            } else {
                None
            }
        } else {
            None
        };

        self.cursor = next_chunk_pos;
        let ts = self.current_frame as f64 / self.fps;
        let is_key = v_data.len() >= 2 && (v_data[1] & 0x80) != 0;
        self.current_frame += 1;

        Some((
            VideoPacket {
                data: v_data,
                is_key,
                timestamp: ts,
                decoded: None,
            },
            a_data,
        ))
    }
}

pub struct BitReaderMoflex<'a> {
    data: &'a [u8],
    pos: usize,
    bit_pos: usize,
}

impl<'a> BitReaderMoflex<'a> {
    pub fn new(data: &'a [u8], offset: usize) -> Self {
        Self {
            data,
            pos: offset,
            bit_pos: 0,
        }
    }

    pub fn pop(&mut self) -> Option<u32> {
        let byte_idx = self.pos + (self.bit_pos >> 3);
        if byte_idx >= self.data.len() {
            return None;
        }
        let bit_in_byte = 7 - (self.bit_pos & 7);
        self.bit_pos += 1;
        Some(((self.data[byte_idx] >> bit_in_byte) & 1) as u32)
    }

    pub fn pop_int(&mut self, n: usize) -> Option<u32> {
        let mut val = 0;
        for _ in 0..n {
            val = (val << 1) | self.pop()?;
        }
        Some(val)
    }

    pub fn pop_length(&mut self) -> Option<usize> {
        let mut n = 1;
        while self.pop()? == 0 {
            n += 1;
        }
        Some(n)
    }

    pub fn bytes_consumed(&self) -> usize {
        (self.bit_pos + 7) >> 3
    }
}

pub struct MoflexDemuxer<'a> {
    data: &'a [u8],
    pub width: usize,
    pub height: usize,
    pub fps: f64,
    pub has_audio: bool,
    pub audio_codec: u16,
    pub sample_rate: u32,
    pub channels: usize,
    cursor: usize,
    current_time: f64,
    video_stream: Option<u32>,
    audio_stream: Option<u32>,
    video_buf: Vec<u8>,
    audio_buf: Vec<u8>,
    packet_size: usize,
    pending_video: std::collections::VecDeque<VideoPacket>,
    pending_audio: std::collections::VecDeque<AudioPacket>,
}

impl<'a> MoflexDemuxer<'a> {
    fn read_var_byte(data: &[u8], cursor: &mut usize) -> Option<u32> {
        let mut b = |cursor: &mut usize| -> Option<u32> {
            if *cursor >= data.len() {
                return None;
            }
            let v = data[*cursor] as u32;
            *cursor += 1;
            Some(v)
        };

        let b0 = b(cursor)?;
        if b0 & 0x80 == 0 {
            return Some(b0);
        }
        let mut value = (b0 & 0x7F) << 7;

        let b1 = b(cursor)?;
        if b1 & 0x80 == 0 {
            return Some(value | b1);
        }
        value = (value | (b1 & 0x7F)) << 7;

        let b2 = b(cursor)?;
        if b2 & 0x80 == 0 {
            return Some(value | b2);
        }
        value = (value | (b2 & 0x7F)) << 7;

        let b3 = b(cursor)?;
        Some(value | b3)
    }

    pub fn open(data: &'a [u8]) -> Result<Self, String> {
        if data.len() < 16 || u16::from_be_bytes([data[0], data[1]]) != 0x4C32 {
            return Err("Not a MOFLEX stream".into());
        }

        let mut width = 0;
        let mut height = 0;
        let mut fps = 30.0;
        let mut has_audio = false;
        let mut audio_codec = 0;
        let mut sample_rate = 32000;
        let mut channels = 2;
        let mut video_stream = None;
        let mut audio_stream = None;

        let mut offset = 14;
        loop {
            let desc_type = match Self::read_var_byte(data, &mut offset) {
                Some(v) => v as u8,
                None => break,
            };
            let desc_size = match Self::read_var_byte(data, &mut offset) {
                Some(v) => v as usize,
                None => break,
            };
            if desc_type == 0 {
                break;
            }

            if offset + desc_size > data.len() {
                break;
            }

            match desc_type {
                1 | 3 => {
                    let d = &data[offset..offset + desc_size];
                    if d.len() >= 10 && d[1] == 0 {
                        video_stream = Some(d[0] as u32);
                        let fps_den = u16::from_be_bytes([d[2], d[3]]) as f64;
                        let fps_num = u16::from_be_bytes([d[4], d[5]]) as f64;
                        if fps_den > 0.0 {
                            fps = fps_den / fps_num.max(1.0);
                        }
                        width = u16::from_be_bytes([d[6], d[7]]) as usize;
                        height = u16::from_be_bytes([d[8], d[9]]) as usize;
                    }
                }
                2 => {
                    let d = &data[offset..offset + desc_size];
                    if d.len() >= 6 {
                        audio_stream = Some(d[0] as u32);
                        has_audio = true;
                        audio_codec = d[1] as u16;
                        sample_rate = ((d[2] as u32) << 16 | (d[3] as u32) << 8 | (d[4] as u32)) + 1;
                        channels = d[5] as usize + 1;
                    }
                }
                _ => {}
            }
            offset += desc_size;
        }

        if video_stream.is_none() {
            return Err("MOFLEX has no MobiClip video stream".into());
        }

        Ok(Self {
            data,
            width,
            height,
            fps,
            has_audio,
            audio_codec,
            sample_rate,
            channels,
            cursor: 0,
            current_time: 0.0,
            video_stream,
            audio_stream,
            video_buf: Vec::new(),
            audio_buf: Vec::new(),
            packet_size: 0,
            pending_video: std::collections::VecDeque::new(),
            pending_audio: std::collections::VecDeque::new(),
        })
    }

    pub fn next_packets(&mut self) -> Option<(Option<VideoPacket>, Option<AudioPacket>)> {
        if !self.pending_video.is_empty() || !self.pending_audio.is_empty() {
            return Some((self.pending_video.pop_front(), self.pending_audio.pop_front()));
        }

        while self.cursor < self.data.len() {
            let start_offset = self.cursor;
            let has_header = self.cursor + 14 <= self.data.len()
                && u16::from_be_bytes([self.data[self.cursor], self.data[self.cursor + 1]]) == 0x4C32;

            if has_header {
                self.packet_size = u16::from_be_bytes([self.data[self.cursor + 12], self.data[self.cursor + 13]]) as usize + 1;
                self.cursor += 14;

                let block_end = start_offset + self.packet_size;
                loop {
                    if self.cursor >= block_end || self.cursor >= self.data.len() {
                        break;
                    }
                    let before = self.cursor;
                    let desc_type = match Self::read_var_byte(self.data, &mut self.cursor) {
                        Some(v) => v,
                        None => break,
                    };
                    let desc_size = match Self::read_var_byte(self.data, &mut self.cursor) {
                        Some(v) => v as usize,
                        None => break,
                    };
                    if desc_type == 0 {
                        self.cursor = (self.cursor + desc_size).min(block_end);
                        break;
                    }
                    let desc_end = (self.cursor + desc_size).min(block_end);
                    self.cursor = desc_end;
                    if self.cursor <= before {
                        break;
                    }
                }
            }

            if self.packet_size == 0 || self.cursor >= self.data.len() {
                break;
            }

            let flags = self.data[self.cursor];
            self.cursor += 1;
            if (flags & 2) != 0 {
                self.cursor += 2;
            }

            let packet_end = (start_offset + self.packet_size).min(self.data.len());
            let payload_start = self.cursor;
            let mut br = BitReaderMoflex::new(self.data, payload_start);

            loop {
                let peek_pos = payload_start + br.bytes_consumed();
                if peek_pos >= packet_end {
                    break;
                }
                if br.bit_pos % 8 == 0 && self.data[peek_pos] == 0 {
                    br.bit_pos += 8;
                    break;
                }

                let bits = match br.pop_length() {
                    Some(b) => b,
                    None => break,
                };
                let stream_idx = match br.pop_int(bits) {
                    Some(s) => s,
                    None => break,
                };
                let endframe = match br.pop() {
                    Some(e) => e != 0,
                    None => break,
                };
                if endframe {
                    let b = match br.pop_length() { Some(b) => b, None => break };
                    if br.pop_int(b).is_none() { break; }
                    if br.pop().is_none() { break; }
                    let b2 = match br.pop_length() { Some(b) => b, None => break };
                    if br.pop_int(b2 * 2 + 26).is_none() { break; }
                }

                let pkt_size = match br.pop_int(13) { Some(v) => v as usize + 1, None => break };
                let header_bytes = br.bytes_consumed();
                let payload_offset = payload_start + header_bytes;

                if payload_offset + pkt_size > packet_end {
                    break;
                }

                let chunk = &self.data[payload_offset..payload_offset + pkt_size];
                if Some(stream_idx) == self.video_stream {
                    self.video_buf.extend_from_slice(chunk);
                    if endframe && !self.video_buf.is_empty() {
                        let data = std::mem::take(&mut self.video_buf);
                        let is_key = data.len() >= 2 && (data[1] & 0x80) != 0;
                        self.pending_video.push_back(VideoPacket {
                            data,
                            is_key,
                            timestamp: self.current_time,
                            decoded: None,
                        });
                        self.current_time += 1.0 / self.fps;
                    }
                } else if Some(stream_idx) == self.audio_stream {
                    self.audio_buf.extend_from_slice(chunk);
                    if endframe && !self.audio_buf.is_empty() {
                        let data = std::mem::take(&mut self.audio_buf);
                        self.pending_audio.push_back(AudioPacket {
                            data,
                            timestamp: self.current_time,
                            is_key: false,
                        });
                    }
                }
                br.bit_pos = (header_bytes + pkt_size) * 8;
            }

            self.cursor = payload_start + br.bytes_consumed();
            if flags % 2 == 0 {
                self.cursor = packet_end;
            }

            if !self.pending_video.is_empty() || !self.pending_audio.is_empty() {
                return Some((self.pending_video.pop_front(), self.pending_audio.pop_front()));
            }
        }

        None
    }
}
