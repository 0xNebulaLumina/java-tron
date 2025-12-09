use std::net::SocketAddr;


use tonic::transport::Server;
use tracing::info;
use tracing_subscriber;

use tron_backend_core::BackendService;
use tron_backend_common::{Config, ModuleManager};

// Use the protobuf code from the core crate
use tron_backend_core::backend;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing with fallback to INFO level when RUST_LOG is not set
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .init();

    info!("Starting Tron Backend v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration
    let config = Config::load()?;
    info!("Loaded configuration: {:?}", config);

    // Initialize module manager
    let mut module_manager = ModuleManager::new();

    // Register modules based on feature flags
    #[cfg(feature = "storage")]
    {
        let storage_module = tron_backend_storage::StorageModule::new(&config.storage)?;
        module_manager.register("storage", Box::new(storage_module));
    }

    #[cfg(feature = "execution")]
    {
        let execution_module = tron_backend_execution::ExecutionModule::new(config.execution.clone());
        module_manager.register("execution", Box::new(execution_module));
    }

    // Start all modules
    module_manager.start_all().await?;
    info!("All modules started successfully");

    // Log remote execution configuration (Phase 2 freeze ledger changes feature)
    info!("=== Remote Execution Configuration ===");
    info!("  AccountInfo AEXT mode: {}", config.execution.remote.accountinfo_aext_mode);
    info!("  Emit freeze ledger changes: {}", config.execution.remote.emit_freeze_ledger_changes);
    info!("  Emit global resource changes: {}", config.execution.remote.emit_global_resource_changes);
    info!("  AccountCreate enabled: {}", config.execution.remote.account_create_enabled);
    info!("  FreezeBalance V1 enabled: {}", config.execution.remote.freeze_balance_enabled);
    info!("  UnfreezeBalance V1 enabled: {}", config.execution.remote.unfreeze_balance_enabled);
    info!("  FreezeBalanceV2 enabled: {}", config.execution.remote.freeze_balance_v2_enabled);
    info!("  UnfreezeBalanceV2 enabled: {}", config.execution.remote.unfreeze_balance_v2_enabled);
    info!("======================================");

    // Create the backend service
    let backend_service = BackendService::new(module_manager);

    // Setup server
    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
        .parse()
        .expect("Invalid server address");

    info!("Starting gRPC server on {}", addr);

    // Start server with graceful shutdown
    Server::builder()
        .add_service(backend::backend_server::BackendServer::new(backend_service))
        .serve_with_shutdown(addr, shutdown_signal())
        .await?;

    info!("Server shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C, shutting down...");
        },
        _ = terminate => {
            info!("Received SIGTERM, shutting down...");
        },
    }
} 