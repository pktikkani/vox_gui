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
use std::slice;

use crate::common::encoder::{VideoEncoder, EncoderType, EncoderSettings, EncodedFrame};

#[link(name = "VideoToolbox", kind = "framework")]
extern "C" {
    fn VTSessionSetProperty(
        session: *mut c_void,
        property_key: CFStringRef,
        property_value: CFTypeRef,
    ) -> i32;
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
    fn CFRelease(cf: *const c_void);
}

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    fn CVPixelBufferCreateWithBytes(
        allocator: *const c_void,
        width: usize,
        height: usize,
        pixel_format_type: u32,
        base_address: *mut c_void,
        bytes_per_row: usize,
        release_callback: *const c_void,
        release_ref_con: *mut c_void,
        buffer_attributes: CFDictionaryRef,
        pixel_buffer_out: *mut *mut c_void,
    ) -> i32;
    
    fn CVPixelBufferRelease(pixel_buffer: *mut c_void);
}

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetDataBuffer(sample_buffer: *mut c_void) -> *mut c_void;
    fn CMBlockBufferGetDataPointer(
        block_buffer: *mut c_void,
        offset: usize,
        length_at_offset_out: *mut usize,
        total_length_out: *mut usize,
        data_pointer_out: *mut *mut u8,
    ) -> i32;
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
const K_CVPIXEL_FORMAT_TYPE_32_BGRA: u32 = 0x42475241; // 'BGRA'
const K_CMSAMPLE_BUFFER_NO_ERROR: i32 = 0;
const K_CVRETURN_SUCCESS: i32 = 0;

// VideoToolbox property keys
const K_VT_COMPRESSION_PROPERTY_KEY_REAL_TIME: &str = "RealTime";
const K_VT_COMPRESSION_PROPERTY_KEY_PROFILE_LEVEL: &str = "ProfileLevel";
const K_VT_COMPRESSION_PROPERTY_KEY_AVERAGE_BITRATE: &str = "AverageBitRate";
const K_VT_COMPRESSION_PROPERTY_KEY_EXPECTED_FRAME_RATE: &str = "ExpectedFrameRate";
const K_VT_COMPRESSION_PROPERTY_KEY_MAX_KEY_FRAME_INTERVAL: &str = "MaxKeyFrameInterval";
const K_VT_COMPRESSION_PROPERTY_KEY_ALLOW_FRAME_REORDERING: &str = "AllowFrameReordering";
const K_VT_PROFILE_LEVEL_H264_BASELINE_AUTO_LEVEL: &str = "H264_Baseline_AutoLevel";

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
                CFRelease(session);
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
        
        // Clear output buffer
        self.output_buffer.lock().clear();
        
        unsafe {
            // Create CVPixelBuffer
            let pixel_buffer = create_pixel_buffer_from_bgra(
                &bgra_data,
                self.settings.width,
                self.settings.height,
            )?;
            
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
                ptr::null()
            };
            
            let status = VTCompressionSessionEncodeFrame(
                session_ptr,
                pixel_buffer,
                timestamp,
                duration,
                frame_props as *const c_void,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            
            // Release pixel buffer
            CVPixelBufferRelease(pixel_buffer);
            
            if status != 0 {
                return Err(anyhow::anyhow!("Failed to encode frame: {}", status));
            }
            
            // Force completion
            VTCompressionSessionCompleteFrames(session_ptr, timestamp);
        }
        
        self.frame_count += 1;
        
        // Wait a bit for the callback to populate the buffer
        std::thread::sleep(std::time::Duration::from_millis(1));
        
        // Get encoded data
        let encoded_data = self.output_buffer.lock().clone();
        
        if encoded_data.is_empty() {
            return Err(anyhow::anyhow!("No encoded data received from VideoToolbox"));
        }
        
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

struct PixelBufferContext {
    data: Vec<u8>,
}

unsafe extern "C" fn pixel_buffer_release_callback(
    release_ref_con: *mut c_void,
    _base_address: *const c_void,
) {
    if !release_ref_con.is_null() {
        let _context = Box::from_raw(release_ref_con as *mut PixelBufferContext);
        // Context is dropped here, releasing the data
    }
}

