use serde::{Deserialize, Serialize};
use bytes::Bytes;
use crate::common::quality::{QualityMode, QualityMetrics};
use crate::common::frame_processor::TileData;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Message {
    // Authentication
    AuthRequest { code: String },
    AuthResponse { success: bool, session_token: Option<String> },
    
    // Key exchange for encryption
    KeyExchange { public_key: Vec<u8> },
    KeyExchangeAck { public_key: Vec<u8> },
    
    // Screen data
    ScreenFrame { 
        timestamp: u64,
        width: u32,
        height: u32,
        data: Vec<u8>,
        compressed: bool,
    },
    
    // Optimized frame with delta encoding
    DeltaFrame {
        timestamp: u64,
        tiles: Vec<TileData>,
    },
    
    // Quality control
    QualityChange { mode: QualityMode },
    QualityMetricsReport { metrics: QualityMetrics },
    RequestQualityChange { mode: QualityMode },
    
    // Input events
    MouseMove { x: i32, y: i32 },
    MouseClick { button: MouseButton, pressed: bool, x: i32, y: i32 },
    MouseScroll { delta_x: f64, delta_y: f64 },
    KeyEvent { key: String, pressed: bool, modifiers: Modifiers },
    
    // Control messages
    StartStream,
    StopStream,
    Ping { timestamp: u64 },
    Pong { timestamp: u64 },
    Disconnect,
    
    // Performance metrics
    FrameAck { timestamp: u64, received_at: u64 },
    NetworkStats { bytes_sent: usize, rtt_ms: u64 },
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Debug)]
pub struct Frame {
    pub timestamp: u64,
    pub width: u32,
    pub height: u32,
    pub data: Bytes,
}

impl Message {
    pub fn serialize(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }
    
    pub fn deserialize(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}