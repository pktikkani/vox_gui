use vox_gui::server::server::Server;
use vox_gui::common::auth::AccessCode;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, error};
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    
    info!("Starting Vox Remote Desktop Server");
    
    // Generate access code
    let access_code = AccessCode::generate();
    info!("=================================");
    info!("Access Code: {}", access_code.code);
    info!("Code expires in 5 minutes");
    info!("=================================");
    
    // Create and run server
    let server = Server::new(Arc::new(RwLock::new(Some(access_code))));
    
    match server.run("0.0.0.0:8080").await {
        Ok(_) => info!("Server stopped"),
        Err(e) => error!("Server error: {}", e),
    }
    
    Ok(())
}