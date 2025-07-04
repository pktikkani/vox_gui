use eframe::egui;
use egui::{CentralPanel, TopBottomPanel, Context, TextureHandle, ColorImage, Margin};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use crate::common::protocol::{Message, MouseButton, Modifiers};
use crate::client::connection::Connection;
use crate::common::quality::{QualityMode, QualityMetrics};
use crate::common::frame_processor::{FrameProcessor, ProcessedFrame, FrameType};
use zstd::stream::decode_all;
use crate::client::h264_decoder::H264Decoder;

pub struct VoxApp {
    state: AppState,
    access_code: String,
    server_address: String,
    
    // Connection state
    connection: Option<Arc<Mutex<Connection>>>,
    tx: Option<mpsc::UnboundedSender<Message>>,
    rx: Option<Arc<Mutex<mpsc::UnboundedReceiver<Message>>>>,
    state_rx: Option<Arc<Mutex<mpsc::UnboundedReceiver<AppState>>>>,
    
    // Screen state
    screen_texture: Option<TextureHandle>,
    screen_size: (u32, u32),
    last_mouse_pos: egui::Pos2,
    current_frame: Option<Vec<u8>>,
    frame_processor: Option<FrameProcessor>,
    h264_decoder: Option<H264Decoder>,
    
    // Quality control
    current_quality: QualityMode,
    quality_metrics: Option<QualityMetrics>,
    show_quality_menu: bool,
    
    // Runtime handle
    runtime: Arc<tokio::runtime::Runtime>,
}

#[derive(Debug, Clone, PartialEq)]
enum AppState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

impl Default for VoxApp {
    fn default() -> Self {
        let runtime = Arc::new(
            tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime")
        );
        
        Self {
            state: AppState::Disconnected,
            access_code: String::new(),
            server_address: "127.0.0.1:8080".to_string(),
            connection: None,
            tx: None,
            rx: None,
            state_rx: None,
            screen_texture: None,
            screen_size: (1920, 1080),
            last_mouse_pos: egui::Pos2::ZERO,
            current_frame: None,
            frame_processor: None,
            h264_decoder: None,
            current_quality: QualityMode::High,
            quality_metrics: None,
            show_quality_menu: false,
            runtime,
        }
    }
}

