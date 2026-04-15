use std::net::SocketAddr;

use tonic::transport::Server;
use tracing::{error, info, warn};
use tracing_subscriber;

use tron_backend_common::{Config, GenesisConfig, ModuleManager};
use tron_backend_core::BackendService;

// Use the protobuf code from the core crate
use tron_backend_core::backend;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing with fallback to INFO level when RUST_LOG is not set
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("Starting Tron Backend v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration
    let config = Config::load()?;
    info!("Loaded configuration: {:?}", config);

    // LD-2 runtime guard: refuse to start the production main binary if the
    // loaded config says rust_persist_enabled=true. See
    // `planning/close_loop.planning.md` §§ LD-1, LD-2: the canonical RR writer
    // is Java's `RuntimeSpiImpl` apply path, so Rust must only compute deltas
    // in RR production. The conformance runner at
    // `rust-backend/crates/core/src/conformance/runner.rs` is the one
    // in-tree force-`true` caller; it builds its own `ExecutionConfig` in
    // code and does not go through `Config::load()`, so this guard does not
    // affect conformance/isolation paths. This guard intentionally does NOT
    // cover the separate NonVm buffered-write bypass in
    // `crates/core/src/service/grpc/mod.rs`, which is tracked as its own
    // open item (§1.1 deferred follow-up #5 / LD-1 Current-state gap).
    // It also does NOT cover the `CreateSmartContract` metadata persist
    // path in `grpc/mod.rs` that writes straight to the storage engine
    // when `write_buffer` is absent — that Rust-owned VM-side write is a
    // separate LD-1 gap outside the `rust_persist_enabled` / `WriteMode`
    // signal and is tracked under the LD-11 bridge-debt inventory.
    if config.execution.remote.rust_persist_enabled {
        error!(
            "LD-2 guard: `execution.remote.rust_persist_enabled = true` was loaded from config, \
             but the `main` binary is the RR production path. The canonical RR writer is Java's \
             RuntimeSpiImpl apply path (LD-1). Set `rust_persist_enabled = false` in \
             `rust-backend/config.toml` (or clear \
             `TRON_BACKEND__EXECUTION__REMOTE__RUST_PERSIST_ENABLED` in the environment). If you \
             need `rust_persist_enabled = true` for conformance or isolation runs, use the \
             conformance runner (`rust-backend/crates/core/src/conformance/runner.rs`) instead: \
             it builds its own ExecutionConfig in code and does not load `config.toml`. \
             See `planning/close_loop.planning.md` §§ LD-1, LD-2 for the write-ownership lock."
        );
        return Err(
            "LD-2 guard: rust_persist_enabled=true is not allowed in main.rs (RR production path)"
                .into(),
        );
    }

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
        let execution_module =
            tron_backend_execution::ExecutionModule::new(config.execution.clone());
        module_manager.register("execution", Box::new(execution_module));
    }

    // Start all modules first (storage engine becomes available after start)
    module_manager.start_all().await?;
    info!("All modules started successfully");

    // Initialize genesis accounts AFTER modules are started (storage engine now available)
    #[cfg(feature = "storage")]
    if config.genesis.enabled {
        if let Some(storage_module) = module_manager.get("storage") {
            if let Some(storage_mod) = storage_module
                .as_any()
                .downcast_ref::<tron_backend_storage::StorageModule>()
            {
                if let Ok(engine) = storage_mod.engine() {
                    initialize_genesis_accounts(engine, &config.genesis)?;
                } else {
                    warn!("Storage engine not available for genesis initialization");
                }
            }
        }
    }

    // Log remote execution configuration (Phase 2 freeze ledger changes feature)
    info!("=== Remote Execution Configuration ===");
    info!(
        "  AccountInfo AEXT mode: {}",
        config.execution.remote.accountinfo_aext_mode
    );
    info!(
        "  Emit freeze ledger changes: {}",
        config.execution.remote.emit_freeze_ledger_changes
    );
    info!(
        "  Emit global resource changes: {}",
        config.execution.remote.emit_global_resource_changes
    );
    info!(
        "  AccountCreate enabled: {}",
        config.execution.remote.account_create_enabled
    );
    info!(
        "  FreezeBalance V1 enabled: {}",
        config.execution.remote.freeze_balance_enabled
    );
    info!(
        "  UnfreezeBalance V1 enabled: {}",
        config.execution.remote.unfreeze_balance_enabled
    );
    info!(
        "  FreezeBalanceV2 enabled: {}",
        config.execution.remote.freeze_balance_v2_enabled
    );
    info!(
        "  UnfreezeBalanceV2 enabled: {}",
        config.execution.remote.unfreeze_balance_v2_enabled
    );
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

