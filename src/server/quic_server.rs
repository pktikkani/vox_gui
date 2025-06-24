use crate::common::{
    auth::{AccessCode, AuthResponse, SessionToken},
    protocol::Message,
    crypto::{CryptoSession, KeyExchange},
    quality::AdaptiveQualityController,
    transport::{QuicTransport, QuicConnection},
    encoder::{EncoderFactory, EncoderType, EncoderSettings, VideoEncoder},
};
use crate::server::screen_capture::ScreenCapture;
use anyhow::{Result, Context};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc, Mutex};
use tracing::{info, error, debug};
use std::collections::HashMap;
use std::net::SocketAddr;
use uuid::Uuid;

pub struct QuicServer {
    transport: QuicTransport,
    access_code: Arc<RwLock<Option<AccessCode>>>,
    sessions: Arc<RwLock<HashMap<String, ClientSession>>>,
}

struct ClientSession {
    id: String,
    #[allow(dead_code)]
    token: SessionToken,
    connection: QuicConnection,
    crypto: Arc<Mutex<CryptoSession>>,
    quality_controller: Arc<Mutex<AdaptiveQualityController>>,
    encoder: Arc<Mutex<Box<dyn VideoEncoder>>>,
}

impl QuicServer {
    pub async fn new(addr: SocketAddr, access_code: Arc<RwLock<Option<AccessCode>>>) -> Result<Self> {
        let transport = QuicTransport::new_server(addr).await?;
        
        Ok(Self {
            transport,
            access_code,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        })
    }
    