unsafe fn create_pixel_buffer_from_bgra(
    bgra_data: &[u8],
    width: u32,
    height: u32,
) -> Result<*mut c_void> {
    let mut pixel_buffer: *mut c_void = ptr::null_mut();
    let bytes_per_row = (width * 4) as usize;
    
    // Create a context to hold the data
    let context = Box::new(PixelBufferContext {
        data: bgra_data.to_vec(),
    });
    let context_ptr = Box::into_raw(context);
    
    let status = CVPixelBufferCreateWithBytes(
        ptr::null(), // Use default allocator
        width as usize,
        height as usize,
        K_CVPIXEL_FORMAT_TYPE_32_BGRA,
        (*context_ptr).data.as_mut_ptr() as *mut c_void,
        bytes_per_row,
        pixel_buffer_release_callback as *const c_void,
        context_ptr as *mut c_void,
        ptr::null(), // No buffer attributes
        &mut pixel_buffer,
    );
    
    if status != K_CVRETURN_SUCCESS {
        // Clean up context if creation failed
        let _ = Box::from_raw(context_ptr);
        return Err(anyhow::anyhow!("Failed to create CVPixelBuffer: {}", status));
    }
    
    Ok(pixel_buffer)
}

unsafe fn configure_session(session: *mut c_void, bitrate: u32, fps: u32) -> Result<()> {
    // Set real-time encoding
    let real_time_key = CFString::new(K_VT_COMPRESSION_PROPERTY_KEY_REAL_TIME);
    let real_time_value = CFBoolean::true_value();
    VTSessionSetProperty(session, real_time_key.as_concrete_TypeRef(), real_time_value.as_CFTypeRef());
    
    // Set profile level
    let profile_key = CFString::new(K_VT_COMPRESSION_PROPERTY_KEY_PROFILE_LEVEL);
    let profile_value = CFString::new(K_VT_PROFILE_LEVEL_H264_BASELINE_AUTO_LEVEL);
    VTSessionSetProperty(session, profile_key.as_concrete_TypeRef(), profile_value.as_CFTypeRef());
    
    // Set bitrate
    let bitrate_key = CFString::new(K_VT_COMPRESSION_PROPERTY_KEY_AVERAGE_BITRATE);
    let bitrate_value = CFNumber::from(bitrate as i32);
    VTSessionSetProperty(session, bitrate_key.as_concrete_TypeRef(), bitrate_value.as_CFTypeRef());
    
    // Set frame rate
    let fps_key = CFString::new(K_VT_COMPRESSION_PROPERTY_KEY_EXPECTED_FRAME_RATE);
    let fps_value = CFNumber::from(fps as i32);
    VTSessionSetProperty(session, fps_key.as_concrete_TypeRef(), fps_value.as_CFTypeRef());
    
    // Set keyframe interval
    let keyframe_key = CFString::new(K_VT_COMPRESSION_PROPERTY_KEY_MAX_KEY_FRAME_INTERVAL);
    let keyframe_value = CFNumber::from(fps as i32 * 2); // Keyframe every 2 seconds
    VTSessionSetProperty(session, keyframe_key.as_concrete_TypeRef(), keyframe_value.as_CFTypeRef());
    
    // Disable frame reordering for lower latency
    let reordering_key = CFString::new(K_VT_COMPRESSION_PROPERTY_KEY_ALLOW_FRAME_REORDERING);
    let reordering_value = CFBoolean::false_value();
    VTSessionSetProperty(session, reordering_key.as_concrete_TypeRef(), reordering_value.as_CFTypeRef());
    
    Ok(())
}

unsafe fn create_keyframe_properties() -> CFDictionaryRef {
    let key = CFString::new("ForceKeyFrame");
    let value = CFBoolean::true_value();
    
    let dict = CFDictionary::from_CFType_pairs(&[
        (key.as_CFType(), value.as_CFType()),
    ]);
    
    dict.as_concrete_TypeRef()
}

unsafe fn extract_data_from_sample_buffer(sample_buffer: *mut c_void) -> Vec<u8> {
    let block_buffer = CMSampleBufferGetDataBuffer(sample_buffer);
    if block_buffer.is_null() {
        return Vec::new();
    }
    
    let mut data_pointer: *mut u8 = ptr::null_mut();
    let mut total_length: usize = 0;
    
    let status = CMBlockBufferGetDataPointer(
        block_buffer,
        0,
        ptr::null_mut(),
        &mut total_length,
        &mut data_pointer,
    );
    
    if status != K_CMSAMPLE_BUFFER_NO_ERROR || data_pointer.is_null() {
        return Vec::new();
    }
    
    // Copy the data
    let data_slice = slice::from_raw_parts(data_pointer, total_length);
    data_slice.to_vec()
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