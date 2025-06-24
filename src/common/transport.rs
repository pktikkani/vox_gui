use anyhow::{Result, Context};
use quinn::{Endpoint, ServerConfig, ClientConfig, Connection, RecvStream, SendStream};
use std::sync::Arc;
use std::net::SocketAddr;
use tokio::sync::mpsc;
use bytes::Bytes;

pub struct QuicTransport {
    endpoint: Endpoint,
}

#[derive(Clone)]
pub struct QuicConnection {
    connection: Connection,
    incoming: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<(SendStream, RecvStream)>>>,
}

impl QuicTransport {
    pub async fn new_server(addr: SocketAddr) -> Result<Self> {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .context("Failed to generate certificate")?;
        
        let cert_der = cert.cert.der().clone();
        let priv_key_der = cert.key_pair.serialize_der();
        
        let mut cert_store = rustls::RootCertStore::empty();
        cert_store.add(cert_der.clone())?;
        
        let server_config = ServerConfig::with_single_cert(
            vec![cert_der.clone()],
            priv_key_der.clone(),
        )?;
        let endpoint = Endpoint::server(server_config, addr)?;
        
        Ok(Self { endpoint })
    }
    
    pub async fn new_client() -> Result<Self> {
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        
        // Create client config that accepts any certificate
        let mut transport_config = ClientConfig::new(Arc::new(
            rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(SkipServerVerification::new())
                .with_no_client_auth()
        ));
        transport_config.transport_config(Arc::new(Self::transport_config()));
        
        endpoint.set_default_client_config(transport_config);
        
        Ok(Self { endpoint })
    }
    
    fn transport_config() -> quinn::TransportConfig {
        let mut config = quinn::TransportConfig::default();
        
        // Optimize for low latency
        config.max_idle_timeout(Some(std::time::Duration::from_secs(30).try_into().unwrap()));
        config.keep_alive_interval(Some(std::time::Duration::from_secs(5)));
        
        // Increase stream limits
        config.max_concurrent_bidi_streams(quinn::VarInt::from_u32(256));
        config.max_concurrent_uni_streams(quinn::VarInt::from_u32(256));
        
        // Optimize for throughput
        config.receive_window(quinn::VarInt::from_u32(10 * 1024 * 1024));
        config.stream_receive_window(quinn::VarInt::from_u32(5 * 1024 * 1024));
        
        // Additional optimizations
        config.initial_rtt(std::time::Duration::from_millis(100));
        
        config
    }
    
    pub async fn accept(&self) -> Result<QuicConnection> {
        let connecting = self.endpoint
            .accept()
            .await
            .context("Failed to accept connection")?;
            
        let connection = connecting.await?;
        
        let (tx, rx) = mpsc::unbounded_channel();
        let conn_clone = connection.clone();
        
        // Spawn task to accept streams
        tokio::spawn(async move {
            while let Ok((send, recv)) = conn_clone.accept_bi().await {
                let _ = tx.send((send, recv));
            }
        });
        
        Ok(QuicConnection {
            connection,
            incoming: Arc::new(tokio::sync::Mutex::new(rx)),
        })
    }
    
    pub async fn connect(&self, addr: SocketAddr, server_name: &str) -> Result<QuicConnection> {
        let connection = self.endpoint
            .connect(addr, server_name)?
            .await?;
            
        let (tx, rx) = mpsc::unbounded_channel();
        let conn_clone = connection.clone();
        
        // Spawn task to accept streams
        tokio::spawn(async move {
            while let Ok((send, recv)) = conn_clone.accept_bi().await {
                let _ = tx.send((send, recv));
            }
        });
        
        Ok(QuicConnection {
            connection,
            incoming: Arc::new(tokio::sync::Mutex::new(rx)),
        })
    }
}

impl QuicConnection {
    pub async fn open_stream(&self) -> Result<(SendStream, RecvStream)> {
        self.connection
            .open_bi()
            .await
            .context("Failed to open stream")
    }
    
    pub async fn accept_stream(&mut self) -> Result<(SendStream, RecvStream)> {
        self.incoming
            .lock()
            .await
            .recv()
            .await
            .context("Connection closed")
    }
    
    pub async fn send_datagram(&self, data: Bytes) -> Result<()> {
        self.connection
            .send_datagram(data)
            .context("Failed to send datagram")
    }
    
    pub async fn receive_datagram(&self) -> Result<Bytes> {
        self.connection
            .read_datagram()
            .await
            .context("Failed to receive datagram")
    }
    
    pub fn remote_address(&self) -> SocketAddr {
        self.connection.remote_address()
    }
    
    pub async fn close(&self) {
        self.connection.close(0u32.into(), b"closing");
    }
}

// Helper to skip certificate verification for development
#[derive(Debug)]
struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer,
        _intermediates: &[rustls::pki_types::CertificateDer],
        _server_name: &rustls::pki_types::ServerName,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

// Stream helpers for message framing
pub async fn send_message(stream: &mut SendStream, data: &[u8]) -> Result<()> {
    let len = data.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(data).await?;
    stream.finish()?;
    Ok(())
}

pub async fn receive_message(stream: &mut RecvStream) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    
    let mut data = vec![0u8; len];
    stream.read_exact(&mut data).await?;
    
    Ok(data)
}