impl VoxApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Apply custom theme
        super::theme::apply_custom_theme(&cc.egui_ctx);
        Self::default()
    }
    
    fn show_connection_ui(&mut self, ctx: &Context) {
        CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                
                // Logo/Title with custom styling
                ui.label(egui::RichText::new("🖥️").size(48.0));
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("Vox Remote Desktop")
                        .size(28.0)
                        .color(egui::Color32::from_rgb(220, 225, 230))
                );
                ui.add_space(30.0);
                
                // Connection form with custom background
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(35, 40, 50))
                    .rounding(egui::Rounding::same(12.0))
                    .inner_margin(Margin::same(30.0))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::Vec2::new(0.0, 4.0),
                        blur: 8.0,
                        spread: 0.0,
                        color: egui::Color32::from_black_alpha(40),
                    })
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("Enter access code")
                                    .size(16.0)
                                    .color(egui::Color32::from_rgb(150, 160, 170))
                            );
                            ui.add_space(15.0);
                            
                            // Access code input with custom styling
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut self.access_code)
                                    .desired_width(250.0)
                                    .hint_text("123456")
                                    .font(egui::TextStyle::Heading)
                                    .margin(egui::Vec2::new(10.0, 10.0))
                            );
                            
                            if response.changed() {
                                self.access_code.retain(|c| c.is_ascii_digit());
                                self.access_code.truncate(6);
                            }
                            
                            ui.add_space(20.0);
                            
                            // Server address with custom frame
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Server:")
                                        .color(egui::Color32::from_rgb(150, 160, 170))
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.server_address)
                                        .desired_width(180.0)
                                        .margin(egui::Vec2::new(8.0, 4.0))
                                );
                            });
                            
                            ui.add_space(25.0);
                            
                            // Connect button with custom styling
                            let connect_enabled = self.access_code.len() == 6 && 
                                                self.state != AppState::Connecting;
                            
                            let button = egui::Button::new(
                                egui::RichText::new("Connect").size(16.0)
                            )
                            .fill(if connect_enabled {
                                egui::Color32::from_rgb(88, 166, 255)
                            } else {
                                egui::Color32::from_rgb(60, 70, 80)
                            })
                            .rounding(egui::Rounding::same(8.0));
                            
                            let response = ui.add_sized([200.0, 45.0], button);
                            if response.clicked() && connect_enabled {
                                self.connect();
                            }
                        });
                    });
                
                ui.add_space(20.0);
                
                // Status messages with custom styling
                match &self.state {
                    AppState::Connecting => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(
                                egui::RichText::new("Connecting...")
                                    .color(egui::Color32::from_rgb(88, 166, 255))
                            );
                        });
                    }
                    AppState::Error(msg) => {
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(255, 88, 88).linear_multiply(0.2))
                            .rounding(egui::Rounding::same(4.0))
                            .inner_margin(Margin::same(10.0))
                            .show(ui, |ui| {
                                ui.colored_label(
                                    egui::Color32::from_rgb(255, 88, 88),
                                    format!("⚠ {}", msg)
                                );
                            });
                    }
                    _ => {}
                }
            });
        });
    }
    
    fn show_remote_screen(&mut self, ctx: &Context) {
        TopBottomPanel::top("top_panel")
            .frame(egui::Frame::none()
                .fill(egui::Color32::from_rgb(28, 32, 40))
                .inner_margin(Margin::symmetric(16.0, 12.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("●")
                            .size(12.0)
                            .color(egui::Color32::from_rgb(88, 255, 166))
                    );
                    ui.label(
                        egui::RichText::new(format!("Connected to {}", self.server_address))
                            .color(egui::Color32::from_rgb(220, 225, 230))
                    );
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let disconnect_button = egui::Button::new(
                            egui::RichText::new("Disconnect").size(14.0)
                        )
                        .fill(egui::Color32::from_rgb(255, 88, 88))
                        .rounding(egui::Rounding::same(6.0));
                        
                        if ui.add(disconnect_button).clicked() {
                            self.disconnect();
                        }
                        
                        ui.separator();
                        
                        // Quality selector
                        let quality_text = format!("Quality: {:?}", self.current_quality);
                        if ui.button(quality_text).clicked() {
                            self.show_quality_menu = !self.show_quality_menu;
                        }
                        
                        // Show quality metrics if available
                        if let Some(metrics) = &self.quality_metrics {
                            ui.separator();
                            ui.label(format!("{:.1} Mbps", metrics.bandwidth_mbps));
                            ui.label(format!("{:.0}ms", metrics.average_rtt.as_millis()));
                        }
                    });
                });
            });
        
        CentralPanel::default().show(ctx, |ui| {
            let available_size = ui.available_size();
            
            // Calculate scaled image dimensions and position
            let image_size = egui::Vec2::new(
                self.screen_size.0 as f32,
                self.screen_size.1 as f32,
            );
            let scale = (available_size.x / image_size.x).min(available_size.y / image_size.y);
            let scaled_size = image_size * scale;
            
            // Calculate the centered image rect
            let image_rect = egui::Rect::from_center_size(
                ui.available_rect_before_wrap().center(),
                scaled_size,
            );
            
            // Handle mouse input
            if let Some(hover_pos) = ui.input(|i| i.pointer.hover_pos()) {
                if image_rect.contains(hover_pos) {
                    // Convert UI coordinates to screen coordinates relative to the actual image
                    let relative_x = ((hover_pos.x - image_rect.left()) / image_rect.width()).clamp(0.0, 1.0);
                    let relative_y = ((hover_pos.y - image_rect.top()) / image_rect.height()).clamp(0.0, 1.0);
                    
                    let screen_x = (relative_x * self.screen_size.0 as f32).round() as i32;
                    let screen_y = (relative_y * self.screen_size.1 as f32).round() as i32;
                    
                    // Send mouse move if position changed significantly
                    let new_pos = egui::Pos2::new(screen_x as f32, screen_y as f32);
                    if (new_pos - self.last_mouse_pos).length() > 1.0 {
                        self.last_mouse_pos = new_pos;
                        self.send_message(Message::MouseMove { x: screen_x, y: screen_y });
                    }
                    
                    // Handle mouse clicks
                    ui.input(|i| {
                        if i.pointer.primary_pressed() {
                            self.send_message(Message::MouseClick {
                                button: MouseButton::Left,
                                pressed: true,
                                x: screen_x,
                                y: screen_y,
                            });
                        }
                        if i.pointer.primary_released() {
                            self.send_message(Message::MouseClick {
                                button: MouseButton::Left,
                                pressed: false,
                                x: screen_x,
                                y: screen_y,
                            });
                        }
                        if i.pointer.secondary_pressed() {
                            self.send_message(Message::MouseClick {
                                button: MouseButton::Right,
                                pressed: true,
                                x: screen_x,
                                y: screen_y,
                            });
                        }
                        if i.pointer.secondary_released() {
                            self.send_message(Message::MouseClick {
                                button: MouseButton::Right,
                                pressed: false,
                                x: screen_x,
                                y: screen_y,
                            });
                        }
                    });
                }
            }
            
            // Handle keyboard input
            ctx.input(|i| {
                for event in &i.events {
                    if let egui::Event::Key { key, physical_key: _, pressed, repeat: _, modifiers } = event {
                        if let Some(key_str) = format_key(*key) {
                            self.send_message(Message::KeyEvent {
                                key: key_str,
                                pressed: *pressed,
                                modifiers: Modifiers {
                                    shift: modifiers.shift,
                                    ctrl: modifiers.ctrl || modifiers.command,
                                    alt: modifiers.alt,
                                    meta: modifiers.command,
                                },
                            });
                        }
                    }
                }
            });
            
            // Display the remote screen
            if let Some(texture) = &self.screen_texture {
                ui.centered_and_justified(|ui| {
                    ui.image((texture.id(), scaled_size));
                });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Waiting for screen data...");
                });
            }
        });
    }
    
    fn connect(&mut self) {
        self.state = AppState::Connecting;
        
        let addr = self.server_address.clone();
        let code = self.access_code.clone();
        
        let (connection, _, _) = Connection::new();
        let connection = Arc::new(Mutex::new(connection));
        self.connection = Some(connection.clone());
        
        // Create channels for state updates
        let (state_tx, state_rx) = mpsc::unbounded_channel::<AppState>();
        let (msg_tx, msg_rx) = mpsc::unbounded_channel::<Message>();
        
        self.rx = Some(Arc::new(Mutex::new(msg_rx)));
        self.state_rx = Some(Arc::new(Mutex::new(state_rx)));
        
        // Channel to send messages to the connection
        let (conn_tx, mut conn_rx) = mpsc::unbounded_channel::<Message>();
        self.tx = Some(conn_tx);
        
        // Spawn connection task
        let runtime = self.runtime.clone();
        
        std::thread::spawn(move || {
            runtime.block_on(async move {
                let mut conn = connection.lock().unwrap();
                match conn.connect(&addr, &code).await {
                    Ok((mut rx, tx)) => {
                        tracing::info!("Connected successfully");
                        state_tx.send(AppState::Connected).ok();
                        
                        // Drop the connection lock
                        drop(conn);
                        
                        // Spawn task to forward outgoing messages
                        let tx_clone = tx.clone();
                        tokio::spawn(async move {
                            while let Some(msg) = conn_rx.recv().await {
                                if tx_clone.send(msg).is_err() {
                                    break;
                                }
                            }
                        });
                        
                        // Forward incoming messages
                        while let Some(msg) = rx.recv().await {
                            if msg_tx.send(msg).is_err() {
                                break;
                            }
                        }
                        
                        state_tx.send(AppState::Disconnected).ok();
                    }
                    Err(e) => {
                        tracing::error!("Connection failed: {}", e);
                        state_tx.send(AppState::Error(format!("Connection failed: {}", e))).ok();
                    }
                }
            });
        });
    }
    
    fn disconnect(&mut self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Message::Disconnect);
        }
        self.state = AppState::Disconnected;
        self.connection = None;
        self.tx = None;
        self.rx = None;
        self.state_rx = None;
        self.screen_texture = None;
        self.access_code.clear();
    }
    
    fn send_message(&self, msg: Message) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(msg);
        }
    }
    
    pub fn update_screen(&mut self, ctx: &Context, width: u32, height: u32, rgb_data: &[u8]) {
        self.screen_size = (width, height);
        
        let color_image = ColorImage::from_rgb(
            [width as usize, height as usize],
            rgb_data,
        );
        
        self.screen_texture = Some(ctx.load_texture(
            "remote_screen",
            color_image,
            Default::default(),
        ));
    }
}

