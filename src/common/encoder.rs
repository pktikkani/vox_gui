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
        
        // Use WebP encoding for now
        let encoder = webp::Encoder::from_rgb(
            rgb_data,
            self.settings.width,
            self.settings.height,
        );
        
        let quality = if is_keyframe { 95.0 } else { 85.0 };
        let encoded = encoder.encode(quality);
        
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

// Platform-specific hardware encoders
#[cfg(target_os = "macos")]
pub mod hardware {
    use super::*;
    use std::ffi::c_void;
    
    // VideoToolbox encoder for macOS
    pub struct VideoToolboxEncoder {
        settings: EncoderSettings,
        #[allow(dead_code)]
        session: Option<*mut c_void>, // VTCompressionSessionRef - placeholder for future implementation
    }
    
    unsafe impl Send for VideoToolboxEncoder {}
    unsafe impl Sync for VideoToolboxEncoder {}
    
    impl VideoToolboxEncoder {
        pub fn new(settings: EncoderSettings) -> Result<Self> {
            // For now, fall back to software encoding
            // Full VideoToolbox implementation would require Objective-C bindings
            Ok(Self {
                settings,
                session: None,
            })
        }
    }
    
    impl VideoEncoder for VideoToolboxEncoder {
        fn encode_frame(&mut self, rgb_data: &[u8], force_keyframe: bool) -> Result<EncodedFrame> {
            // Fallback to software encoding for now
            let mut sw_encoder = SoftwareEncoder::new(self.settings)?;
            sw_encoder.encode_frame(rgb_data, force_keyframe)
        }
        
        fn get_type(&self) -> EncoderType {
            EncoderType::Hardware
        }
        
        fn update_settings(&mut self, settings: EncoderSettings) -> Result<()> {
            self.settings = settings;
            Ok(())
        }
    }
}

#[cfg(windows)]
pub mod hardware {
    use super::*;
    
    // NVENC or Quick Sync encoder for Windows
    pub struct WindowsHardwareEncoder {
        settings: EncoderSettings,
    }
    
    impl WindowsHardwareEncoder {
        pub fn new(settings: EncoderSettings) -> Result<Self> {
            Ok(Self { settings })
        }
    }
    
    impl VideoEncoder for WindowsHardwareEncoder {
        fn encode_frame(&mut self, rgb_data: &[u8], force_keyframe: bool) -> Result<EncodedFrame> {
            // Fallback to software encoding for now
            let mut sw_encoder = SoftwareEncoder::new(self.settings)?;
            sw_encoder.encode_frame(rgb_data, force_keyframe)
        }
        
        fn get_type(&self) -> EncoderType {
            EncoderType::Hardware
        }
        
        fn update_settings(&mut self, settings: EncoderSettings) -> Result<()> {
            self.settings = settings;
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
pub mod hardware {
    use super::*;
    
    // VAAPI encoder for Linux
    pub struct VaapiEncoder {
        settings: EncoderSettings,
    }
    
    impl VaapiEncoder {
        pub fn new(settings: EncoderSettings) -> Result<Self> {
            Ok(Self { settings })
        }
    }
    
    impl VideoEncoder for VaapiEncoder {
        fn encode_frame(&mut self, rgb_data: &[u8], force_keyframe: bool) -> Result<EncodedFrame> {
            // Fallback to software encoding for now
            let mut sw_encoder = SoftwareEncoder::new(self.settings)?;
            sw_encoder.encode_frame(rgb_data, force_keyframe)
        }
        
        fn get_type(&self) -> EncoderType {
            EncoderType::Hardware
        }
        
        fn update_settings(&mut self, settings: EncoderSettings) -> Result<()> {
            self.settings = settings;
            Ok(())
        }
    }
}

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
                #[cfg(target_os = "macos")]
                {
                    match hardware::VideoToolboxEncoder::new(settings) {
                        Ok(encoder) => Ok(Box::new(encoder)),
                        Err(_) => Ok(Box::new(SoftwareEncoder::new(settings)?)),
                    }
                }
                
                #[cfg(windows)]
                {
                    match hardware::WindowsHardwareEncoder::new(settings) {
                        Ok(encoder) => Ok(Box::new(encoder)),
                        Err(_) => Ok(Box::new(SoftwareEncoder::new(settings)?)),
                    }
                }
                
                #[cfg(target_os = "linux")]
                {
                    match hardware::VaapiEncoder::new(settings) {
                        Ok(encoder) => Ok(Box::new(encoder)),
                        Err(_) => Ok(Box::new(SoftwareEncoder::new(settings)?)),
                    }
                }
                
                #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
                {
                    Ok(Box::new(SoftwareEncoder::new(settings)?))
                }
            }
        }
    }
    
    pub fn is_hardware_available() -> bool {
        // In a real implementation, this would check for hardware encoder availability
        false
    }
}