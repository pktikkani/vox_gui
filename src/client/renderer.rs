use bytes::Bytes;
use anyhow::Result;

pub struct Renderer {
    // TODO: Implement frame rendering
}

impl Renderer {
    pub fn new() -> Self {
        Renderer {}
    }
    
    pub fn decode_frame(&self, _data: &Bytes) -> Result<Vec<u8>> {
        // TODO: Implement frame decoding
        Ok(vec![])
    }
}