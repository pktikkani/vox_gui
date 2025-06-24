use anyhow::{Result, Context};
use ffmpeg_next as ffmpeg;
use ffmpeg::{codec, decoder, frame, Packet};

pub struct H264Decoder {
    decoder: decoder::Video,
    frame: frame::Video,
    packet: Packet,
    width: u32,
    height: u32,
}

impl H264Decoder {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        ffmpeg::init().context("Failed to initialize FFmpeg")?;
        
        // Find H.264 decoder
        let codec = decoder::find(codec::Id::H264)
            .context("Failed to find H.264 decoder")?;
        
        let context = codec::context::Context::new_with_codec(codec);
        let decoder = context.decoder().video()?;
        
        Ok(Self {
            decoder,
            frame: frame::Video::empty(),
            packet: Packet::empty(),
            width,
            height,
        })
    }
    
    pub fn decode(&mut self, h264_data: &[u8]) -> Result<Option<Vec<u8>>> {
        // Create packet from H.264 data
        self.packet = Packet::copy(h264_data);
        
        // Send packet to decoder
        self.decoder.send_packet(&self.packet)?;
        
        // Try to receive decoded frame
        match self.decoder.receive_frame(&mut self.frame) {
            Ok(_) => {
                // Update dimensions if they changed
                self.width = self.frame.width();
                self.height = self.frame.height();
                
                // Convert YUV to RGB
                let rgb_data = self.yuv_to_rgb()?;
                Ok(Some(rgb_data))
            }
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {
                // Decoder needs more data
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }
    
    fn yuv_to_rgb(&self) -> Result<Vec<u8>> {
        let width = self.frame.width() as usize;
        let height = self.frame.height() as usize;
        let mut rgb = Vec::with_capacity(width * height * 3);
        
        // Get YUV planes
        let y_plane = self.frame.data(0);
        let u_plane = self.frame.data(1);
        let v_plane = self.frame.data(2);
        
        let y_stride = self.frame.stride(0);
        let u_stride = self.frame.stride(1);
        let v_stride = self.frame.stride(2);
        
        // Convert YUV420P to RGB
        for y in 0..height {
            for x in 0..width {
                let y_idx = y * y_stride + x;
                let u_idx = (y / 2) * u_stride + (x / 2);
                let v_idx = (y / 2) * v_stride + (x / 2);
                
                let y_val = y_plane[y_idx] as f32;
                let u_val = u_plane[u_idx] as f32 - 128.0;
                let v_val = v_plane[v_idx] as f32 - 128.0;
                
                // BT.601 conversion
                let r = (y_val + 1.402 * v_val).clamp(0.0, 255.0) as u8;
                let g = (y_val - 0.344 * u_val - 0.714 * v_val).clamp(0.0, 255.0) as u8;
                let b = (y_val + 1.772 * u_val).clamp(0.0, 255.0) as u8;
                
                rgb.push(r);
                rgb.push(g);
                rgb.push(b);
            }
        }
        
        Ok(rgb)
    }
    
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    
    pub fn flush(&mut self) -> Result<Vec<Vec<u8>>> {
        // Send EOF to decoder
        self.decoder.send_eof()?;
        
        let mut frames = Vec::new();
        
        // Drain remaining frames
        while self.decoder.receive_frame(&mut self.frame).is_ok() {
            if let Ok(rgb_data) = self.yuv_to_rgb() {
                frames.push(rgb_data);
            }
        }
        
        Ok(frames)
    }
}