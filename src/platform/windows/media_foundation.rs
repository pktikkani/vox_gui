use anyhow::{Result, Context};
use bytes::Bytes;
use windows::{
    core::*,
    Win32::{
        Media::MediaFoundation::*,
        System::Com::*,
        Foundation::*,
    },
};
use std::sync::Arc;
use parking_lot::Mutex;

use crate::common::encoder::{VideoEncoder, EncoderType, EncoderSettings, EncodedFrame};

pub struct MediaFoundationEncoder {
    settings: EncoderSettings,
    transform: Option<IMFTransform>,
    frame_count: u64,
    output_buffer: Arc<Mutex<Vec<u8>>>,
}

unsafe impl Send for MediaFoundationEncoder {}
unsafe impl Sync for MediaFoundationEncoder {}

impl MediaFoundationEncoder {
    pub fn new(settings: EncoderSettings) -> Result<Self> {
        unsafe {
            // Initialize Media Foundation
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
            
            let encoder = Self {
                settings,
                transform: None,
                frame_count: 0,
                output_buffer: Arc::new(Mutex::new(Vec::with_capacity(1024 * 1024))),
            };
            
            encoder.create_encoder()?;
            Ok(encoder)
        }
    }
    
    fn create_encoder(&self) -> Result<()> {
        unsafe {
            // Create H.264 encoder
            let transform: IMFTransform = CoCreateInstance(
                &CLSID_MSH264EncoderMFT,
                None,
                CLSCTX_INPROC_SERVER,
            )?;
            
            // Configure input type (RGB32)
            let input_type = create_video_type(
                &MFVideoFormat_RGB32,
                self.settings.width,
                self.settings.height,
                self.settings.fps,
            )?;
            
            transform.SetInputType(0, &input_type, 0)?;
            
            // Configure output type (H.264)
            let output_type = create_video_type(
                &MFVideoFormat_H264,
                self.settings.width,
                self.settings.height,
                self.settings.fps,
            )?;
            
            // Set bitrate
            output_type.SetUINT32(&MF_MT_AVG_BITRATE, self.settings.bitrate)?;
            
            transform.SetOutputType(0, &output_type, 0)?;
            
            // Set encoder properties for low latency
            if let Ok(codec_api) = transform.cast::<ICodecAPI>() {
                // Enable low latency mode
                let low_latency = VARIANT {
                    Anonymous: VARIANT_0 {
                        vt: VT_BOOL,
                        wReserved1: 0,
                        wReserved2: 0,
                        wReserved3: 0,
                        Anonymous: VARIANT_0_0 {
                            boolVal: VARIANT_TRUE,
                        },
                    },
                };
                let _ = codec_api.SetValue(&CODECAPI_AVLowLatencyMode, &low_latency);
                
                // Set rate control mode
                let rate_control = VARIANT {
                    Anonymous: VARIANT_0 {
                        vt: VT_UI4,
                        wReserved1: 0,
                        wReserved2: 0,
                        wReserved3: 0,
                        Anonymous: VARIANT_0_0 {
                            ulVal: 3, // CBR
                        },
                    },
                };
                let _ = codec_api.SetValue(&CODECAPI_AVEncCommonRateControlMode, &rate_control);
            }
            
            // Start the encoder
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
            
            *self.transform.lock() = Some(transform);
            Ok(())
        }
    }
}

impl Drop for MediaFoundationEncoder {
    fn drop(&mut self) {
        unsafe {
            if let Some(transform) = self.transform.take() {
                let _ = transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
                let _ = transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
            }
            let _ = MFShutdown();
        }
    }
}

