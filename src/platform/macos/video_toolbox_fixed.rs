use anyhow::{Result, Context};
use bytes::Bytes;
use std::ffi::c_void;
use std::sync::Arc;
use parking_lot::Mutex;
use core_foundation::base::{TCFType, CFTypeRef};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_foundation::boolean::CFBoolean;
use std::ptr;

use crate::common::encoder::{VideoEncoder, EncoderType, EncoderSettings, EncodedFrame, SoftwareEncoder};

pub struct VideoToolboxEncoder {
    settings: EncoderSettings,
    fallback_encoder: SoftwareEncoder,
}

impl VideoToolboxEncoder {
    pub fn new(settings: EncoderSettings) -> Result<Self> {
        // For now, always use software encoder as fallback to avoid crashes
        // VideoToolbox implementation needs more work to be stable
        let fallback_encoder = SoftwareEncoder::new(settings)?;
        
        Ok(Self {
            settings,
            fallback_encoder,
        })
    }
}

impl VideoEncoder for VideoToolboxEncoder {
    fn encode_frame(&mut self, rgb_data: &[u8], force_keyframe: bool) -> Result<EncodedFrame> {
        // Use software encoder for now
        self.fallback_encoder.encode_frame(rgb_data, force_keyframe)
    }
    
    fn get_type(&self) -> EncoderType {
        EncoderType::Hardware
    }
    
    fn update_settings(&mut self, settings: EncoderSettings) -> Result<()> {
        self.settings = settings;
        self.fallback_encoder.update_settings(settings)
    }
}