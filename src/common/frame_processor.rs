use anyhow::Result;
use bytes::Bytes;
use std::sync::Arc;
use parking_lot::RwLock;

const TILE_SIZE: usize = 64; // Process in 64x64 tiles for better cache locality

pub struct FrameProcessor {
    last_frame: Arc<RwLock<Option<Vec<u8>>>>,
    width: u32,
    height: u32,
    tile_width: u32,
    tile_height: u32,
}

#[derive(Debug, Clone)]
pub struct ProcessedFrame {
    pub frame_type: FrameType,
    pub data: Bytes,
    pub width: u32,
    pub height: u32,
    pub tiles: Option<Vec<TileData>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FrameType {
    KeyFrame,
    DeltaFrame,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TileData {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    #[serde(with = "bytes_serde")]
    pub data: Bytes,
}

mod bytes_serde {
    use bytes::Bytes;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    
    pub fn serialize<S>(bytes: &Bytes, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        bytes.as_ref().serialize(serializer)
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Bytes, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec = Vec::<u8>::deserialize(deserializer)?;
        Ok(Bytes::from(vec))
    }
}

impl FrameProcessor {
    pub fn new(width: u32, height: u32) -> Self {
        let tile_width = (width + TILE_SIZE as u32 - 1) / TILE_SIZE as u32;
        let tile_height = (height + TILE_SIZE as u32 - 1) / TILE_SIZE as u32;
        
        Self {
            last_frame: Arc::new(RwLock::new(None)),
            width,
            height,
            tile_width,
            tile_height,
        }
    }
    
    pub fn process_frame(&self, frame: &[u8], force_keyframe: bool) -> Result<ProcessedFrame> {
        let mut last_frame = self.last_frame.write();
        
        // First frame or forced keyframe
        if last_frame.is_none() || force_keyframe {
            *last_frame = Some(frame.to_vec());
            
            return Ok(ProcessedFrame {
                frame_type: FrameType::KeyFrame,
                data: Bytes::copy_from_slice(frame),
                width: self.width,
                height: self.height,
                tiles: None,
            });
        }
        
        // Delta encoding - find changed tiles
        let previous = last_frame.as_ref().unwrap();
        let changed_tiles = self.find_changed_tiles(previous, frame);
        
        // If more than 60% of tiles changed, send keyframe
        let total_tiles = (self.tile_width * self.tile_height) as usize;
        if changed_tiles.len() > total_tiles * 6 / 10 {
            *last_frame = Some(frame.to_vec());
            
            return Ok(ProcessedFrame {
                frame_type: FrameType::KeyFrame,
                data: Bytes::copy_from_slice(frame),
                width: self.width,
                height: self.height,
                tiles: None,
            });
        }
        
        // Update last frame with changed tiles
        let mut new_frame = previous.clone();
        for tile in &changed_tiles {
            self.copy_tile_to_frame(&mut new_frame, tile);
        }
        *last_frame = Some(new_frame);
        
        // Return delta frame with only changed tiles
        Ok(ProcessedFrame {
            frame_type: FrameType::DeltaFrame,
            data: Bytes::new(), // No full data for delta frames
            width: self.width,
            height: self.height,
            tiles: Some(changed_tiles),
        })
    }
    
    fn find_changed_tiles(&self, previous: &[u8], current: &[u8]) -> Vec<TileData> {
        let mut changed_tiles = Vec::new();
        let bytes_per_pixel = 3; // RGB
        
        // Use rayon for parallel processing if available
        for tile_y in 0..self.tile_height {
            for tile_x in 0..self.tile_width {
                let x = tile_x * TILE_SIZE as u32;
                let y = tile_y * TILE_SIZE as u32;
                let w = TILE_SIZE.min((self.width - x) as usize) as u32;
                let h = TILE_SIZE.min((self.height - y) as usize) as u32;
                
                if self.is_tile_changed(previous, current, x, y, w, h, bytes_per_pixel) {
                    let tile_data = self.extract_tile(current, x, y, w, h, bytes_per_pixel);
                    changed_tiles.push(TileData {
                        x,
                        y,
                        width: w,
                        height: h,
                        data: Bytes::from(tile_data),
                    });
                }
            }
        }
        
        changed_tiles
    }
    
    fn is_tile_changed(&self, prev: &[u8], curr: &[u8], x: u32, y: u32, w: u32, h: u32, bpp: usize) -> bool {
        // Quick sampling first - check corners and center
        let sample_points = [
            (0, 0),
            (w - 1, 0),
            (0, h - 1),
            (w - 1, h - 1),
            (w / 2, h / 2),
        ];
        
        for (sx, sy) in sample_points {
            let px = x + sx;
            let py = y + sy;
            
            if px >= self.width || py >= self.height {
                continue;
            }
            
            let offset = ((py * self.width + px) * bpp as u32) as usize;
            if offset + bpp <= prev.len() && offset + bpp <= curr.len() {
                if prev[offset..offset + bpp] != curr[offset..offset + bpp] {
                    return true;
                }
            }
        }
        
        false
    }
    
    fn extract_tile(&self, frame: &[u8], x: u32, y: u32, w: u32, h: u32, bpp: usize) -> Vec<u8> {
        let mut tile_data = Vec::with_capacity((w * h * bpp as u32) as usize);
        
        for ty in 0..h {
            let py = y + ty;
            if py >= self.height {
                break;
            }
            
            let row_start = ((py * self.width + x) * bpp as u32) as usize;
            let row_end = row_start + (w * bpp as u32) as usize;
            
            if row_end <= frame.len() {
                tile_data.extend_from_slice(&frame[row_start..row_end]);
            }
        }
        
        tile_data
    }
    
    fn copy_tile_to_frame(&self, frame: &mut [u8], tile: &TileData) {
        let bpp = 3; // RGB
        let mut tile_offset = 0;
        
        for ty in 0..tile.height {
            let py = tile.y + ty;
            if py >= self.height {
                break;
            }
            
            let frame_offset = ((py * self.width + tile.x) * bpp) as usize;
            let copy_len = (tile.width * bpp) as usize;
            
            if frame_offset + copy_len <= frame.len() && tile_offset + copy_len <= tile.data.len() {
                frame[frame_offset..frame_offset + copy_len]
                    .copy_from_slice(&tile.data[tile_offset..tile_offset + copy_len]);
            }
            
            tile_offset += copy_len;
        }
    }
    
    pub fn apply_delta(&self, base_frame: &mut [u8], delta: &ProcessedFrame) -> Result<()> {
        if let Some(tiles) = &delta.tiles {
            for tile in tiles {
                self.copy_tile_to_frame(base_frame, tile);
            }
        }
        Ok(())
    }
}