use anyhow::{Result, Context};
use bytes::Bytes;
use ffmpeg_next as ffmpeg;
use ffmpeg::{codec, encoder, format, frame, Rational};

use crate::common::encoder::{VideoEncoder, EncoderType, EncoderSettings, EncodedFrame};

pub struct FFmpegHardwareEncoder {
    encoder: encoder::Video,
    frame: frame::Video,
    packet: codec::packet::Packet,
    settings: EncoderSettings,
    frame_count: u64,
    pts: i64,
    hw_frames_ctx: Option<*mut ffmpeg_sys_next::AVBufferRef>,
}

unsafe impl Send for FFmpegHardwareEncoder {}
unsafe impl Sync for FFmpegHardwareEncoder {}

impl FFmpegHardwareEncoder {
    pub fn new(settings: EncoderSettings) -> Result<Self> {
        ffmpeg::init().context("Failed to initialize FFmpeg")?;
        
        // Find H.264 encoder with hardware acceleration
        let codec = if cfg!(target_os = "macos") {
            // Try VideoToolbox first
            encoder::find_by_name("h264_videotoolbox")
                .or_else(|| encoder::find_by_name("hevc_videotoolbox"))
                .or_else(|| encoder::find(codec::Id::H264))
                .context("Failed to find H.264 encoder")?
        } else if cfg!(windows) {
            // Try hardware encoders on Windows
            encoder::find_by_name("h264_nvenc")
                .or_else(|| encoder::find_by_name("h264_qsv"))
                .or_else(|| encoder::find_by_name("h264_amf"))
                .or_else(|| encoder::find(codec::Id::H264))
                .context("Failed to find H.264 encoder")?
        } else {
            // Linux: try VAAPI or NVENC
            encoder::find_by_name("h264_vaapi")
                .or_else(|| encoder::find_by_name("h264_nvenc"))
                .or_else(|| encoder::find(codec::Id::H264))
                .context("Failed to find H.264 encoder")?
        };
        
        tracing::info!("Using encoder: {}", codec.name());
        
        let context = codec::context::Context::new_with_codec(codec);
        let mut encoder = context.encoder().video()?;
        
        // Configure encoder
        encoder.set_width(settings.width);
        encoder.set_height(settings.height);
        encoder.set_format(format::Pixel::YUV420P);
        encoder.set_time_base(Rational(1, settings.fps as i32));
        encoder.set_frame_rate(Some(Rational(settings.fps as i32, 1)));
        encoder.set_bit_rate(settings.bitrate as usize);
        encoder.set_max_b_frames(0); // No B-frames for low latency
        encoder.set_gop(settings.keyframe_interval);
        
        // Set quality/speed tradeoff (removed for now, ffmpeg-next doesn't expose priv_data)
        
        let encoder = encoder.open()?;
        
        // Create frame for input
        let mut frame = frame::Video::new(format::Pixel::YUV420P, settings.width, settings.height);
        frame.set_pts(Some(0));
        
        Ok(Self {
            encoder,
            frame,
            packet: codec::packet::Packet::empty(),
            settings,
            frame_count: 0,
            pts: 0,
            hw_frames_ctx: None,
        })
    }
    
    fn rgb_to_yuv420p(&mut self, rgb_data: &[u8]) -> Result<()> {
        let width = self.settings.width as usize;
        let height = self.settings.height as usize;
        
        // Get strides first
        let y_stride = self.frame.stride(0);
        let u_stride = self.frame.stride(1);
        let v_stride = self.frame.stride(2);
        
        // Process Y plane
        {
            let y_plane = self.frame.data_mut(0);
            for y in 0..height {
                for x in 0..width {
                    let rgb_idx = (y * width + x) * 3;
                    let r = rgb_data[rgb_idx] as f32;
                    let g = rgb_data[rgb_idx + 1] as f32;
                    let b = rgb_data[rgb_idx + 2] as f32;
                    
                    // BT.601 conversion
                    let y_val = (0.299 * r + 0.587 * g + 0.114 * b) as u8;
                    y_plane[y * y_stride + x] = y_val;
                }
            }
        }
        
        // Process U plane
        {
            let u_plane = self.frame.data_mut(1);
            for y in (0..height).step_by(2) {
                for x in (0..width).step_by(2) {
                    let rgb_idx = (y * width + x) * 3;
                    let r = rgb_data[rgb_idx] as f32;
                    let g = rgb_data[rgb_idx + 1] as f32;
                    let b = rgb_data[rgb_idx + 2] as f32;
                    
                    let u_idx = (y / 2) * u_stride + (x / 2);
                    u_plane[u_idx] = ((-0.169 * r - 0.331 * g + 0.500 * b) + 128.0) as u8;
                }
            }
        }
        
        // Process V plane
        {
            let v_plane = self.frame.data_mut(2);
            for y in (0..height).step_by(2) {
                for x in (0..width).step_by(2) {
                    let rgb_idx = (y * width + x) * 3;
                    let r = rgb_data[rgb_idx] as f32;
                    let g = rgb_data[rgb_idx + 1] as f32;
                    let b = rgb_data[rgb_idx + 2] as f32;
                    
                    let v_idx = (y / 2) * v_stride + (x / 2);
                    v_plane[v_idx] = ((0.500 * r - 0.419 * g - 0.081 * b) + 128.0) as u8;
                }
            }
        }
        
        Ok(())
    }
}

impl Drop for FFmpegHardwareEncoder {
    fn drop(&mut self) {
        // Flush encoder
        let _ = self.encoder.send_eof();
        while self.encoder.receive_packet(&mut self.packet).is_ok() {
            // Drain remaining packets
        }
    }
}

impl VideoEncoder for FFmpegHardwareEncoder {
    fn encode_frame(&mut self, rgb_data: &[u8], force_keyframe: bool) -> Result<EncodedFrame> {
        // Convert RGB to YUV420P
        self.rgb_to_yuv420p(rgb_data)?;
        
        // Set frame properties
        self.frame.set_pts(Some(self.pts));
        self.pts += 1;
        
        if force_keyframe {
            // Force keyframe - FFmpeg doesn't expose direct keyframe forcing in this API
            // The encoder will decide based on GOP settings
        }
        
        // Send frame to encoder
        self.encoder.send_frame(&self.frame)?;
        
        // Try to receive packet
        let mut encoded_data = Vec::new();
        let mut is_keyframe = false;
        
        while self.encoder.receive_packet(&mut self.packet).is_ok() {
            encoded_data.extend_from_slice(self.packet.data().unwrap());
            is_keyframe = self.packet.is_key();
        }
        
        self.frame_count += 1;
        
        if encoded_data.is_empty() {
            // Encoder might be buffering, return empty frame
            return Ok(EncodedFrame {
                data: Bytes::new(),
                is_keyframe: false,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            });
        }
        
        Ok(EncodedFrame {
            data: Bytes::from(encoded_data),
            is_keyframe,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        })
    }
    
    fn get_type(&self) -> EncoderType {
        EncoderType::Hardware
    }
    
    fn update_settings(&mut self, settings: EncoderSettings) -> Result<()> {
        // FFmpeg doesn't support dynamic reconfiguration easily
        // Would need to recreate encoder
        self.settings = settings;
        Ok(())
    }
}