use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;

#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    // Frame metrics
    frames_captured: Arc<AtomicU64>,
    frames_encoded: Arc<AtomicU64>,
    frames_sent: Arc<AtomicU64>,
    frames_dropped: Arc<AtomicU64>,
    
    // Timing metrics
    capture_time: Arc<RwLock<MovingAverage>>,
    encode_time: Arc<RwLock<MovingAverage>>,
    network_time: Arc<RwLock<MovingAverage>>,
    
    // Data metrics
    bytes_sent: Arc<AtomicUsize>,
    bytes_received: Arc<AtomicUsize>,
    
    // Start time
    start_time: Instant,
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self {
            frames_captured: Arc::new(AtomicU64::new(0)),
            frames_encoded: Arc::new(AtomicU64::new(0)),
            frames_sent: Arc::new(AtomicU64::new(0)),
            frames_dropped: Arc::new(AtomicU64::new(0)),
            capture_time: Arc::new(RwLock::new(MovingAverage::new(100))),
            encode_time: Arc::new(RwLock::new(MovingAverage::new(100))),
            network_time: Arc::new(RwLock::new(MovingAverage::new(100))),
            bytes_sent: Arc::new(AtomicUsize::new(0)),
            bytes_received: Arc::new(AtomicUsize::new(0)),
            start_time: Instant::now(),
        }
    }
    
    pub fn frame_captured(&self) {
        self.frames_captured.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn frame_encoded(&self) {
        self.frames_encoded.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn frame_sent(&self) {
        self.frames_sent.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn frame_dropped(&self) {
        self.frames_dropped.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_capture_time(&self, duration: Duration) {
        self.capture_time.write().add_sample(duration.as_micros() as f64);
    }
    
    pub fn record_encode_time(&self, duration: Duration) {
        self.encode_time.write().add_sample(duration.as_micros() as f64);
    }
    
    pub fn record_network_time(&self, duration: Duration) {
        self.network_time.write().add_sample(duration.as_micros() as f64);
    }
    
    pub fn add_bytes_sent(&self, bytes: usize) {
        self.bytes_sent.fetch_add(bytes, Ordering::Relaxed);
    }
    
    pub fn add_bytes_received(&self, bytes: usize) {
        self.bytes_received.fetch_add(bytes, Ordering::Relaxed);
    }
    
    pub fn get_stats(&self) -> PerformanceStats {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let frames_captured = self.frames_captured.load(Ordering::Relaxed);
        let frames_encoded = self.frames_encoded.load(Ordering::Relaxed);
        let frames_sent = self.frames_sent.load(Ordering::Relaxed);
        let frames_dropped = self.frames_dropped.load(Ordering::Relaxed);
        
        PerformanceStats {
            fps_captured: frames_captured as f64 / elapsed,
            fps_encoded: frames_encoded as f64 / elapsed,
            fps_sent: frames_sent as f64 / elapsed,
            drop_rate: frames_dropped as f64 / frames_captured.max(1) as f64,
            avg_capture_time_us: self.capture_time.read().average(),
            avg_encode_time_us: self.encode_time.read().average(),
            avg_network_time_us: self.network_time.read().average(),
            throughput_mbps: (self.bytes_sent.load(Ordering::Relaxed) as f64 * 8.0) / (elapsed * 1_000_000.0),
            total_frames: frames_sent,
            uptime_seconds: elapsed,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PerformanceStats {
    pub fps_captured: f64,
    pub fps_encoded: f64,
    pub fps_sent: f64,
    pub drop_rate: f64,
    pub avg_capture_time_us: f64,
    pub avg_encode_time_us: f64,
    pub avg_network_time_us: f64,
    pub throughput_mbps: f64,
    pub total_frames: u64,
    pub uptime_seconds: f64,
}

impl std::fmt::Display for PerformanceStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Performance Stats:\n")?;
        write!(f, "  FPS: {:.1} captured, {:.1} encoded, {:.1} sent\n", 
               self.fps_captured, self.fps_encoded, self.fps_sent)?;
        write!(f, "  Drop rate: {:.1}%\n", self.drop_rate * 100.0)?;
        write!(f, "  Avg times: capture={:.0}µs, encode={:.0}µs, network={:.0}µs\n",
               self.avg_capture_time_us, self.avg_encode_time_us, self.avg_network_time_us)?;
        write!(f, "  Throughput: {:.1} Mbps\n", self.throughput_mbps)?;
        write!(f, "  Total frames: {}, Uptime: {:.0}s", self.total_frames, self.uptime_seconds)
    }
}

#[derive(Debug)]
struct MovingAverage {
    samples: Vec<f64>,
    index: usize,
    count: usize,
    sum: f64,
}

impl MovingAverage {
    fn new(capacity: usize) -> Self {
        Self {
            samples: vec![0.0; capacity],
            index: 0,
            count: 0,
            sum: 0.0,
        }
    }
    
    fn add_sample(&mut self, value: f64) {
        if self.count == self.samples.len() {
            self.sum -= self.samples[self.index];
        }
        
        self.samples[self.index] = value;
        self.sum += value;
        self.index = (self.index + 1) % self.samples.len();
        
        if self.count < self.samples.len() {
            self.count += 1;
        }
    }
    
    fn average(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum / self.count as f64
        }
    }
}

// Timer helper for measuring operations
pub struct Timer {
    start: Instant,
}

impl Timer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }
    
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}