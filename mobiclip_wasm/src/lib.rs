mod audio;
mod bitreader;
mod demuxer;
mod mobiclip;
mod tables;
mod vlc;

use wasm_bindgen::prelude::*;

use audio::{AdpcmDecoder, DspAdpcmDecoder, FastAudioDecoder, VxAudioDecoder};
use demuxer::{ContainerType, Moc5Demuxer, MoflexDemuxer, ModsDemuxer, VideoPacket, AudioPacket};
use mobiclip::MobiClipDecoder;

#[wasm_bindgen]
pub struct WebMobiPlayer {
    decoder: Option<MobiClipDecoder>,
    adpcm: Option<AdpcmDecoder>,
    fastaudio: Option<FastAudioDecoder>,
    vx_audio: Option<VxAudioDecoder>,
    dsp_adpcm: Option<DspAdpcmDecoder>,
    moflex_pcm16: bool,
    width: usize,
    height: usize,
    fps: f64,
    has_audio: bool,
    sample_rate: u32,
    channels: usize,
    container_type: ContainerType,
    mods_pcm16: bool,
    raw_data: Vec<u8>,
    video_packets: Vec<VideoPacket>,
    audio_packets: Vec<AudioPacket>,
    current_packet_idx: usize,
    rgba_buffer: Vec<u8>,
}