impl VideoEncoder for MediaFoundationEncoder {
    fn encode_frame(&mut self, rgb_data: &[u8], force_keyframe: bool) -> Result<EncodedFrame> {
        let transform = self.transform.as_ref()
            .context("Media Foundation encoder not initialized")?;
        
        unsafe {
            // Create input sample
            let sample = create_sample_from_rgb(
                rgb_data,
                self.settings.width,
                self.settings.height,
            )?;
            
            // Set timestamp
            let timestamp = (self.frame_count * 10_000_000) / self.settings.fps as u64;
            sample.SetSampleTime(timestamp as i64)?;
            sample.SetSampleDuration(10_000_000 / self.settings.fps as i64)?;
            
            // Set keyframe if needed
            if force_keyframe {
                sample.SetUINT32(&MFSampleExtension_CleanPoint, 1)?;
            }
            
            // Process input
            transform.ProcessInput(0, &sample, 0)?;
            
            // Get output
            let mut output_buffer = Vec::new();
            
            loop {
                let mut output_info = MFT_OUTPUT_STREAM_INFO::default();
                transform.GetOutputStreamInfo(0, &mut output_info)?;
                
                let buffer: IMFMediaBuffer = MFCreateMemoryBuffer(output_info.cbSize)?;
                let output_sample: IMFSample = MFCreateSample()?;
                output_sample.AddBuffer(&buffer)?;
                
                let mut output = MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: ManuallyDrop::new(Some(output_sample.clone())),
                    dwStatus: 0,
                    pEvents: None,
                };
                
                let mut status = 0;
                let result = transform.ProcessOutput(0, &mut [output], &mut status);
                
                match result {
                    Ok(()) => {
                        // Extract data from sample
                        if let Some(sample) = output.pSample.as_ref() {
                            let buffer = sample.GetBufferByIndex(0)?;
                            let mut ptr = std::ptr::null_mut();
                            let mut max_len = 0;
                            let mut current_len = 0;
                            
                            buffer.Lock(&mut ptr, &mut max_len, &mut current_len)?;
                            if current_len > 0 {
                                output_buffer.extend_from_slice(
                                    std::slice::from_raw_parts(ptr, current_len as usize)
                                );
                            }
                            buffer.Unlock()?;
                        }
                    }
                    Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => break,
                    Err(e) => return Err(e.into()),
                }
                
                // Clean up
                ManuallyDrop::drop(&mut output.pSample);
            }
            
            self.frame_count += 1;
            
            Ok(EncodedFrame {
                data: Bytes::from(output_buffer),
                is_keyframe: force_keyframe || self.frame_count % self.settings.keyframe_interval as u64 == 1,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            })
        }
    }
    
    fn get_type(&self) -> EncoderType {
        EncoderType::Hardware
    }
    
    fn update_settings(&mut self, settings: EncoderSettings) -> Result<()> {
        self.settings = settings;
        // Recreate encoder with new settings
        self.transform = None;
        self.create_encoder()
    }
}

// Helper functions
unsafe fn create_video_type(
    subtype: *const GUID,
    width: u32,
    height: u32,
    fps: u32,
) -> Result<IMFMediaType> {
    let media_type: IMFMediaType = MFCreateMediaType()?;
    
    media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
    media_type.SetGUID(&MF_MT_SUBTYPE, subtype)?;
    media_type.SetUINT64(&MF_MT_FRAME_SIZE, ((width as u64) << 32) | (height as u64))?;
    media_type.SetUINT64(&MF_MT_FRAME_RATE, ((fps as u64) << 32) | 1)?;
    media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
    media_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1 << 32) | 1)?;
    
    Ok(media_type)
}

unsafe fn create_sample_from_rgb(
    rgb_data: &[u8],
    width: u32,
    height: u32,
) -> Result<IMFSample> {
    // Convert RGB to RGB32 (BGRA)
    let mut bgra_data = Vec::with_capacity((width * height * 4) as usize);
    
    for chunk in rgb_data.chunks_exact(3) {
        bgra_data.push(chunk[2]); // B
        bgra_data.push(chunk[1]); // G
        bgra_data.push(chunk[0]); // R
        bgra_data.push(255);      // A
    }
    
    // Create buffer
    let buffer: IMFMediaBuffer = MFCreateMemoryBuffer((width * height * 4) as u32)?;
    
    let mut ptr = std::ptr::null_mut();
    let mut max_len = 0;
    let mut _current_len = 0;
    
    buffer.Lock(&mut ptr, &mut max_len, &mut _current_len)?;
    std::ptr::copy_nonoverlapping(bgra_data.as_ptr(), ptr, bgra_data.len());
    buffer.Unlock()?;
    
    buffer.SetCurrentLength(bgra_data.len() as u32)?;
    
    // Create sample
    let sample: IMFSample = MFCreateSample()?;
    sample.AddBuffer(&buffer)?;
    
    Ok(sample)
}

// GUIDs for encoder configuration
const CODECAPI_AVLowLatencyMode: GUID = GUID::from_u128(0x9c27891a_ed7a_40e1_88e8_b22727a024ee);
const CODECAPI_AVEncCommonRateControlMode: GUID = GUID::from_u128(0x1c0608e9_370c_4710_8a58_cb6181c42423);