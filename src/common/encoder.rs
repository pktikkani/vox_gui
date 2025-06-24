use anyhow::Result;
use bytes::Bytes;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EncoderType {
    Software,
    Hardware,
}

#[derive(Debug, Clone, Copy)]
pub struct EncoderSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
    pub keyframe_interval: u32,
}

pub trait VideoEncoder: Send + Sync {
    fn encode_frame(&mut self, rgb_data: &[u8], force_keyframe: bool) -> Result<EncodedFrame>;
    fn get_type(&self) -> EncoderType;
    fn update_settings(&mut self, settings: EncoderSettings) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct EncodedFrame {
    pub data: Bytes,
    pub is_keyframe: bool,
    pub timestamp: u64,
}

// Software encoder using WebP for now (can be replaced with VP8/VP9)
pub struct SoftwareEncoder {
    settings: EncoderSettings,
    frame_count: u64,
}

impl SoftwareEncoder {
    pub fn new(settings: EncoderSettings) -> Result<Self> {
        Ok(Self {
            settings,
            frame_count: 0,
        })
    }
}

impl VideoEncoder for SoftwareEncoder {
    fn encode_frame(&mut self, rgb_data: &[u8], force_keyframe: bool) -> Result<EncodedFrame> {
        let is_keyframe = force_keyframe || self.frame_count % self.settings.keyframe_interval as u64 == 0;
        self.frame_count += 1;
        
        // Use WebP lossless encoding for better quality
        let encoder = webp::Encoder::from_rgb(
            rgb_data,
            self.settings.width,
            self.settings.height,
        );
        
        // Use lossless encoding to avoid compression artifacts
        let encoded = encoder.encode_lossless();
        
        Ok(EncodedFrame {
            data: Bytes::from(encoded.to_vec()),
            is_keyframe,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        })
    }
    
    fn get_type(&self) -> EncoderType {
        EncoderType::Software
    }
    
    fn update_settings(&mut self, settings: EncoderSettings) -> Result<()> {
        self.settings = settings;
        Ok(())
    }
}

// Use FFmpeg for hardware encoding on all platforms
pub use crate::common::ffmpeg_encoder::FFmpegHardwareEncoder as HardwareEncoder;

// Encoder factory
pub struct EncoderFactory;

impl EncoderFactory {
    pub fn create_encoder(
        encoder_type: EncoderType,
        settings: EncoderSettings,
    ) -> Result<Box<dyn VideoEncoder>> {
        match encoder_type {
            EncoderType::Software => {
                Ok(Box::new(SoftwareEncoder::new(settings)?))
            }
            EncoderType::Hardware => {
                match HardwareEncoder::new(settings) {
                    Ok(encoder) => Ok(Box::new(encoder)),
                    Err(e) => {
                        tracing::warn!("Hardware encoder failed: {}, falling back to software", e);
                        Ok(Box::new(SoftwareEncoder::new(settings)?))
                    }
                }
            }
        }
    }
    
    pub fn is_hardware_available() -> bool {
        #[cfg(any(target_os = "macos", windows))]
        {
            // Try to create a test encoder
            let test_settings = EncoderSettings {
                width: 1920,
                height: 1080,
                fps: 30,
                bitrate: 5_000_000,
                keyframe_interval: 60,
            };
            match HardwareEncoder::new(test_settings) {
                Ok(_) => {
                    tracing::info!("Hardware encoder is available");
                    true
                }
                Err(e) => {
                    tracing::info!("Hardware encoder not available: {}", e);
                    false
                }
            }
        }
        
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            false
        }
    }
}