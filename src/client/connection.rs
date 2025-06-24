use crate::common::{
    protocol::Message,
    crypto::{CryptoSession, KeyExchange},
};
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, Mutex};
use bytes::{BytesMut, Buf};
use anyhow::{Result, Context};
use std::sync::Arc;
use tracing::{info, debug, error};

pub struct Connection {
    #[allow(dead_code)]
    stream: Option<TcpStream>,
    crypto: Option<Arc<Mutex<CryptoSession>>>,
    session_token: Option<String>,
}

impl Connection {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<Message>, mpsc::UnboundedSender<Message>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let connection = Connection {
            stream: None,
            crypto: None,
            session_token: None,
        };
        (connection, rx, tx)
    }
    
    pub async fn connect(
        &mut self,
        addr: &str,
        code: &str,
    ) -> Result<(mpsc::UnboundedReceiver<Message>, mpsc::UnboundedSender<Message>)> {
        let mut stream = TcpStream::connect(addr).await
            .context("Failed to connect to server")?;
        
        info!("Connected to server at {}", addr);
        
        // Start key exchange
        let key_exchange = KeyExchange::new();
        let our_public = key_exchange.public_key_bytes();
        
        // Send key exchange
        let key_msg = Message::KeyExchange {
            public_key: our_public.to_vec(),
        };
        
        send_raw_message(&mut stream, &key_msg).await?;
        
        // Read server's public key
        let their_key_msg = read_raw_message(&mut stream).await?;
        
        let their_public_key = if let Message::KeyExchangeAck { public_key } = their_key_msg {
            public_key
        } else {
            return Err(anyhow::anyhow!("Expected KeyExchangeAck"));
        };
        
        // Compute shared secret
        let their_public = x25519_dalek::PublicKey::from(
            <[u8; 32]>::try_from(&their_public_key[..]).context("Invalid public key")?
        );
        let shared_secret = key_exchange.compute_shared_secret(&their_public);
        
        // Create crypto session
        let crypto = Arc::new(Mutex::new(CryptoSession::from_shared_secret(&shared_secret)?));
        self.crypto = Some(crypto.clone());
        
        debug!("Key exchange completed");
        
        // Send authentication
        let auth_msg = Message::AuthRequest {
            code: code.to_string(),
        };
        
        send_encrypted_message(&mut stream, &auth_msg, &crypto).await?;
        
        // Read auth response
        let auth_response = read_encrypted_message(&mut stream, &crypto).await?;
        
        if let Message::AuthResponse { success, session_token } = auth_response {
            if !success {
                return Err(anyhow::anyhow!("Authentication failed"));
            }
            self.session_token = session_token;
        } else {
            return Err(anyhow::anyhow!("Expected AuthResponse"));
        }
        
        info!("Authentication successful");
        
        // Create channels for message passing
        let (tx_in, rx_in) = mpsc::unbounded_channel();
        let (tx_out, rx_out) = mpsc::unbounded_channel();
        
        // Start message handling loops
        self.start_message_loops(stream, crypto, tx_out, rx_in).await?;
        
        // Request stream start
        tx_in.send(Message::StartStream)?;
        
        Ok((rx_out, tx_in))
    }
    
    async fn start_message_loops(
        &self,
        stream: TcpStream,
        crypto: Arc<Mutex<CryptoSession>>,
        tx_out: mpsc::UnboundedSender<Message>,
        mut rx_in: mpsc::UnboundedReceiver<Message>,
    ) -> Result<()> {
        let (mut reader, mut writer) = stream.into_split();
        
        // Spawn reader task
        let reader_crypto = crypto.clone();
        tokio::spawn(async move {
            let mut buffer = BytesMut::with_capacity(65536);
            
            loop {
                match reader.read_buf(&mut buffer).await {
                    Ok(0) => {
                        error!("Server disconnected");
                        break;
                    }
                    Ok(_) => {
                        while buffer.len() >= 4 {
                            let len = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
                            
                            if buffer.len() < 4 + len {
                                break;
                            }
                            
                            buffer.advance(4);
                            let msg_data = buffer.split_to(len).freeze();
                            
                            // Decrypt
                            let decrypted = match reader_crypto.lock().await.decrypt(&msg_data) {
                                Ok(data) => data,
                                Err(e) => {
                                    error!("Decryption error: {}", e);
                                    continue;
                                }
                            };
                            
                            // Parse and send
                            match Message::deserialize(&decrypted) {
                                Ok(msg) => {
                                    if tx_out.send(msg).is_err() {
                                        return;
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to deserialize message: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Read error: {}", e);
                        break;
                    }
                }
            }
        });
        
        // Spawn writer task
        tokio::spawn(async move {
            while let Some(msg) = rx_in.recv().await {
                let serialized = match msg.serialize() {
                    Ok(data) => data,
                    Err(e) => {
                        error!("Serialization error: {}", e);
                        continue;
                    }
                };
                
                // Encrypt
                let encrypted = match crypto.lock().await.encrypt(&serialized) {
                    Ok(data) => data,
                    Err(e) => {
                        error!("Encryption error: {}", e);
                        continue;
                    }
                };
                
                // Send
                if let Err(e) = send_message(&mut writer, &encrypted).await {
                    error!("Failed to send message: {}", e);
                    break;
                }
            }
        });
        
        Ok(())
    }
}

async fn send_raw_message(stream: &mut TcpStream, msg: &Message) -> Result<()> {
    let data = msg.serialize()?;
    send_message(stream, &data).await
}

async fn read_raw_message(stream: &mut TcpStream) -> Result<Message> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    
    let mut data = vec![0u8; len];
    stream.read_exact(&mut data).await?;
    
    Ok(Message::deserialize(&data)?)
}

async fn send_encrypted_message(
    stream: &mut TcpStream,
    msg: &Message,
    crypto: &Arc<Mutex<CryptoSession>>,
) -> Result<()> {
    let data = msg.serialize()?;
    let encrypted = crypto.lock().await.encrypt(&data)?;
    send_message(stream, &encrypted).await
}

async fn read_encrypted_message(
    stream: &mut TcpStream,
    crypto: &Arc<Mutex<CryptoSession>>,
) -> Result<Message> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    
    let mut encrypted = vec![0u8; len];
    stream.read_exact(&mut encrypted).await?;
    
    let decrypted = crypto.lock().await.decrypt(&encrypted)?;
    Ok(Message::deserialize(&decrypted)?)
}

async fn send_message<W: AsyncWriteExt + Unpin>(writer: &mut W, data: &[u8]) -> Result<()> {
    let len = data.len() as u32;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(data).await?;
    writer.flush().await?;
    Ok(())
}