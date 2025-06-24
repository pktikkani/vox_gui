use anyhow::{Result, Context};
use bytes::Bytes;
use std::ffi::c_void;
use std::sync::Arc;
use parking_lot::Mutex;
// Temporarily commented out until proper implementation
// use objc::{msg_send, sel, sel_impl};
// use objc::runtime::{Object, YES, NO};
// use core_foundation::base::{TCFType};
// use core_foundation::dictionary::CFDictionary;
// use core_foundation::number::CFNumber;
// use core_foundation::string::CFString;
// use core_video::pixel_buffer::CVPixelBuffer;
// use block::ConcreteBlock;

use crate::common::encoder::{VideoEncoder, EncoderType, EncoderSettings, EncodedFrame};

#[link(name = "VideoToolbox", kind = "framework")]
extern "C" {
    fn VTCompressionSessionCreate(
        allocator: *const c_void,
        width: i32,
        height: i32,
        codec_type: u32,
        encoder_specification: *const c_void,
        source_image_buffer_attributes: *const c_void,
        compressed_data_allocator: *const c_void,
        output_callback: *const c_void,
        output_callback_ref_con: *mut c_void,
        compression_session_out: *mut *mut c_void,
    ) -> i32;

    fn VTCompressionSessionEncodeFrame(
        session: *mut c_void,
        image_buffer: *const c_void,
        presentation_time_stamp: CMTime,
        duration: CMTime,
        frame_properties: *const c_void,
        source_frame_ref_con: *mut c_void,
        info_flags_out: *mut u32,
    ) -> i32;

    fn VTCompressionSessionCompleteFrames(
        session: *mut c_void,
        complete_until_presentation_time_stamp: CMTime,
    ) -> i32;

    fn VTCompressionSessionInvalidate(session: *mut c_void);
    // fn CFRelease(cf: *const c_void); // Already imported from core_foundation
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CMTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

const K_CMVIDEO_CODEC_TYPE_H264: u32 = 0x61766331; // 'avc1'
#[allow(dead_code)]
const K_CVPIXEL_FORMAT_TYPE_32_BGRA: u32 = 0x42475241; // 'BGRA'

pub struct VideoToolboxEncoder {
    settings: EncoderSettings,
    session: Arc<Mutex<Option<*mut c_void>>>,
    frame_count: u64,
    output_buffer: Arc<Mutex<Vec<u8>>>,
}

unsafe impl Send for VideoToolboxEncoder {}
unsafe impl Sync for VideoToolboxEncoder {}

impl VideoToolboxEncoder {
    pub fn new(settings: EncoderSettings) -> Result<Self> {
        let encoder = Self {
            settings,
            session: Arc::new(Mutex::new(None)),
            frame_count: 0,
            output_buffer: Arc::new(Mutex::new(Vec::new())),
        };
        
        encoder.create_session()?;
        Ok(encoder)
    }
    
    fn create_session(&self) -> Result<()> {
        unsafe {
            let output_buffer = self.output_buffer.clone();
            
            // Create output callback
            let callback = Box::new(move |_status: i32, _flags: u32, sample_buffer: *mut c_void| {
                if !sample_buffer.is_null() {
                    // Extract encoded data from sample buffer
                    let data = extract_data_from_sample_buffer(sample_buffer);
                    if !data.is_empty() {
                        let mut buffer = output_buffer.lock();
                        buffer.extend_from_slice(&data);
                    }
                }
            });
            
            let callback_ptr = Box::into_raw(callback) as *mut c_void;
            
            let mut session_ptr: *mut c_void = std::ptr::null_mut();
            
            let status = VTCompressionSessionCreate(
                std::ptr::null(), // Use default allocator
                self.settings.width as i32,
                self.settings.height as i32,
                K_CMVIDEO_CODEC_TYPE_H264,
                std::ptr::null(), // Use default encoder
                std::ptr::null(), // No source attributes
                std::ptr::null(), // Use default allocator
                output_callback_trampoline as *const c_void,
                callback_ptr,
                &mut session_ptr,
            );
            
            if status != 0 {
                return Err(anyhow::anyhow!("Failed to create VideoToolbox session: {}", status));
            }
            
            // Configure session for real-time encoding
            configure_session(session_ptr, self.settings.bitrate, self.settings.fps)?;
            
            *self.session.lock() = Some(session_ptr);
            Ok(())
        }
    }
}

impl Drop for VideoToolboxEncoder {
    fn drop(&mut self) {
        if let Some(session) = self.session.lock().take() {
            unsafe {
                VTCompressionSessionInvalidate(session);
                // CFRelease(session); // TODO: Implement proper cleanup
            }
        }
    }
}

impl VideoEncoder for VideoToolboxEncoder {
    fn encode_frame(&mut self, rgb_data: &[u8], force_keyframe: bool) -> Result<EncodedFrame> {
        let session = self.session.lock();
        let session_ptr = *session.as_ref()
            .context("VideoToolbox session not initialized")?;
        
        // Convert RGB to BGRA (VideoToolbox requirement)
        let bgra_data = rgb_to_bgra(rgb_data, self.settings.width, self.settings.height);
        
        // Create CVPixelBuffer
        let pixel_buffer = unsafe {
            create_pixel_buffer_from_bgra(
                &bgra_data,
                self.settings.width,
                self.settings.height,
            )?
        };
        
        // Clear output buffer
        self.output_buffer.lock().clear();
        
        unsafe {
            let timestamp = CMTime {
                value: self.frame_count as i64,
                timescale: self.settings.fps as i32,
                flags: 1,
                epoch: 0,
            };
            
            let duration = CMTime {
                value: 1,
                timescale: self.settings.fps as i32,
                flags: 1,
                epoch: 0,
            };
            
            // Set frame properties for keyframe if needed
            let frame_props = if force_keyframe {
                create_keyframe_properties()
            } else {
                std::ptr::null()
            };
            
            let status = VTCompressionSessionEncodeFrame(
                session_ptr,
                pixel_buffer,
                timestamp,
                duration,
                frame_props,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            
            if status != 0 {
                return Err(anyhow::anyhow!("Failed to encode frame: {}", status));
            }
            
            // Force completion
            VTCompressionSessionCompleteFrames(session_ptr, timestamp);
        }
        
        self.frame_count += 1;
        
        // Get encoded data
        let encoded_data = self.output_buffer.lock().clone();
        
        Ok(EncodedFrame {
            data: Bytes::from(encoded_data),
            is_keyframe: force_keyframe || self.frame_count % self.settings.keyframe_interval as u64 == 1,
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
        self.settings = settings;
        // Recreate session with new settings
        if let Some(old_session) = self.session.lock().take() {
            unsafe {
                VTCompressionSessionInvalidate(old_session);
                // CFRelease(old_session); // TODO: Implement proper cleanup
            }
        }
        self.create_session()
    }
}

// Helper functions
fn rgb_to_bgra(rgb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut bgra = Vec::with_capacity((width * height * 4) as usize);
    
    for chunk in rgb.chunks_exact(3) {
        bgra.push(chunk[2]); // B
        bgra.push(chunk[1]); // G
        bgra.push(chunk[0]); // R
        bgra.push(255);      // A
    }
    
    bgra
}

unsafe fn create_pixel_buffer_from_bgra(
    bgra_data: &[u8],
    _width: u32,
    _height: u32,
) -> Result<*const c_void> {
    // This is a simplified version - in production you'd use CVPixelBufferCreateWithBytes
    // For now, we'll pass the data directly
    Ok(bgra_data.as_ptr() as *const c_void)
}

unsafe fn configure_session(_session: *mut c_void, _bitrate: u32, _fps: u32) -> Result<()> {
    // Configure encoder properties
    // This would use VTSessionSetProperty in a real implementation
    Ok(())
}

unsafe fn create_keyframe_properties() -> *const c_void {
    // Create dictionary with kVTEncodeFrameOptionKey_ForceKeyFrame
    std::ptr::null()
}

unsafe fn extract_data_from_sample_buffer(_sample_buffer: *mut c_void) -> Vec<u8> {
    // Extract H.264 data from CMSampleBuffer
    // This is a placeholder - real implementation would use CMSampleBufferGetDataBuffer
    Vec::new()
}

unsafe extern "C" fn output_callback_trampoline(
    output_callback_ref_con: *mut c_void,
    _source_frame_ref_con: *mut c_void,
    status: i32,
    info_flags: u32,
    sample_buffer: *mut c_void,
) {
    let callback = output_callback_ref_con as *mut Box<dyn Fn(i32, u32, *mut c_void)>;
    if !callback.is_null() {
        (*callback)(status, info_flags, sample_buffer);
    }
}