impl eframe::App for VoxApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Check for state updates
        if let Some(state_rx) = &self.state_rx {
            if let Ok(mut rx) = state_rx.try_lock() {
                if let Ok(new_state) = rx.try_recv() {
                    self.state = new_state;
                }
            }
        }
        
        // Process incoming messages
        let mut screen_update = None;
        
        if let Some(rx) = &self.rx {
            if let Ok(mut rx) = rx.try_lock() {
                while let Ok(msg) = rx.try_recv() {
                    match msg {
                        Message::ScreenFrame { timestamp, width, height, data, encoding } => {
                            // Decode based on encoding type
                            let rgb_data = match encoding {
                                crate::common::protocol::EncodingType::Raw => data,
                                crate::common::protocol::EncodingType::ZstdCompressed => {
                                    match decode_all(&data[..]) {
                                        Ok(decompressed) => decompressed,
                                        Err(e) => {
                                            tracing::error!("Failed to decompress frame: {}", e);
                                            continue;
                                        }
                                    }
                                }
                                crate::common::protocol::EncodingType::H264 => {
                                    // Initialize H.264 decoder if needed
                                    if self.h264_decoder.is_none() {
                                        match H264Decoder::new(width, height) {
                                            Ok(decoder) => self.h264_decoder = Some(decoder),
                                            Err(e) => {
                                                tracing::error!("Failed to create H.264 decoder: {}", e);
                                                continue;
                                            }
                                        }
                                    }
                                    
                                    // Decode H.264 frame
                                    if let Some(decoder) = &mut self.h264_decoder {
                                        match decoder.decode(&data) {
                                            Ok(Some(rgb_data)) => rgb_data,
                                            Ok(None) => {
                                                // Decoder needs more data
                                                continue;
                                            }
                                            Err(e) => {
                                                tracing::error!("Failed to decode H.264 frame: {}", e);
                                                continue;
                                            }
                                        }
                                    } else {
                                        continue;
                                    }
                                }
                                crate::common::protocol::EncodingType::WebP => {
                                    // WebP is handled by software encoder
                                    tracing::error!("WebP decoding not yet implemented");
                                    continue;
                                }
                            };
                            
                            // Initialize frame processor if needed
                            if self.frame_processor.is_none() {
                                self.frame_processor = Some(FrameProcessor::new(width, height));
                            }
                            
                            // Store as current frame
                            self.current_frame = Some(rgb_data.clone());
                            screen_update = Some((width, height, rgb_data));
                            
                            // Send acknowledgment
                            self.send_message(Message::FrameAck {
                                timestamp,
                                received_at: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_millis() as u64,
                            });
                        }
                        Message::DeltaFrame { timestamp, tiles } => {
                            // Apply delta to current frame
                            if let Some(current) = &mut self.current_frame {
                                let delta = ProcessedFrame {
                                    frame_type: FrameType::DeltaFrame,
                                    data: bytes::Bytes::new(),
                                    width: self.screen_size.0,
                                    height: self.screen_size.1,
                                    tiles: Some(tiles.clone()),
                                };
                                
                                if let Some(processor) = &self.frame_processor {
                                    if processor.apply_delta(current, &delta).is_ok() {
                                        screen_update = Some((self.screen_size.0, self.screen_size.1, current.clone()));
                                    }
                                }
                            }
                            
                            // Send acknowledgment
                            self.send_message(Message::FrameAck {
                                timestamp,
                                received_at: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_millis() as u64,
                            });
                        }
                        Message::QualityChange { mode } => {
                            self.current_quality = mode;
                            tracing::info!("Quality changed to: {:?}", mode);
                        }
                        Message::QualityMetricsReport { metrics } => {
                            self.quality_metrics = Some(metrics);
                        }
                        Message::AuthResponse { success, session_token: _ } => {
                            if !success {
                                self.state = AppState::Error("Authentication failed".to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        
        // Apply screen update outside of the lock
        if let Some((width, height, rgb_data)) = screen_update {
            self.update_screen(ctx, width, height, &rgb_data);
        }
        
        match self.state {
            AppState::Disconnected | AppState::Connecting | AppState::Error(_) => {
                self.show_connection_ui(ctx);
            }
            AppState::Connected => {
                self.show_remote_screen(ctx);
                
                // Show quality menu if requested
                if self.show_quality_menu {
                    egui::Window::new("Quality Settings")
                        .collapsible(false)
                        .auto_sized()
                        .show(ctx, |ui| {
                            ui.label("Select video quality:");
                            ui.separator();
                            
                            for mode in [
                                QualityMode::Ultra,
                                QualityMode::High,
                                QualityMode::Medium,
                                QualityMode::Low,
                                QualityMode::Minimal,
                            ] {
                                let selected = self.current_quality == mode;
                                if ui.selectable_label(selected, format!("{:?} - {:.0} Mbps", mode, mode.bandwidth_requirement())).clicked() {
                                    self.send_message(Message::RequestQualityChange { mode });
                                    self.show_quality_menu = false;
                                }
                            }
                            
                            ui.separator();
                            if ui.button("Close").clicked() {
                                self.show_quality_menu = false;
                            }
                        });
                }
            }
        }
        
        // Request repaint for smooth updates
        ctx.request_repaint();
    }
}

fn format_key(key: egui::Key) -> Option<String> {
    use egui::Key;
    
    Some(match key {
        Key::A => "a".to_string(),
        Key::B => "b".to_string(),
        Key::C => "c".to_string(),
        Key::D => "d".to_string(),
        Key::E => "e".to_string(),
        Key::F => "f".to_string(),
        Key::G => "g".to_string(),
        Key::H => "h".to_string(),
        Key::I => "i".to_string(),
        Key::J => "j".to_string(),
        Key::K => "k".to_string(),
        Key::L => "l".to_string(),
        Key::M => "m".to_string(),
        Key::N => "n".to_string(),
        Key::O => "o".to_string(),
        Key::P => "p".to_string(),
        Key::Q => "q".to_string(),
        Key::R => "r".to_string(),
        Key::S => "s".to_string(),
        Key::T => "t".to_string(),
        Key::U => "u".to_string(),
        Key::V => "v".to_string(),
        Key::W => "w".to_string(),
        Key::X => "x".to_string(),
        Key::Y => "y".to_string(),
        Key::Z => "z".to_string(),
        Key::Num0 => "0".to_string(),
        Key::Num1 => "1".to_string(),
        Key::Num2 => "2".to_string(),
        Key::Num3 => "3".to_string(),
        Key::Num4 => "4".to_string(),
        Key::Num5 => "5".to_string(),
        Key::Num6 => "6".to_string(),
        Key::Num7 => "7".to_string(),
        Key::Num8 => "8".to_string(),
        Key::Num9 => "9".to_string(),
        Key::Space => " ".to_string(),
        Key::Enter => "Return".to_string(),
        Key::Escape => "Escape".to_string(),
        Key::Backspace => "BackSpace".to_string(),
        Key::Tab => "Tab".to_string(),
        Key::ArrowDown => "Down".to_string(),
        Key::ArrowLeft => "Left".to_string(),
        Key::ArrowRight => "Right".to_string(),
        Key::ArrowUp => "Up".to_string(),
        _ => return None,
    })
}