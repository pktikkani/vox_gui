use scrap::{Capturer, Display};
use std::io::ErrorKind::WouldBlock;
use std::time::{Duration, Instant};
use anyhow::{Result, Context};
use bytes::Bytes;
use zstd::stream::encode_all;
use crate::common::quality::QualityMode;
use crate::common::frame_processor::FrameProcessor;

pub struct ScreenCapture {
    capturer: Capturer,
    width: usize,
    height: usize,
    last_frame_time: Instant,
    frame_interval: Duration,
    quality_mode: QualityMode,
    frame_processor: FrameProcessor,
    frame_count: u64,
}

impl ScreenCapture {
    pub fn new(fps: u32) -> Result<Self> {
        let display = Display::primary()
            .context("Failed to get primary display")?;
        
        let capturer = Capturer::new(display)
            .context("Failed to create screen capturer")?;
        
        let width = capturer.width();
        let height = capturer.height();
        
        Ok(ScreenCapture {
            capturer,
            width,
            height,
            last_frame_time: Instant::now(),
            frame_interval: Duration::from_millis(1000 / fps as u64),
            quality_mode: QualityMode::High,
            frame_processor: FrameProcessor::new(width as u32, height as u32),
            frame_count: 0,
        })
    }
    
    pub fn set_quality(&mut self, quality: QualityMode) {
        self.quality_mode = quality;
        self.frame_interval = Duration::from_millis(1000 / quality.target_fps() as u64);
    }
    
    pub fn capture_frame(&mut self) -> Result<Option<CapturedFrame>> {
        // Check if enough time has passed for next frame
        if self.last_frame_time.elapsed() < self.frame_interval {
            return Ok(None);
        }
        
        match self.capturer.frame() {
            Ok(frame) => {
                self.last_frame_time = Instant::now();
                self.frame_count += 1;
                
                // Clone the frame data to avoid borrow issues
                let frame_data = frame.to_vec();
                
                // Convert BGRA to RGB
                let mut rgb_data = bgra_to_rgb(&frame_data, self.width, self.height);
                
                // Apply quality scaling if needed (disabled for now to avoid pixelation)
                let scale = self.quality_mode.resolution_scale();
                if scale < 1.0 && false { // Temporarily disabled
                    rgb_data = self.scale_frame(&rgb_data, scale)?;
                }
                
                // For now, always send keyframes to avoid artifacts
                let force_keyframe = true; // TODO: Re-enable delta encoding when client properly handles it
                
                // Process frame with delta encoding
                let processed = self.frame_processor.process_frame(&rgb_data, force_keyframe)?;
                
                // Compress based on quality mode
                let compression_level = self.quality_mode.compression_level();
                let compressed_data = match processed.frame_type {
                    crate::common::frame_processor::FrameType::KeyFrame => {
                        encode_all(&processed.data[..], compression_level)?
                    }
                    crate::common::frame_processor::FrameType::DeltaFrame => {
                        // For delta frames, compress tiles individually
                        if let Some(tiles) = &processed.tiles {
                            let mut compressed_tiles = Vec::new();
                            for tile in tiles {
                                let compressed = encode_all(&tile.data[..], compression_level)?;
                                compressed_tiles.push(crate::common::frame_processor::TileData {
                                    x: tile.x,
                                    y: tile.y,
                                    width: tile.width,
                                    height: tile.height,
                                    data: Bytes::from(compressed),
                                });
                            }
                            // Return delta frame data
                            return Ok(Some(CapturedFrame {
                                width: processed.width,
                                height: processed.height,
                                data: Bytes::new(), // No full data for delta
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_millis() as u64,
                                frame_type: processed.frame_type,
                                tiles: Some(compressed_tiles),
                            }));
                        }
                        vec![]
                    }
                };
                
                Ok(Some(CapturedFrame {
                    width: processed.width,
                    height: processed.height,
                    data: Bytes::from(compressed_data),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                    frame_type: processed.frame_type,
                    tiles: None,
                }))
            }
            Err(ref e) if e.kind() == WouldBlock => {
                // Frame not ready yet
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }
    
    fn scale_frame(&self, rgb_data: &[u8], scale: f32) -> Result<Vec<u8>> {
        let new_width = (self.width as f32 * scale) as u32;
        let new_height = (self.height as f32 * scale) as u32;
        
        // Simple nearest-neighbor scaling for speed
        let mut scaled = vec![0u8; (new_width * new_height * 3) as usize];
        
        for y in 0..new_height {
            for x in 0..new_width {
                let src_x = (x as f32 / scale) as usize;
                let src_y = (y as f32 / scale) as usize;
                
                if src_x < self.width && src_y < self.height {
                    let src_idx = (src_y * self.width + src_x) * 3;
                    let dst_idx = ((y * new_width + x) * 3) as usize;
                    
                    if src_idx + 3 <= rgb_data.len() && dst_idx + 3 <= scaled.len() {
                        scaled[dst_idx..dst_idx + 3].copy_from_slice(&rgb_data[src_idx..src_idx + 3]);
                    }
                }
            }
        }
        
        Ok(scaled)
    }
    
    
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width as u32, self.height as u32)
    }
}

fn bgra_to_rgb(bgra: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    
    for chunk in bgra.chunks_exact(4) {
        rgb.push(chunk[2]); // R
        rgb.push(chunk[1]); // G
        rgb.push(chunk[0]); // B
        // Skip alpha channel
    }
    
    rgb
}

#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub data: Bytes,
    pub timestamp: u64,
    pub frame_type: crate::common::frame_processor::FrameType,
    pub tiles: Option<Vec<crate::common::frame_processor::TileData>>,
}