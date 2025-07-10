use anyhow::Result;
use std::net::SocketAddr;
use tonic::transport::Server;
use tracing::info;
use tracing_subscriber;

mod service;
mod storage;
mod config;

use service::StorageServiceImpl;
use storage::storage_service_server::StorageServiceServer;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Load configuration
    let config = config::load_config()?;
    info!("Starting Tron Storage Service with config: {:?}", config);

    // Create storage service
    let storage_service = StorageServiceImpl::new(config.clone()).await?;

    // Setup gRPC server
    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    info!("Storage service listening on {}", addr);

    Server::builder()
        .add_service(StorageServiceServer::new(storage_service))
        .serve(addr)
        .await?;

    Ok(())
} 