/// Initialize genesis accounts in storage.
///
/// This function pre-populates accounts with initial balances for testing/parity.
/// It's used to ensure the Rust storage has the same initial state as the mainnet
/// at a given starting block.
#[cfg(feature = "storage")]
fn initialize_genesis_accounts(
    engine: &tron_backend_storage::StorageEngine,
    genesis_config: &GenesisConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("=== Genesis Account Initialization ===");
    info!(
        "  Accounts to initialize: {}",
        genesis_config.accounts.len()
    );

    // Database name for accounts (matching java-tron's AccountStore)
    const ACCOUNT_DB: &str = "account";

    for account in &genesis_config.accounts {
        // Decode Base58 address to bytes
        let addr_bytes = match tron_backend_common::from_tron_address(&account.address) {
            Ok(bytes) => bytes,
            Err(e) => {
                error!("Failed to decode address {}: {}", account.address, e);
                continue;
            }
        };

        // Build 21-byte key with 0x41 prefix (TRON address format)
        let mut key = Vec::with_capacity(21);
        key.push(0x41);
        key.extend_from_slice(&addr_bytes);

        // Check if account already exists
        match engine.get(ACCOUNT_DB, &key) {
            Ok(Some(existing_data)) => {
                info!(
                    "  Account {} already exists (data_len={}), skipping genesis init",
                    account.address,
                    existing_data.len()
                );
                continue;
            }
            Ok(None) => {
                // Account doesn't exist, will create it
            }
            Err(e) => {
                warn!(
                    "  Error checking account {}: {}, attempting to create anyway",
                    account.address, e
                );
            }
        }

        // Create a minimal Account protobuf with the balance
        // Field numbers from java-tron's Account.proto:
        //   bytes address = 3;
        //   int64 balance = 4;
        //   int32 type = 1;  (Normal = 0)
        let mut proto_data = Vec::new();

        // Field 1: type = 0 (Normal account)
        // Tag = (1 << 3) | 0 = 8, value = 0
        proto_data.push(0x08); // Tag for field 1, varint
        proto_data.push(0x00); // Value 0 (Normal)

        // Field 3: address (length-delimited)
        // Tag = (3 << 3) | 2 = 26 = 0x1A
        proto_data.push(0x1A); // Tag for field 3, length-delimited
        proto_data.push(21); // Length of 21-byte address
        proto_data.extend_from_slice(&key); // 21-byte address with 0x41 prefix

        // Field 4: balance (varint)
        // Tag = (4 << 3) | 0 = 32 = 0x20
        proto_data.push(0x20); // Tag for field 4, varint

        // Encode balance as varint (handle both positive and negative)
        let balance = account.balance_sun;
        encode_signed_varint(&mut proto_data, balance);

        // Store the account
        match engine.put(ACCOUNT_DB, &key, &proto_data) {
            Ok(()) => {
                info!(
                    "  Initialized account {} with balance {} SUN{}",
                    account.address,
                    account.balance_sun,
                    if account.comment.is_empty() {
                        "".to_string()
                    } else {
                        format!(" ({})", account.comment)
                    }
                );
            }
            Err(e) => {
                error!("  Failed to store account {}: {}", account.address, e);
            }
        }
    }

    info!("=== Genesis Initialization Complete ===");
    Ok(())
}

/// Encode a signed i64 as a protobuf varint (zigzag encoding for signed)
#[cfg(feature = "storage")]
fn encode_signed_varint(output: &mut Vec<u8>, value: i64) {
    // For protobuf, signed integers use zigzag encoding for sint64,
    // but int64 uses standard varint encoding (two's complement)
    // Java's protobuf uses standard varint for int64, so we do the same
    let mut v = value as u64;
    loop {
        if v < 0x80 {
            output.push(v as u8);
            break;
        } else {
            output.push(((v & 0x7F) | 0x80) as u8);
            v >>= 7;
        }
    }
}