    pub async fn run(&self) -> Result<()> {
        info!("QUIC server listening with hardware acceleration support");
        
        // Start screen capture thread
        let sessions = self.sessions.clone();
        tokio::spawn(async move {
            if let Err(e) = screen_capture_loop(sessions).await {
                error!("Screen capture error: {}", e);
            }
        });
        
        // Accept connections
        loop {
            match self.transport.accept().await {
                Ok(connection) => {
                    let access_code = self.access_code.clone();
                    let sessions = self.sessions.clone();
                    
                    tokio::spawn(async move {
                        if let Err(e) = handle_client(connection, access_code, sessions).await {
                            error!("Client handler error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }
}

async fn handle_client(
    mut connection: QuicConnection,
    access_code: Arc<RwLock<Option<AccessCode>>>,
    sessions: Arc<RwLock<HashMap<String, ClientSession>>>,
) -> Result<()> {
    info!("New QUIC connection from: {}", connection.remote_address());
    
    let mut crypto_session: Option<Arc<Mutex<CryptoSession>>> = None;
    let mut session_id: Option<String> = None;
    
    // Handle initial handshake on first stream
    let (mut send, mut recv) = connection.accept_stream().await?;
    
    // Key exchange
    let key_msg_data = crate::common::transport::receive_message(&mut recv).await?;
    let key_msg = Message::deserialize(&key_msg_data)?;
    
    if let Message::KeyExchange { public_key } = key_msg {
        let key_exchange = KeyExchange::new();
        let our_public = key_exchange.public_key_bytes();
        
        let response = Message::KeyExchangeAck {
            public_key: our_public.to_vec(),
        };
        
        crate::common::transport::send_message(&mut send, &response.serialize()?).await?;
        
        let their_public = x25519_dalek::PublicKey::from(
            <[u8; 32]>::try_from(&public_key[..]).context("Invalid public key")?
        );
        let shared_secret = key_exchange.compute_shared_secret(&their_public);
        
        crypto_session = Some(Arc::new(Mutex::new(CryptoSession::from_shared_secret(&shared_secret)?)));
        debug!("Key exchange completed");
    }
    
    // Authentication
    let auth_msg_data = crate::common::transport::receive_message(&mut recv).await?;
    let auth_msg = Message::deserialize(&auth_msg_data)?;
    
    if let Message::AuthRequest { code } = auth_msg {
        let response = handle_auth(&code, &access_code).await;
        
        if response.success {
            let session_token = SessionToken::generate(24);
            let token_string = session_token.token.clone();
            let id = Uuid::new_v4().to_string();
            session_id = Some(id.clone());
            
            // Create encoder with hardware acceleration if available
            let encoder_settings = EncoderSettings {
                width: 1920, // Will be updated with actual screen size
                height: 1080,
                fps: 30,
                bitrate: 5_000_000, // 5 Mbps
                keyframe_interval: 60,
            };
            
            let encoder_type = if EncoderFactory::is_hardware_available() {
                EncoderType::Hardware
            } else {
                EncoderType::Software
            };
            
            let encoder = EncoderFactory::create_encoder(encoder_type, encoder_settings)?;
            
            let session = ClientSession {
                id: id.clone(),
                token: session_token,
                connection,
                crypto: crypto_session.as_ref().unwrap().clone(),
                quality_controller: Arc::new(Mutex::new(AdaptiveQualityController::new())),
                encoder: Arc::new(Mutex::new(encoder)),
            };
            
            sessions.write().await.insert(id, session);
            
            let auth_resp = Message::AuthResponse {
                success: true,
                session_token: Some(token_string),
            };
            
            let crypto = crypto_session.as_ref().unwrap().lock().await;
            let encrypted = crypto.encrypt(&auth_resp.serialize()?)?;
            crate::common::transport::send_message(&mut send, &encrypted).await?;
        }
    }
    
    // Handle control messages on separate streams
    if let Some(id) = session_id {
        let sessions_clone = sessions.clone();
        tokio::spawn(async move {
            handle_control_streams(id, sessions_clone).await;
        });
    }
    
    Ok(())
}

async fn handle_control_streams(
    session_id: String,
    sessions: Arc<RwLock<HashMap<String, ClientSession>>>,
) {
    loop {
        let sessions_guard = sessions.read().await;
        if let Some(session) = sessions_guard.get(&session_id) {
            let mut connection = session.connection.clone();
            drop(sessions_guard);
            
            match connection.accept_stream().await {
                Ok((mut _send, mut recv)) => {
                    let sessions_clone = sessions.clone();
                    let session_id_clone = session_id.clone();
                    
                    tokio::spawn(async move {
                        if let Ok(data) = crate::common::transport::receive_message(&mut recv).await {
                            handle_control_message(session_id_clone, data, sessions_clone).await;
                        }
                    });
                }
                Err(_) => break,
            }
        } else {
            break;
        }
    }
}

async fn handle_control_message(
    session_id: String,
    data: Vec<u8>,
    sessions: Arc<RwLock<HashMap<String, ClientSession>>>,
) {
    let sessions_guard = sessions.read().await;
    if let Some(session) = sessions_guard.get(&session_id) {
        let crypto = session.crypto.lock().await;
        if let Ok(decrypted) = crypto.decrypt(&data) {
            if let Ok(message) = Message::deserialize(&decrypted) {
                match message {
                    Message::RequestQualityChange { mode } => {
                        let mut controller = session.quality_controller.lock().await;
                        controller.force_quality(Some(mode));
                        
                        // Update encoder settings
                        let mut encoder = session.encoder.lock().await;
                        let settings = EncoderSettings {
                            width: 1920, // Should get from actual screen
                            height: 1080,
                            fps: mode.target_fps(),
                            bitrate: (mode.bandwidth_requirement() * 1_000_000.0) as u32,
                            keyframe_interval: mode.keyframe_interval(),
                        };
                        let _ = encoder.update_settings(settings);
                    }
                    Message::FrameAck { timestamp: _, received_at: _ } => {
                        // Update quality metrics
                        // This would be handled by the streaming loop
                    }
                    _ => {}
                }
            }
        }
    }
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
    let (tx, mut rx) = mpsc::unbounded_channel::<crate::server::screen_capture::CapturedFrame>();
    
    std::thread::spawn(move || {
        let mut capture = match ScreenCapture::new(60) { // 60 FPS capture
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
            std::thread::sleep(std::time::Duration::from_millis(8)); // ~120 FPS polling
        }
    });
    
    // Process frames
    while let Some(frame) = rx.recv().await {
        let sessions_guard = sessions.read().await;
        
        for (_, session) in sessions_guard.iter() {
            let quality_controller = session.quality_controller.lock().await;
            let _quality = quality_controller.get_current_quality();
            drop(quality_controller);
            
            // Encode frame with hardware encoder
            let mut encoder = session.encoder.lock().await;
            
            let force_keyframe = false; // Let encoder decide
            match encoder.encode_frame(&frame.data, force_keyframe) {
                Ok(encoded_frame) => {
                    // Send encoded frame via QUIC datagram for lowest latency
                    let msg = Message::ScreenFrame {
                        timestamp: encoded_frame.timestamp,
                        width: frame.width,
                        height: frame.height,
                        data: encoded_frame.data.to_vec(),
                        compressed: true,
                    };
                    if let Ok(serialized) = msg.serialize() {
                        let crypto = session.crypto.lock().await;
                        if let Ok(encrypted) = crypto.encrypt(&serialized) {
                            let _ = session.connection.send_datagram(encrypted.into()).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Encoding error: {}", e);
                }
            }
        }
    }
    
    Ok(())
}