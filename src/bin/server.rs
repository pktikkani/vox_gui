use vox_gui::server::server::Server;
// use vox_gui::server::quic_server::QuicServer;
use vox_gui::common::auth::AccessCode;
use vox_gui::common::metrics::PerformanceMetrics;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, error};
use tracing_subscriber;
use clap::{Parser, ValueEnum};

#[derive(Parser)]
#[command(name = "vox_server")]
#[command(about = "High-performance remote desktop server")]
struct Args {
    /// Server address to bind to
    #[arg(short, long, default_value = "0.0.0.0:8080")]
    address: String,
    
    /// Transport protocol to use
    #[arg(short, long, value_enum, default_value = "tcp")]
    transport: Transport,
    
    /// Enable performance metrics
    #[arg(short, long)]
    metrics: bool,
}

#[derive(Clone, ValueEnum)]
enum Transport {
    Tcp,
    Quic,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    
    let args = Args::parse();
    
    info!("Starting Vox Remote Desktop Server");
    
    // Generate access code
    let access_code = AccessCode::generate();
    info!("=================================");
    info!("Access Code: {}", access_code.code);
    info!("Code expires in 5 minutes");
    info!("=================================");
    
    let access_code = Arc::new(RwLock::new(Some(access_code)));
    
    // Start metrics collection if enabled
    let _metrics = if args.metrics {
        let metrics = Arc::new(PerformanceMetrics::new());
        let metrics_clone = metrics.clone();
        
        // Print metrics every 5 seconds
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                info!("\n{}", metrics_clone.get_stats());
            }
        });
        
        Some(metrics)
    } else {
        None
    };
    
    // Start server with selected transport
    match args.transport {
        Transport::Tcp => {
            info!("Starting TCP server on {}", args.address);
            let server = Server::new(access_code);
            match server.run(&args.address).await {
                Ok(_) => info!("Server stopped"),
                Err(e) => error!("Server error: {}", e),
            }
        }
        Transport::Quic => {
            error!("QUIC transport is temporarily disabled due to dependency issues");
            error!("Please use TCP transport for now: --transport tcp");
            return Err(anyhow::anyhow!("QUIC not available"));
        }
    }
    
    Ok(())
}