use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityMode {
    Ultra,   // Original resolution, 60 FPS, minimal compression
    High,    // Original resolution, 30 FPS, moderate compression
    Medium,  // 75% resolution, 30 FPS, high compression
    Low,     // 50% resolution, 15 FPS, maximum compression
    Minimal, // 25% resolution, 10 FPS, extreme compression
}

impl QualityMode {
    pub fn resolution_scale(&self) -> f32 {
        match self {
            QualityMode::Ultra => 1.0,
            QualityMode::High => 1.0,
            QualityMode::Medium => 0.75,
            QualityMode::Low => 0.5,
            QualityMode::Minimal => 0.25,
        }
    }
    
    pub fn target_fps(&self) -> u32 {
        match self {
            QualityMode::Ultra => 60,
            QualityMode::High => 30,
            QualityMode::Medium => 30,
            QualityMode::Low => 15,
            QualityMode::Minimal => 10,
        }
    }
    
    pub fn compression_level(&self) -> i32 {
        match self {
            QualityMode::Ultra => 1,    // Fastest compression
            QualityMode::High => 3,
            QualityMode::Medium => 6,
            QualityMode::Low => 9,
            QualityMode::Minimal => 12, // Maximum compression
        }
    }
    
    pub fn keyframe_interval(&self) -> u32 {
        match self {
            QualityMode::Ultra => 120,   // Every 2 seconds at 60fps
            QualityMode::High => 60,     // Every 2 seconds at 30fps
            QualityMode::Medium => 30,   // Every 1 second
            QualityMode::Low => 15,      // Every 1 second
            QualityMode::Minimal => 10,  // Every 1 second
        }
    }
    
    // Estimated bandwidth requirements in Mbps
    pub fn bandwidth_requirement(&self) -> f32 {
        match self {
            QualityMode::Ultra => 50.0,
            QualityMode::High => 20.0,
            QualityMode::Medium => 10.0,
            QualityMode::Low => 5.0,
            QualityMode::Minimal => 2.0,
        }
    }
}

pub struct BandwidthMonitor {
    samples: VecDeque<BandwidthSample>,
    max_samples: usize,
    last_update: Instant,
}

struct BandwidthSample {
    timestamp: Instant,
    bytes_sent: usize,
    rtt: Duration,
}

impl BandwidthMonitor {
    pub fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(30),
            max_samples: 30,
            last_update: Instant::now(),
        }
    }
    
    pub fn add_sample(&mut self, bytes_sent: usize, rtt: Duration) {
        let sample = BandwidthSample {
            timestamp: Instant::now(),
            bytes_sent,
            rtt,
        };
        
        self.samples.push_back(sample);
        if self.samples.len() > self.max_samples {
            self.samples.pop_front();
        }
        
        self.last_update = Instant::now();
    }
    
    pub fn get_bandwidth_mbps(&self) -> Option<f32> {
        if self.samples.len() < 2 {
            return None;
        }
        
        let first = self.samples.front()?;
        let last = self.samples.back()?;
        
        let duration = last.timestamp.duration_since(first.timestamp);
        if duration.as_secs_f32() < 0.1 {
            return None;
        }
        
        let total_bytes: usize = self.samples.iter().map(|s| s.bytes_sent).sum();
        let mbps = (total_bytes as f32 * 8.0) / (duration.as_secs_f32() * 1_000_000.0);
        
        Some(mbps)
    }
    
    pub fn get_average_rtt(&self) -> Option<Duration> {
        if self.samples.is_empty() {
            return None;
        }
        
        let total_millis: u64 = self.samples.iter()
            .map(|s| s.rtt.as_millis() as u64)
            .sum();
        
        Some(Duration::from_millis(total_millis / self.samples.len() as u64))
    }
    
    pub fn get_packet_loss_rate(&self) -> f32 {
        // This would need actual packet tracking implementation
        // For now, we estimate based on RTT variance
        if self.samples.len() < 5 {
            return 0.0;
        }
        
        let avg_rtt = self.get_average_rtt().unwrap_or(Duration::from_millis(50));
        let variance: f32 = self.samples.iter()
            .map(|s| {
                let diff = s.rtt.as_secs_f32() - avg_rtt.as_secs_f32();
                diff * diff
            })
            .sum::<f32>() / self.samples.len() as f32;
        
        // High variance suggests packet loss
        (variance * 100.0).min(20.0)
    }
}

