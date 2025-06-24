use scrap::{Capturer, Display};
use std::io::ErrorKind::WouldBlock;
use std::time::{Duration, Instant};
use anyhow::{Result, Context};
use bytes::Bytes;
use zstd::stream::encode_all;

pub struct ScreenCapture {
    capturer: Capturer,
    width: usize,
    height: usize,
    last_frame_time: Instant,
    frame_interval: Duration,
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
        })
    }
    
    pub fn capture_frame(&mut self) -> Result<Option<CapturedFrame>> {
        // Check if enough time has passed for next frame
        if self.last_frame_time.elapsed() < self.frame_interval {
            return Ok(None);
        }
        
        match self.capturer.frame() {
            Ok(frame) => {
                self.last_frame_time = Instant::now();
                
                // Clone the frame data to avoid borrow issues
                let frame_data = frame.to_vec();
                
                // Convert BGRA to RGB
                let rgb_data = bgra_to_rgb(&frame_data, self.width, self.height);
                
                // Compress the frame
                let compressed = encode_all(&rgb_data[..], 3)?;
                
                Ok(Some(CapturedFrame {
                    width: self.width as u32,
                    height: self.height as u32,
                    data: Bytes::from(compressed),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                }))
            }
            Err(ref e) if e.kind() == WouldBlock => {
                // Frame not ready yet
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
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
}