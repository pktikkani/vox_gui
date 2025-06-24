use crate::common::{
    auth::{AccessCode, AuthRequest, AuthResponse, SessionToken},
    protocol::Message,
    crypto::{CryptoSession, KeyExchange},
};
use crate::server::{
    screen_capture::ScreenCapture,
    input_handler::InputHandler,
};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc, Mutex};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt, AsyncWrite};
use anyhow::{Result, Context};
use tracing::{info, warn, error, debug};
use std::collections::HashMap;
use uuid::Uuid;
use bytes::{BytesMut, Buf};

pub struct Server {
    access_code: Arc<RwLock<Option<AccessCode>>>,
    sessions: Arc<RwLock<HashMap<String, ClientSession>>>,
}

struct ClientSession {
    id: String,
    token: SessionToken,
    crypto: Arc<Mutex<CryptoSession>>,
    tx: mpsc::UnboundedSender<Vec<u8>>,
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
    mut socket: TcpStream,
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
                    let response = handle_auth(&code, &access_code).await;
                    
                    if response.success {
                        // Generate session
                        let session_token = SessionToken::generate(24);
                        let token_string = session_token.token.clone();
                        let id = Uuid::new_v4().to_string();
                        session_id = Some(id.clone());
                        
                        // Store session
                        let session = ClientSession {
                            id: id.clone(),
                            token: session_token,
                            crypto: Arc::new(Mutex::new(CryptoSession::from_shared_secret(&[0; 32])?)), // Placeholder
                            tx: tx.clone(),
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
                    crypto_session = Some(crypto.clone());
                    
                    // Update session crypto if authenticated
                    if let Some(id) = &session_id {
                        if let Some(session) = sessions.write().await.get_mut(id) {
                            session.crypto = crypto;
                        }
                    }
                    
                    debug!("Key exchange completed");
                }
                
                Message::StartStream => {
                    info!("Client requested stream start");
                    // Screen capture is already running
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
        let message = Message::ScreenFrame {
            timestamp: frame.timestamp,
            width: frame.width,
            height: frame.height,
            data: frame.data.to_vec(),
            compressed: true,
        };
        
        let serialized = message.serialize()?;
        
        // Send to all connected clients
        let sessions = sessions.read().await;
        for (_, session) in sessions.iter() {
            let crypto = session.crypto.lock().await;
            if let Ok(encrypted) = crypto.encrypt(&serialized) {
                let _ = session.tx.send(encrypted);
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