pub struct AdaptiveQualityController {
    current_quality: QualityMode,
    bandwidth_monitor: BandwidthMonitor,
    last_quality_change: Instant,
    quality_change_cooldown: Duration,
    forced_quality: Option<QualityMode>,
}

impl AdaptiveQualityController {
    pub fn new() -> Self {
        Self {
            current_quality: QualityMode::High,
            bandwidth_monitor: BandwidthMonitor::new(),
            last_quality_change: Instant::now(),
            quality_change_cooldown: Duration::from_secs(2),
            forced_quality: None,
        }
    }
    
    pub fn force_quality(&mut self, quality: Option<QualityMode>) {
        self.forced_quality = quality;
        if let Some(q) = quality {
            self.current_quality = q;
        }
    }
    
    pub fn update_metrics(&mut self, bytes_sent: usize, rtt: Duration) {
        self.bandwidth_monitor.add_sample(bytes_sent, rtt);
    }
    
    pub fn get_recommended_quality(&mut self) -> QualityMode {
        // If quality is forced by user, return that
        if let Some(quality) = self.forced_quality {
            return quality;
        }
        
        // Don't change quality too frequently
        if self.last_quality_change.elapsed() < self.quality_change_cooldown {
            return self.current_quality;
        }
        
        // Get current metrics
        let bandwidth = self.bandwidth_monitor.get_bandwidth_mbps().unwrap_or(10.0);
        let avg_rtt = self.bandwidth_monitor.get_average_rtt()
            .unwrap_or(Duration::from_millis(50));
        let packet_loss = self.bandwidth_monitor.get_packet_loss_rate();
        
        // Determine quality based on metrics
        let recommended = self.calculate_quality(bandwidth, avg_rtt, packet_loss);
        
        // Only change if significantly different
        if recommended != self.current_quality {
            self.current_quality = recommended;
            self.last_quality_change = Instant::now();
        }
        
        self.current_quality
    }
    
    fn calculate_quality(&self, bandwidth: f32, rtt: Duration, packet_loss: f32) -> QualityMode {
        // Score based on multiple factors
        let bandwidth_score = (bandwidth / 50.0).min(1.0);
        let rtt_score = 1.0 - (rtt.as_millis() as f32 / 200.0).min(1.0);
        let loss_score = 1.0 - (packet_loss / 10.0).min(1.0);
        
        // Weighted average
        let total_score = bandwidth_score * 0.5 + rtt_score * 0.3 + loss_score * 0.2;
        
        match total_score {
            s if s >= 0.8 => QualityMode::Ultra,
            s if s >= 0.6 => QualityMode::High,
            s if s >= 0.4 => QualityMode::Medium,
            s if s >= 0.2 => QualityMode::Low,
            _ => QualityMode::Minimal,
        }
    }
    
    pub fn get_current_quality(&self) -> QualityMode {
        self.current_quality
    }
    
    pub fn get_metrics(&self) -> QualityMetrics {
        QualityMetrics {
            quality: self.current_quality,
            bandwidth_mbps: self.bandwidth_monitor.get_bandwidth_mbps().unwrap_or(0.0),
            average_rtt: self.bandwidth_monitor.get_average_rtt()
                .unwrap_or(Duration::from_millis(0)),
            packet_loss: self.bandwidth_monitor.get_packet_loss_rate(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityMetrics {
    pub quality: QualityMode,
    pub bandwidth_mbps: f32,
    pub average_rtt: Duration,
    pub packet_loss: f32,
}