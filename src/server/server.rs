use crate::common::{
    auth::{AccessCode, AuthResponse, SessionToken},
    protocol::Message,
    crypto::{CryptoSession, KeyExchange},
    quality::AdaptiveQualityController,
};
use crate::server::{
    screen_capture::ScreenCapture,
    input_handler::InputHandler,
};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc, Mutex};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use anyhow::{Result, Context};
use tracing::{info, error, debug};
use std::collections::HashMap;
use uuid::Uuid;
use bytes::{BytesMut, Buf};

pub struct Server {
    access_code: Arc<RwLock<Option<AccessCode>>>,
    sessions: Arc<RwLock<HashMap<String, ClientSession>>>,
}

struct ClientSession {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    token: SessionToken,
    crypto: Arc<Mutex<CryptoSession>>,
    tx: mpsc::UnboundedSender<Vec<u8>>,
    quality_controller: Arc<Mutex<AdaptiveQualityController>>,
    last_frame_time: Arc<Mutex<std::time::Instant>>,
}

impl Server {
    pub fn new(access_code: Arc<RwLock<Option<AccessCode>>>) -> Self {
        Server { 
            access_code,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub async fn run(&self, addr: &str) -> Result<()> {
        let listener = TcpListener::bind(addr).await
            .context("Failed to bind to address")?;
        
        info!("Server listening on {}", addr);
        
        // Start screen capture thread
        let (_frame_tx, _frame_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let sessions = self.sessions.clone();
        
        // Spawn screen capture task
        tokio::spawn(async move {
            if let Err(e) = screen_capture_loop(sessions).await {
                error!("Screen capture error: {}", e);
            }
        });
        
        // Accept connections
        loop {
            let (socket, addr) = listener.accept().await?;
            info!("New connection from: {}", addr);
            
            let access_code = self.access_code.clone();
            let sessions = self.sessions.clone();
            
            tokio::spawn(async move {
                if let Err(e) = handle_client(socket, access_code, sessions).await {
                    error!("Client handler error: {}", e);
                }
            });
        }
    }
}

async fn handle_client(
    socket: TcpStream,
    access_code: Arc<RwLock<Option<AccessCode>>>,
    sessions: Arc<RwLock<HashMap<String, ClientSession>>>,
) -> Result<()> {
    let mut buffer = BytesMut::with_capacity(4096);
    let mut crypto_session: Option<Arc<Mutex<CryptoSession>>> = None;
    let mut session_id: Option<String> = None;
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    
    // Split socket for concurrent read/write
    let (mut reader, mut writer) = socket.into_split();
    
    // Spawn task to handle outgoing messages
    let writer_task = tokio::spawn(async move {
        while let Some(data) = rx.recv().await {
            if let Err(e) = send_message(&mut writer, &data).await {
                error!("Failed to send message: {}", e);
                break;
            }
        }
    });
    
    // Handle incoming messages
    loop {
        // Read message length
        if reader.read_buf(&mut buffer).await? == 0 {
            break; // Connection closed
        }
        
        while buffer.len() >= 4 {
            let len = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
            
            if buffer.len() < 4 + len {
                break; // Wait for more data
            }
            
            buffer.advance(4);
            let msg_data = buffer.split_to(len).freeze();
            
            // Decrypt if we have a crypto session
            let decrypted = if let Some(crypto) = &crypto_session {
                let crypto = crypto.lock().await;
                crypto.decrypt(&msg_data)?
            } else {
                msg_data.to_vec()
            };
            
            // Parse message
            let message = Message::deserialize(&decrypted)?;
            
            match message {
                Message::AuthRequest { code } => {
                    // Ensure key exchange has happened first
                    if crypto_session.is_none() {
                        error!("Authentication attempted before key exchange");
                        return Err(anyhow::anyhow!("Key exchange must happen before authentication"));
                    }
                    
                    let response = handle_auth(&code, &access_code).await;
                    
                    if response.success {
                        // Generate session
                        let session_token = SessionToken::generate(24);
                        let token_string = session_token.token.clone();
                        let id = Uuid::new_v4().to_string();
                        session_id = Some(id.clone());
                        
                        // Store session with the current crypto session
                        let session = ClientSession {
                            id: id.clone(),
                            token: session_token,
                            crypto: crypto_session.as_ref().unwrap().clone(),
                            tx: tx.clone(),
                            quality_controller: Arc::new(Mutex::new(AdaptiveQualityController::new())),
                            last_frame_time: Arc::new(Mutex::new(std::time::Instant::now())),
                        };
                        
                        sessions.write().await.insert(id, session);
                        
                        let auth_resp = Message::AuthResponse {
                            success: true,
                            session_token: Some(token_string),
                        };
                        
                        send_encrypted(&tx, &auth_resp, &crypto_session).await?;
                    } else {
                        let auth_resp = Message::AuthResponse {
                            success: false,
                            session_token: None,
                        };
                        
                        send_encrypted(&tx, &auth_resp, &crypto_session).await?;
                    }
                }
                
                Message::KeyExchange { public_key } => {
                    // Perform key exchange
                    let key_exchange = KeyExchange::new();
                    let our_public = key_exchange.public_key_bytes();
                    
                    // Send our public key
                    let response = Message::KeyExchangeAck {
                        public_key: our_public.to_vec(),
                    };
                    
                    tx.send(response.serialize()?)?;
                    
                    // Compute shared secret
                    let their_public = x25519_dalek::PublicKey::from(
                        <[u8; 32]>::try_from(&public_key[..]).context("Invalid public key")?
                    );
                    let shared_secret = key_exchange.compute_shared_secret(&their_public);
                    
                    // Create crypto session
                    let crypto = Arc::new(Mutex::new(CryptoSession::from_shared_secret(&shared_secret)?));
                    crypto_session = Some(crypto);
                    
                    debug!("Key exchange completed");
                }
                
                Message::StartStream => {
                    info!("Client requested stream start");
                    // Send initial quality mode
                    if let Some(id) = &session_id {
                        if let Some(session) = sessions.read().await.get(id) {
                            let quality = session.quality_controller.lock().await.get_current_quality();
                            let msg = Message::QualityChange { mode: quality };
                            send_encrypted(&tx, &msg, &crypto_session).await?;
                        }
                    }
                }
                
                Message::StopStream => {
                    info!("Client requested stream stop");
                    // Could implement pausing logic here
                }
                
                Message::MouseMove { x, y } => {
                    handle_mouse_move(x, y).await?;
                }
                
                Message::MouseClick { button, pressed, x, y } => {
                    handle_mouse_click(button, pressed, x, y).await?;
                }
                
                Message::KeyEvent { key, pressed, modifiers } => {
                    handle_key_event(&key, pressed, modifiers).await?;
                }
                
                Message::FrameAck { timestamp, received_at } => {
                    // Update quality metrics
                    if let Some(id) = &session_id {
                        if let Some(session) = sessions.read().await.get(id) {
                            let rtt = received_at.saturating_sub(timestamp);
                            let mut controller = session.quality_controller.lock().await;
                            controller.update_metrics(0, std::time::Duration::from_millis(rtt));
                        }
                    }
                }
                
                Message::RequestQualityChange { mode } => {
                    if let Some(id) = &session_id {
                        if let Some(session) = sessions.read().await.get(id) {
                            let mut controller = session.quality_controller.lock().await;
                            controller.force_quality(Some(mode));
                            
                            // Send confirmation
                            let msg = Message::QualityChange { mode };
                            send_encrypted(&tx, &msg, &crypto_session).await?;
                        }
                    }
                }
                
                Message::Disconnect => {
                    info!("Client disconnecting");
                    break;
                }
                
                _ => {
                    debug!("Unhandled message type");
                }
            }
        }
    }
    
    // Cleanup
    if let Some(id) = session_id {
        sessions.write().await.remove(&id);
    }
    
    writer_task.abort();
    Ok(())
}

async fn handle_auth(
    code: &str,
    access_code: &Arc<RwLock<Option<AccessCode>>>,
) -> AuthResponse {
    let code_guard = access_code.read().await;
    
    if let Some(stored_code) = &*code_guard {
        if stored_code.verify(code) {
            return AuthResponse {
                success: true,
                session_token: None,
                message: "Authentication successful".to_string(),
            };
        }
    }
    
    AuthResponse {
        success: false,
        session_token: None,
        message: "Invalid or expired code".to_string(),
    }
}

async fn screen_capture_loop(
    sessions: Arc<RwLock<HashMap<String, ClientSession>>>,
) -> Result<()> {
    // Run screen capture in a separate thread
    let (tx, mut rx) = mpsc::unbounded_channel::<crate::server::screen_capture::CapturedFrame>();
    
    std::thread::spawn(move || {
        let mut capture = match ScreenCapture::new(30) {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to initialize screen capture: {}", e);
                return;
            }
        };
        
        loop {
            if let Ok(Some(frame)) = capture.capture_frame() {
                let _ = tx.send(frame);
            }
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    });
    
    // Process frames in async context
    while let Some(frame) = rx.recv().await {
        let sessions_guard = sessions.read().await;
        
        for (_, session) in sessions_guard.iter() {
            // Check quality settings for this client
            let mut quality_controller = session.quality_controller.lock().await;
            let quality = quality_controller.get_recommended_quality();
            
            // Update frame time and metrics
            let now = std::time::Instant::now();
            let last_time = *session.last_frame_time.lock().await;
            let frame_time = now.duration_since(last_time);
            *session.last_frame_time.lock().await = now;
            
            // Skip frame if it's too soon for this quality level
            let target_interval = std::time::Duration::from_millis(1000 / quality.target_fps() as u64);
            if frame_time < target_interval {
                continue;
            }
            
            // Create appropriate message based on frame type
            let message = match frame.frame_type {
                crate::common::frame_processor::FrameType::KeyFrame => {
                    Message::ScreenFrame {
                        timestamp: frame.timestamp,
                        width: frame.width,
                        height: frame.height,
                        data: frame.data.to_vec(),
                        compressed: true,
                    }
                }
                crate::common::frame_processor::FrameType::DeltaFrame => {
                    if let Some(tiles) = &frame.tiles {
                        Message::DeltaFrame {
                            timestamp: frame.timestamp,
                            tiles: tiles.clone(),
                        }
                    } else {
                        continue;
                    }
                }
            };
            
            // Serialize and encrypt
            if let Ok(serialized) = message.serialize() {
                let crypto = session.crypto.lock().await;
                if let Ok(encrypted) = crypto.encrypt(&serialized) {
                    // Update metrics with frame size
                    quality_controller.update_metrics(encrypted.len(), frame_time);
                    
                    // Send frame
                    let _ = session.tx.send(encrypted);
                }
            }
        }
    }
    
    Ok(())
}

async fn handle_mouse_move(x: i32, y: i32) -> Result<()> {
    // Run input handling in blocking task
    tokio::task::spawn_blocking(move || {
        let mut handler = InputHandler::new()?;
        handler.mouse_move(x, y)
    }).await?
}

async fn handle_mouse_click(
    button: crate::common::protocol::MouseButton,
    pressed: bool,
    x: i32,
    y: i32,
) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let mut handler = InputHandler::new()?;
        handler.mouse_click(button, pressed, x, y)
    }).await?
}

async fn handle_key_event(
    key: &str,
    pressed: bool,
    modifiers: crate::common::protocol::Modifiers,
) -> Result<()> {
    let key = key.to_string();
    tokio::task::spawn_blocking(move || {
        let mut handler = InputHandler::new()?;
        handler.key_event(&key, pressed, modifiers)
    }).await?
}

async fn send_message(writer: &mut tokio::net::tcp::OwnedWriteHalf, data: &[u8]) -> Result<()> {
    let len = data.len() as u32;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(data).await?;
    writer.flush().await?;
    Ok(())
}

async fn send_encrypted(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    message: &Message,
    crypto: &Option<Arc<Mutex<CryptoSession>>>,
) -> Result<()> {
    let serialized = message.serialize()?;
    
    let data = if let Some(crypto) = crypto {
        let crypto = crypto.lock().await;
        crypto.encrypt(&serialized)?
    } else {
        serialized
    };
    
    tx.send(data)?;
    Ok(())
}