#[wasm_bindgen]
impl WebMobiPlayer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            decoder: None,
            adpcm: None,
            fastaudio: None,
            vx_audio: None,
            dsp_adpcm: None,
            moflex_pcm16: false,
            width: 0,
            height: 0,
            fps: 30.0,
            has_audio: false,
            sample_rate: 32000,
            channels: 2,
            container_type: ContainerType::Unknown,
            mods_pcm16: false,
            raw_data: Vec::new(),
            video_packets: Vec::new(),
            audio_packets: Vec::new(),
            current_packet_idx: 0,
            rgba_buffer: Vec::new(),
        }
    }

    pub fn load_file(&mut self, data: &[u8]) -> Result<bool, JsValue> {
        self.raw_data = data.to_vec();
        self.video_packets.clear();
        self.audio_packets.clear();
        self.current_packet_idx = 0;
        self.adpcm = None;
        self.fastaudio = None;
        self.vx_audio = None;
        self.dsp_adpcm = None;
        self.mods_pcm16 = false;
        self.moflex_pcm16 = false;

        if data.len() >= 8 && &data[0..8] == b"MODSN3\n\0" {
            self.container_type = ContainerType::Mods;
            let mut demux = ModsDemuxer::open(data).map_err(|e| JsValue::from_str(&e))?;
            self.width = demux.width;
            self.height = demux.height;
            self.fps = demux.fps;
            self.has_audio = demux.has_audio;
            self.sample_rate = demux.sample_rate;
            self.channels = demux.channels;

            if demux.audio_codec == 1 {
                self.adpcm = Some(AdpcmDecoder::new(demux.channels));
            } else if demux.audio_codec == 2 {
                self.fastaudio = Some(FastAudioDecoder::new(demux.channels));
            } else if demux.audio_codec == 3 {
                self.mods_pcm16 = true;
            } else if demux.audio_codec == 4 {
                self.vx_audio = VxAudioDecoder::new(demux.channels, &demux.sx_codebook);
            }

            while let Some((v_pkt, a_pkt)) = demux.next_chunk() {
                if !v_pkt.data.is_empty() {
                    self.video_packets.push(v_pkt);
                }
                if let Some(ap) = a_pkt {
                    self.audio_packets.push(ap);
                }
            }
        } else if data.len() >= 4 && &data[0..4] == b"MOC5" {
            self.container_type = ContainerType::Moc5;
            let mut demux = Moc5Demuxer::open(data).map_err(|e| JsValue::from_str(&e))?;
            self.width = demux.width;
            self.height = demux.height;
            self.fps = demux.fps;
            self.has_audio = demux.has_audio;
            self.sample_rate = demux.sample_rate;
            self.channels = demux.channels;

            if demux.audio_codec == 1 {
                self.adpcm = Some(AdpcmDecoder::new(demux.channels));
            } else if demux.audio_codec == 2 {
                self.fastaudio = Some(FastAudioDecoder::new(demux.channels));
            }

            while let Some((v_pkt, a_pkt)) = demux.next_chunk() {
                self.video_packets.push(v_pkt);
                if let Some(ap) = a_pkt {
                    self.audio_packets.push(ap);
                }
            }
        } else if data.len() >= 2 && u16::from_be_bytes([data[0], data[1]]) == 0x4C32 {
            self.container_type = ContainerType::Moflex;
            let mut demux = MoflexDemuxer::open(data).map_err(|e| JsValue::from_str(&e))?;
            self.width = demux.width;
            self.height = demux.height;
            self.fps = demux.fps;
            self.has_audio = demux.has_audio;
            self.sample_rate = demux.sample_rate;
            self.channels = demux.channels;

            if demux.audio_codec == 1 {
                self.adpcm = Some(AdpcmDecoder::new(demux.channels));
            } else if demux.audio_codec == 0 {
                self.fastaudio = Some(FastAudioDecoder::new(demux.channels));
            } else if demux.audio_codec == 2 {
                self.moflex_pcm16 = true;
            } else if demux.audio_codec == 4 {
                self.dsp_adpcm = Some(DspAdpcmDecoder::new(demux.channels));
            }

            while let Some((v_pkt, a_pkt)) = demux.next_packets() {
                if let Some(vp) = v_pkt {
                    self.video_packets.push(vp);
                }
                if let Some(ap) = a_pkt {
                    self.audio_packets.push(ap);
                }
            }
        } else {
            return Err(JsValue::from_str("Unsupported format or missing header"));
        }

        let dec_w = (self.width + 15) & !15;
        let dec_h = (self.height + 15) & !15;

        self.decoder = Some(MobiClipDecoder::new(dec_w, dec_h).map_err(|e| JsValue::from_str(&e))?);
        self.rgba_buffer = vec![0u8; dec_w * dec_h * 4];

        Ok(true)
    }

    pub fn get_width(&self) -> usize {
        self.width
    }

    pub fn get_height(&self) -> usize {
        self.height
    }

    pub fn get_fps(&self) -> f64 {
        self.fps
    }

    pub fn get_total_frames(&self) -> usize {
        self.video_packets.len()
    }

    pub fn get_current_frame(&self) -> usize {
        self.current_packet_idx
    }

    pub fn has_audio(&self) -> bool {
        self.has_audio
    }

    pub fn get_sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn get_channels(&self) -> usize {
        self.channels
    }

    pub fn seek_frame(&mut self, target_frame: usize) -> bool {
        if target_frame >= self.video_packets.len() {
            return false;
        }
        if self.video_packets[target_frame].decoded.is_some() {
            self.current_packet_idx = target_frame;
            return true;
        }

        let mut keyframe_idx = target_frame;
        while keyframe_idx > 0 && !self.video_packets[keyframe_idx].is_key {
            keyframe_idx -= 1;
        }

        let dec_w = (self.width + 15) & !15;
        let dec_h = (self.height + 15) & !15;
        if let Ok(dec) = MobiClipDecoder::new(dec_w, dec_h) {
            self.decoder = Some(dec);
        }

        for idx in keyframe_idx..target_frame {
            let pkt = &self.video_packets[idx].data;
            if let Some(dec) = &mut self.decoder {
                let _ = dec.decode_frame(pkt);
            }
        }
        self.current_packet_idx = target_frame;
        true
    }

    pub fn decode_next_frame(&mut self) -> Result<bool, JsValue> {
        if self.current_packet_idx >= self.video_packets.len() {
            return Ok(false);
        }

        let idx = self.current_packet_idx;
        self.current_packet_idx += 1;

        if let Some(frame) = self.video_packets[idx].decoded.clone() {
            if let Some(decoder) = &self.decoder {
                decoder.yuv_to_rgba(&frame, &mut self.rgba_buffer);
            }
            return Ok(true);
        }

        let pkt = self.video_packets[idx].data.clone();
        if let Some(decoder) = &mut self.decoder {
            let pic_idx = decoder.decode_frame(&pkt).map_err(|e| JsValue::from_str(&e))?;
            decoder.render_to_rgba(pic_idx, &mut self.rgba_buffer);
            Ok(true)
        } else {
            Err(JsValue::from_str("Decoder not initialized"))
        }
    }

    pub fn get_rgba_ptr(&self) -> *const u8 {
        self.rgba_buffer.as_ptr()
    }

    pub fn get_rgba_size(&self) -> usize {
        self.rgba_buffer.len()
    }

    pub fn decode_all_audio(&mut self) -> Vec<f32> {
        let mut audio_samples = Vec::new();

        if let Some(adpcm) = &mut self.adpcm {
            for pkt in &self.audio_packets {
                let s = match self.container_type {
                    ContainerType::Moflex => adpcm.decode_moflex(&pkt.data),
                    ContainerType::Mods => adpcm.decode_nds(&pkt.data, pkt.is_key),
                    _ => adpcm.decode_wii(&pkt.data),
                };
                audio_samples.extend_from_slice(&s);
            }
        } else if let Some(fastaudio) = &mut self.fastaudio {
            for pkt in &self.audio_packets {
                let s = fastaudio.decode(&pkt.data);
                audio_samples.extend_from_slice(&s);
            }
        } else if let Some(vx_audio) = &mut self.vx_audio {
            for pkt in &self.audio_packets {
                let s = vx_audio.decode(&pkt.data);
                audio_samples.extend_from_slice(&s);
            }
        } else if let Some(dsp_adpcm) = &mut self.dsp_adpcm {
            for pkt in &self.audio_packets {
                let s = dsp_adpcm.decode(&pkt.data);
                audio_samples.extend_from_slice(&s);
            }
        } else if self.moflex_pcm16 {
            let channels = self.channels.max(1);
            for pkt in &self.audio_packets {
                for s in pkt.data.chunks_exact(2 * channels) {
                    for ch in 0..channels {
                        let v = i16::from_le_bytes([s[2 * ch], s[2 * ch + 1]]);
                        audio_samples.push(v as f32 / 32768.0);
                    }
                }
            }
        } else if self.mods_pcm16 {
            let channels = self.channels.max(1);
            let block = channels * 512;
            for pkt in &self.audio_packets {
                for blk in pkt.data.chunks(block) {
                    let per_ch = blk.len() / channels / 2;
                    for s in 0..per_ch {
                        for ch in 0..channels {
                            let off = ch * per_ch * 2 + s * 2;
                            let v = i16::from_le_bytes([blk[off], blk[off + 1]]);
                            audio_samples.push(v as f32 / 32768.0);
                        }
                    }
                }
            }
        }

        audio_samples
    }
}
