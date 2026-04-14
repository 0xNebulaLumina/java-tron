use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub execution: ExecutionConfig,
    pub modules: HashMap<String, ModuleConfig>,
    /// Genesis account initialization configuration
    #[serde(default)]
    pub genesis: GenesisConfig,
}

/// Genesis account initialization configuration.
/// Allows pre-populating accounts with balances at startup for testing/parity.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenesisConfig {
    /// Whether to initialize genesis accounts at startup
    #[serde(default)]
    pub enabled: bool,
    /// List of accounts to initialize with their balances
    #[serde(default)]
    pub accounts: Vec<GenesisAccount>,
}

/// A single genesis account entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisAccount {
    /// Base58-encoded TRON address (e.g., "TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy")
    pub address: String,
    /// Initial balance in SUN (1 TRX = 1,000,000 SUN)
    pub balance_sun: i64,
    /// Optional comment/description for documentation
    #[serde(default)]
    pub comment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub max_connections: usize,
    pub keepalive_timeout: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub data_dir: String,
    pub max_open_files: i32,
    pub cache_size: usize,
    pub write_buffer_size: usize,
    pub max_write_buffer_number: i32,
    pub compression: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    pub max_call_depth: usize,
    pub max_code_size: usize,
    pub max_init_code_size: usize,
    pub enable_london_fork: bool,
    pub enable_berlin_fork: bool,
    pub enable_istanbul_fork: bool,
    // Tron-specific
    pub energy_limit: u64,
    pub bandwidth_limit: u64,
    pub max_cpu_time_of_one_tx: u64,
    /// For TRON parity: suppress EVM-style coinbase/miner payouts (default: false for parity)
    pub evm_eth_coinbase_compat: bool,
    /// Skip precompile address collision check for CREATE opcode (java-tron parity).
    /// Java's VMActuator doesn't check if CREATE-derived addresses collide with precompile addresses.
    /// This is extremely unlikely to happen in practice, but we gate the check for strict parity.
    /// Default: true (skip the check to match Java behavior)
    pub skip_precompile_create_collision_check: bool,
    /// TRON fee handling configuration
    pub fees: ExecutionFeeConfig,
    /// Remote execution feature flags
    pub remote: RemoteExecutionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionFeeConfig {
    /// Fee handling mode: "burn", "blackhole", or "none"
    /// - "burn": No state delta for fees (supply reduction handled elsewhere)
    /// - "blackhole": Credit fees to designated blackhole address
    /// - "none": No fee handling (useful for testing)
    pub mode: String,

    /// Whether black hole optimization is supported (matches java-tron's supportBlackHoleOptimization)
    pub support_black_hole_optimization: bool,

    /// Base58-encoded TRON address for blackhole (required if mode = "blackhole")
    pub blackhole_address_base58: String,

    /// Experimental: emit synthetic VM blackhole credits (default: false)
    /// When enabled, VM transactions will emit estimated fee credits to blackhole
    /// This is an approximation and should remain off by default
    pub experimental_vm_blackhole_credit: bool,

    /// Optional flat fee for non-VM transactions in SUN (when not reading from dynamic properties)
    /// If None, no fee deltas are emitted for non-VM transactions
    pub non_vm_blackhole_credit_flat: Option<u64>,
}

/// Remote-execution feature flags for the Rust backend.
///
/// The per-flag classification (EE baseline / RR experimental / RR canonical /
/// legacy) lives in `planning/close_loop.config_convergence.md`. The
/// `RemoteExecutionConfig::default()` values below represent the
/// **conservative** profile (almost everything `false`). The checked-in
/// `rust-backend/config.toml` overrides these defaults to produce the
/// **experimental** profile used for Phase 1 parity work.
///
/// When you add a new flag, add a row to `close_loop.config_convergence.md`
/// in the same change and pick a classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteExecutionConfig {
    /// Global enable/disable for remote system contract execution
    pub system_enabled: bool,
    /// Enable WITNESS_CREATE_CONTRACT execution
    pub witness_create_enabled: bool,
    /// Enable WITNESS_UPDATE_CONTRACT execution
    pub witness_update_enabled: bool,
    /// Enable VOTE_WITNESS_CONTRACT execution
    pub vote_witness_enabled: bool,
    /// Enable TRC-10 transfers (requires additional storage support)
    pub trc10_enabled: bool,
    /// Enable FREEZE_BALANCE_CONTRACT execution
    pub freeze_balance_enabled: bool,
    /// Enable UNFREEZE_BALANCE_CONTRACT execution
    pub unfreeze_balance_enabled: bool,
    /// Enable FREEZE_BALANCE_V2_CONTRACT execution
    pub freeze_balance_v2_enabled: bool,
    /// Enable UNFREEZE_BALANCE_V2_CONTRACT execution
    pub unfreeze_balance_v2_enabled: bool,
    /// Enable WITHDRAW_BALANCE_CONTRACT execution
    /// Phase 1: Uses Account.allowance only (skips delegation/mortgage reward)
    pub withdraw_balance_enabled: bool,
    /// Emit storage changes for freeze ledger (EXPERIMENTAL - may affect CSV output)
    /// Default: false to maintain CSV parity with Phase 1
    pub emit_freeze_ledger_changes: bool,
    /// Emit GlobalResourceTotalsChange alongside freeze/unfreeze operations
    /// When enabled, backend computes and sends total net/energy weight and limits
    /// so Java can update DynamicPropertiesStore immediately (fixes FREE_NET vs ACCOUNT_NET divergence)
    /// Default: false for backward compatibility; enable true for Phase 2 parity runs
    pub emit_global_resource_changes: bool,
    /// Emit storage changes for witness/vote data (may affect CSV output)
    pub emit_storage_changes: bool,
    /// AEXT (Account EXTension) presence mode for AccountInfo serialization
    /// Controls how AEXT tail (76 bytes of resource usage fields) is populated
    /// - "none": All resource fields set to None (current behavior, remote omits AEXT)
    /// - "zeros": Set Some(0)/false for EOAs (enables AEXT presence parity with embedded)
    /// - "defaults": Set window sizes to 28800, other fields to 0/false for EOAs (matches embedded defaults exactly)
    /// - "tracked": Some(real values) when backend supports resource metrics (future)
    /// Default: "none" for backward compatibility; set to "defaults" for full CSV parity with embedded
    pub accountinfo_aext_mode: String,
    /// Seed old_votes from Account.votes on first VotesRecord creation
    /// When true: On first VoteWitness for an account, old_votes is seeded from Account.votes field
    /// When false: On first VoteWitness, old_votes is empty (legacy remote behavior)
    /// Default: true to match embedded semantics
    pub vote_witness_seed_old_from_account: bool,
    /// Enable ACCOUNT_CREATE_CONTRACT execution
    /// Creates new accounts with proper fee charging and blackhole handling
    /// Default: false for safe rollout - falls back to Java embedded execution when disabled
    pub account_create_enabled: bool,
    /// DEPRECATED: Delegation reward is now always computed when CHANGE_DELEGATION is enabled,
    /// matching Java's MortgageService.withdrawReward() which self-gates on allowChangeDelegation().
    /// This field is kept for backward config compatibility but has no effect.
    #[serde(default)]
    pub delegation_reward_enabled: bool,

    /// Genesis guard representative addresses (Base58-encoded TRON addresses).
    /// These addresses are blocked from WithdrawBalance operations, matching Java's
    /// `CommonParameter.getInstance().getGenesisBlock().getWitnesses()` check.
    ///
    /// When empty (default), falls back to hardcoded mainnet/testnet genesis witness lists.
    /// Set this for custom/private networks with different genesis witnesses.
    #[serde(default)]
    pub genesis_guard_representatives_base58: Vec<String>,

    // === Write Consistency Model (close_loop Phase 1) ===
    //
    // Canonical policy: planning/close_loop.write_ownership.md.
    //
    // The system has two potential write paths:
    // 1. Rust handler writes to RocksDB via the buffered storage adapter.
    // 2. Java `RuntimeSpiImpl.apply*` reflects state changes back into the local chainbase.
    //
    // Double-write is prevented by the `write_mode` guard: when Rust returns
    // `write_mode = PERSISTED`, Java skips `apply*` and only runs `postExecMirror`.
    //
    // Phase 1 profile mapping:
    // - `RR` canonical (Phase 1 acceptance):   rust_persist_enabled = true
    //     Rust is the authoritative writer. Java is a read-side mirror.
    // - `RR` compute-only (development only):  rust_persist_enabled = false
    //     Rust computes, Java applies. Developer/diagnostic mode only.
    //     Results from this mode are NOT citable as RR parity.
    // - `EE` baseline:                          flag ignored, Rust backend not hit.
    //
    // The code default is `false` so a fresh install without any config
    // changes is the conservative compute-only profile. The checked-in
    // `rust-backend/config.toml` sets `true` for the Phase 1 canonical
    // RR profile. Do not flip the checked-in value without updating
    // `close_loop.write_ownership.md` in the same change.
    /// Whether Rust handlers should persist state changes directly to storage.
    ///
    /// See planning/close_loop.write_ownership.md for the policy.
    /// Code default: `false` (compute-only / development profile).
    /// `rust-backend/config.toml` default: `true` (canonical `RR` profile).
    pub rust_persist_enabled: bool,

    // === Phase 2.A: Proposal Contracts (16/17/18) ===
    //
    // Proposal contracts are governance operations for network parameter changes.
    // They have minimal dependencies (ProposalStore, WitnessStore, DynamicPropertiesStore)
    // and don't require complex Account field mutations, making them ideal first candidates.
    /// Enable PROPOSAL_CREATE_CONTRACT (type 16) execution
    /// Creates new proposals with parameters, expiration time, and initial state
    /// Default: false for safe rollout
    pub proposal_create_enabled: bool,

    /// Enable PROPOSAL_APPROVE_CONTRACT (type 17) execution
    /// Allows witnesses to add/remove their approval from proposals
    /// Default: false for safe rollout
    pub proposal_approve_enabled: bool,

    /// Enable PROPOSAL_DELETE_CONTRACT (type 18) execution
    /// Allows proposal creator to cancel their proposal before expiration
    /// Default: false for safe rollout
    pub proposal_delete_enabled: bool,

    /// Proposal expiration time in milliseconds (matches CommonParameter.getProposalExpireTime())
    /// Default: 3 days = 259200000 ms
    pub proposal_expire_time_ms: u64,

    // === Phase 2.B: Account Management Contracts (19/46) ===
    //
    // These contracts test the Account codec implementation by modifying
    // account fields like account_id and permissions.
    /// Enable SET_ACCOUNT_ID_CONTRACT (type 19) execution
    /// Sets a unique, immutable account ID for an account
    /// Requires: AccountStore (full Account proto read/write), AccountIdIndexStore
    /// Default: false for safe rollout
    pub set_account_id_enabled: bool,

    /// Enable ACCOUNT_PERMISSION_UPDATE_CONTRACT (type 46) execution
    /// Updates owner/witness/active permissions for multi-sig functionality
    /// Requires: AccountStore (permissions fields), DynamicPropertiesStore (ALLOW_MULTI_SIGN, UPDATE_ACCOUNT_PERMISSION_FEE, etc.)
    /// Default: false for safe rollout
    pub account_permission_update_enabled: bool,

    // === Phase 2.C: Contract Metadata Contracts (33/45/48) ===
    //
    // These contracts modify smart contract metadata fields (consume_user_resource_percent,
    // origin_energy_limit, ABI). They require ContractStore and AbiStore access.
    /// Enable UPDATE_SETTING_CONTRACT (type 33) execution
    /// Updates consume_user_resource_percent field of a smart contract
    /// Requires: ContractStore (SmartContract proto read/write), AccountStore (owner validation)
    /// Default: false for safe rollout
    pub update_setting_enabled: bool,

    /// Enable UPDATE_ENERGY_LIMIT_CONTRACT (type 45) execution
    /// Updates origin_energy_limit field of a smart contract
    /// Requires: ContractStore (SmartContract proto read/write), AccountStore (owner validation)
    /// Gate: checkForEnergyLimit() - block_num >= BLOCK_NUM_FOR_ENERGY_LIMIT
    /// Default: false for safe rollout
    pub update_energy_limit_enabled: bool,

    /// Enable CLEAR_ABI_CONTRACT (type 48) execution
    /// Clears ABI of a smart contract by writing default ABI to AbiStore
    /// Requires: AbiStore (ABI write), ContractStore (owner validation), AccountStore
    /// Gate: getAllowTvmConstantinople() != 0
    /// Default: false for safe rollout
    pub clear_abi_enabled: bool,

    // === Phase 2.C2: UpdateBrokerage Contract (49) ===
    //
    // UpdateBrokerage allows witnesses to set their commission rate for delegation rewards.
    // The brokerage percentage (0-100) is stored in DelegationStore.
    /// Enable UPDATE_BROKERAGE_CONTRACT (type 49) execution
    /// Updates the brokerage (commission rate) for a witness in DelegationStore
    /// Requires: WitnessStore (witness validation), AccountStore (account validation)
    /// Gate: allowChangeDelegation() must be true
    /// Default: false for safe rollout
    pub update_brokerage_enabled: bool,

    // === Phase 2.D: Resource/Freeze/Delegation Contracts (56/57/58/59) ===
    //
    // These contracts handle the UnfreezeV2/Delegation lifecycle:
    // - WithdrawExpireUnfreeze (56): Withdraw TRX from expired unfrozenV2 entries
    // - DelegateResource (57): Delegate frozen resources to another account
    // - UnDelegateResource (58): Reclaim delegated resources
    // - CancelAllUnfreezeV2 (59): Cancel pending unfreezes and optionally withdraw expired
    /// Enable WITHDRAW_EXPIRE_UNFREEZE_CONTRACT (type 56) execution
    /// Withdraws TRX from unfrozenV2 entries whose expiration has passed
    /// Requires: AccountStore (unfrozenV2 list access), DynamicPropertiesStore (timestamp)
    /// Gate: supportUnfreezeDelay() must be true
    /// Receipt: withdraw_expire_amount
    /// Default: false for safe rollout
    pub withdraw_expire_unfreeze_enabled: bool,

    /// Enable DELEGATE_RESOURCE_CONTRACT (type 57) execution
    /// Delegates frozen resources (bandwidth/energy) to another account
    /// Requires: AccountStore, DelegatedResourceStore, DelegatedResourceAccountIndexStore
    /// Gate: supportDR() and supportUnfreezeDelay() must be true
    /// Default: false for safe rollout
    pub delegate_resource_enabled: bool,

    /// Enable UNDELEGATE_RESOURCE_CONTRACT (type 58) execution
    /// Reclaims delegated resources from a receiver
    /// Requires: AccountStore, DelegatedResourceStore, DelegatedResourceAccountIndexStore
    /// Gate: supportDR() and supportUnfreezeDelay() must be true
    /// Default: false for safe rollout
    pub undelegate_resource_enabled: bool,

    /// Enable CANCEL_ALL_UNFREEZE_V2_CONTRACT (type 59) execution
    /// Cancels all pending unfreezeV2 entries, re-freezing unexpired and withdrawing expired
    /// Requires: AccountStore, DynamicPropertiesStore (for weights and timestamp)
    /// Gate: supportAllowCancelAllUnfreezeV2() must be true
    /// Receipt: withdraw_expire_amount + cancel_unfreezeV2_amount map
    /// Default: false for safe rollout
    pub cancel_all_unfreeze_v2_enabled: bool,

    // === Phase 2.E: TRC-10 Extension Contracts (9/14/15) ===
    //
    // These contracts handle TRC-10 token operations beyond basic transfer and issuance:
    // - ParticipateAssetIssue (9): Participate in a TRC-10 token sale
    // - UnfreezeAsset (14): Unfreeze frozen TRC-10 asset supply
    // - UpdateAsset (15): Update TRC-10 asset metadata (url, description, limits)
    /// Enable PARTICIPATE_ASSET_ISSUE_CONTRACT (type 9) execution
    /// Allows users to participate in a TRC-10 token sale by exchanging TRX for tokens
    /// Requires: AccountStore (balance + asset map), AssetIssueStore/V2, DynamicPropertiesStore
    /// Default: false for safe rollout
    pub participate_asset_issue_enabled: bool,

    /// Enable UNFREEZE_ASSET_CONTRACT (type 14) execution
    /// Unfreezes frozen TRC-10 supply and returns it to the asset issuer's balance
    /// Requires: AccountStore (frozen_supply + asset map), AssetIssueStore/V2
    /// Default: false for safe rollout
    pub unfreeze_asset_enabled: bool,

    /// Enable UPDATE_ASSET_CONTRACT (type 15) execution
    /// Updates TRC-10 asset metadata: url, description, free_asset_net_limit, public_free_asset_net_limit
    /// Requires: AccountStore (asset_issued_id), AssetIssueStore/V2
    /// Default: false for safe rollout
    pub update_asset_enabled: bool,

    // === Phase 2.F: Exchange Contracts (41/42/43/44) ===
    //
    // These contracts handle the Bancor-style exchange (AMM) functionality:
    // - ExchangeCreate (41): Create a new exchange pair with initial liquidity
    // - ExchangeInject (42): Add liquidity to an existing exchange
    // - ExchangeWithdraw (43): Remove liquidity from an exchange (creator only)
    // - ExchangeTransaction (44): Swap tokens using the AMM
    //
    // Dependencies:
    // - ExchangeStore / ExchangeV2Store (dbName: "exchange" / "exchange-v2")
    // - AccountStore (TRX + asset balances)
    // - AssetIssueStore (for allowSameTokenName=0 token name→id resolution)
    // - DynamicPropertiesStore (latestExchangeNum, exchangeBalanceLimit, exchangeCreateFee,
    //                           allowStrictMath, allowSameTokenName, supportBlackHoleOptimization)
    //
    // Receipt fields:
    // - ExchangeCreate: exchange_id
    // - ExchangeInject: exchange_inject_another_amount
    // - ExchangeWithdraw: exchange_withdraw_another_amount
    // - ExchangeTransaction: exchange_received_amount
    /// Enable EXCHANGE_CREATE_CONTRACT (type 41) execution
    /// Creates a new exchange pair with initial token balances
    /// Fee: getExchangeCreateFee() from DynamicPropertiesStore
    /// Requires: AccountStore, ExchangeStore/V2, DynamicPropertiesStore
    /// Default: false for safe rollout
    pub exchange_create_enabled: bool,

    /// Enable EXCHANGE_INJECT_CONTRACT (type 42) execution
    /// Injects additional liquidity into an existing exchange (creator only)
    /// Calculates proportional amount of the other token
    /// Requires: AccountStore, ExchangeStore/V2, DynamicPropertiesStore
    /// Default: false for safe rollout
    pub exchange_inject_enabled: bool,

    /// Enable EXCHANGE_WITHDRAW_CONTRACT (type 43) execution
    /// Withdraws liquidity from an exchange (creator only)
    /// Calculates proportional amount of the other token
    /// Requires: AccountStore, ExchangeStore/V2, DynamicPropertiesStore
    /// Default: false for safe rollout
    pub exchange_withdraw_enabled: bool,

    /// Enable EXCHANGE_TRANSACTION_CONTRACT (type 44) execution
    /// Executes a token swap using the Bancor AMM formula
    /// Requires: AccountStore, ExchangeStore/V2, DynamicPropertiesStore
    /// Default: false for safe rollout
    pub exchange_transaction_enabled: bool,

    // === Phase 2.G: Market (DEX) Contracts (52/53) ===
    //
    // These contracts handle the order-book DEX functionality:
    // - MarketSellAsset (52): Create a sell order and match against existing orders
    // - MarketCancelOrder (53): Cancel an existing active order
    //
    // Dependencies:
    // - MarketAccountStore (dbName: "market_account") - per-account order tracking
    // - MarketOrderStore (dbName: "market_order") - order storage
    // - MarketPairToPriceStore (dbName: "market_pair_to_price") - pair→price count
    // - MarketPairPriceToOrderStore (dbName: "market_pair_price_to_order") - price→order list
    // - AccountStore (TRX + asset balances)
    // - AssetIssueStore/V2 (token validation)
    // - DynamicPropertiesStore (allowMarketTransaction, marketSellFee, marketCancelFee, marketQuantityLimit)
    //
    // Receipt fields:
    // - MarketSellAsset: orderId + orderDetails[]
    // - MarketCancelOrder: (no additional fields)
    //
    // Note: MarketSellAsset is complex due to order matching with price comparison,
    // linked list management, and MAX_MATCH_NUM limit.
    /// Enable MARKET_SELL_ASSET_CONTRACT (type 52) execution
    /// Creates a sell order and matches against existing orders
    /// Fee: getMarketSellFee() from DynamicPropertiesStore
    /// Requires: All Market stores, AccountStore, AssetIssueStore/V2, DynamicPropertiesStore
    /// Default: false for safe rollout
    pub market_sell_asset_enabled: bool,

    /// Enable MARKET_CANCEL_ORDER_CONTRACT (type 53) execution
    /// Cancels an existing active order and returns remaining tokens
    /// Fee: getMarketCancelFee() from DynamicPropertiesStore
    /// Requires: MarketOrderStore, MarketAccountStore, MarketPairToPriceStore,
    ///           MarketPairPriceToOrderStore, AccountStore, DynamicPropertiesStore
    /// Default: false for safe rollout
    pub market_cancel_order_enabled: bool,

    /// Strict market index parity mode
    ///
    /// Java's MarketCancelOrderActuator throws ItemNotFoundException when these are missing:
    /// - MarketAccountStore.get(owner) during updateOrderState()
    /// - MarketPairPriceToOrderStore.get(pairPriceKey) in the cancel actuator
    /// - Neighbor orders referenced by prev/next pointers during linked-list removal
    ///
    /// Rust's default behavior is more permissive (treats missing entries as optional and continues).
    ///
    /// When enabled:
    /// - Missing MarketAccountOrder for an active order cancel → error
    /// - Missing MarketOrderIdList for the order's pairPriceKey → error
    /// - Missing neighbor orders (when prev/next is non-empty) → error
    ///
    /// Default: false for backward compatibility (defensive recovery)
    /// Set to true for strict Java parity
    pub market_strict_index_parity: bool,

    // === Dynamic Property Strictness (Task 5 parity) ===
    //
    // Java's DynamicPropertiesStore throws IllegalArgumentException when keys are missing,
    // but the store initialization catches these and saves defaults. In practice, a properly
    // initialized store always has these keys.
    //
    // Rust defaults to safe fallback values for robustness, but this can mask configuration
    // issues or cause subtle divergence in edge cases.
    //
    // When strict mode is enabled, Rust will return errors when required dynamic properties
    // are missing, matching Java's "throw when missing" behavior.
    /// Strict dynamic property mode for Java parity
    ///
    /// When enabled, getters for critical dynamic properties will return errors when keys
    /// are missing, rather than using default values. This matches Java's behavior and helps
    /// catch configuration issues early.
    ///
    /// Affected properties for AssetIssueContract:
    /// - ASSET_ISSUE_FEE
    /// - TOKEN_ID_NUM
    /// - ALLOW_SAME_TOKEN_NAME
    /// - ONE_DAY_NET_LIMIT
    /// - MIN_FROZEN_SUPPLY_TIME, MAX_FROZEN_SUPPLY_TIME, MAX_FROZEN_SUPPLY_NUMBER
    ///
    /// Default: false for backward compatibility (uses safe defaults)
    /// Set to true for conformance testing or strict Java parity
    pub strict_dynamic_properties: bool,

    /// Genesis block timestamp in milliseconds since epoch.
    /// Used to compute headSlot for bandwidth resource windows:
    ///   headSlot = (block_timestamp_ms - genesis_block_timestamp) / 3000
    /// Default: 1529891469000 (TRON mainnet genesis block timestamp)
    #[serde(default = "default_genesis_block_timestamp")]
    pub genesis_block_timestamp: i64,
}

fn default_genesis_block_timestamp() -> i64 {
    1529891469000 // TRON mainnet genesis block timestamp (ms)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleConfig {
    pub enabled: bool,
    pub settings: HashMap<String, serde_json::Value>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            storage: StorageConfig::default(),
            execution: ExecutionConfig::default(),
            modules: HashMap::new(),
            genesis: GenesisConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 50011,
            max_connections: 1000,
            keepalive_timeout: 60,
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: "./data".to_string(),
            max_open_files: 1000,
            cache_size: 128 * 1024 * 1024,       // 128MB
            write_buffer_size: 64 * 1024 * 1024, // 64MB
            max_write_buffer_number: 3,
            compression: "lz4".to_string(),
        }
    }
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            max_call_depth: 1024,
            max_code_size: 24576,
            max_init_code_size: 49152,
            enable_london_fork: true,
            enable_berlin_fork: true,
            enable_istanbul_fork: true,
            // Tron defaults
            energy_limit: 100_000_000,
            bandwidth_limit: 5000,
            max_cpu_time_of_one_tx: 80,
            evm_eth_coinbase_compat: false, // Default off for TRON parity
            skip_precompile_create_collision_check: true, // Default true for TRON parity (Java doesn't have this check)
            fees: ExecutionFeeConfig::default(),
            remote: RemoteExecutionConfig::default(),
        }
    }
}

impl Default for ExecutionFeeConfig {
    fn default() -> Self {
        Self {
            mode: "burn".to_string(), // Default to burn mode for TRON parity
            support_black_hole_optimization: true, // Match java-tron default
            blackhole_address_base58: String::new(), // Empty by default, required if mode = "blackhole"
            experimental_vm_blackhole_credit: false, // Default off to avoid double-counting
            non_vm_blackhole_credit_flat: None,      // No flat fee emission by default
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, config::ConfigError> {
        let mut builder = config::Config::builder()
            .add_source(config::File::with_name("config").required(false))
            .add_source(config::Environment::with_prefix("TRON_BACKEND").separator("__"));

        // Add default values
        builder = builder.set_default("server.host", "127.0.0.1")?;
        builder = builder.set_default("server.port", 50011)?;
        builder = builder.set_default("server.max_connections", 1000)?;
        builder = builder.set_default("server.keepalive_timeout", 60)?;

        builder = builder.set_default("storage.data_dir", "./data")?;
        builder = builder.set_default("storage.max_open_files", 1000)?;
        builder = builder.set_default("storage.cache_size", 128 * 1024 * 1024)?;
        builder = builder.set_default("storage.write_buffer_size", 64 * 1024 * 1024)?;
        builder = builder.set_default("storage.max_write_buffer_number", 3)?;
        builder = builder.set_default("storage.compression", "lz4")?;

        builder = builder.set_default("execution.max_call_depth", 1024)?;
        builder = builder.set_default("execution.max_code_size", 24576)?;
        builder = builder.set_default("execution.max_init_code_size", 49152)?;
        builder = builder.set_default("execution.enable_london_fork", true)?;
        builder = builder.set_default("execution.enable_berlin_fork", true)?;
        builder = builder.set_default("execution.enable_istanbul_fork", true)?;
        builder = builder.set_default("execution.energy_limit", 100_000_000u64)?;
        builder = builder.set_default("execution.bandwidth_limit", 5000u64)?;
        builder = builder.set_default("execution.max_cpu_time_of_one_tx", 80u64)?;
        builder = builder.set_default("execution.evm_eth_coinbase_compat", false)?;
        builder = builder.set_default("execution.skip_precompile_create_collision_check", true)?;

        // Fee configuration defaults
        builder = builder.set_default("execution.fees.mode", "burn")?;
        builder = builder.set_default("execution.fees.support_black_hole_optimization", true)?;
        builder = builder.set_default("execution.fees.blackhole_address_base58", "")?;
        builder = builder.set_default("execution.fees.experimental_vm_blackhole_credit", false)?;
        // non_vm_blackhole_credit_flat is Option<u64>, leave unset for None default

        // Remote execution configuration defaults
        builder = builder.set_default("execution.remote.system_enabled", true)?;
        builder = builder.set_default("execution.remote.witness_create_enabled", true)?;
        builder = builder.set_default("execution.remote.witness_update_enabled", true)?;
        builder = builder.set_default("execution.remote.vote_witness_enabled", false)?;
        builder = builder.set_default("execution.remote.trc10_enabled", false)?;
        builder = builder.set_default("execution.remote.freeze_balance_enabled", false)?;
        builder = builder.set_default("execution.remote.unfreeze_balance_enabled", false)?;
        builder = builder.set_default("execution.remote.freeze_balance_v2_enabled", false)?;
        builder = builder.set_default("execution.remote.unfreeze_balance_v2_enabled", false)?;
        builder = builder.set_default("execution.remote.withdraw_balance_enabled", false)?;
        builder = builder.set_default("execution.remote.emit_freeze_ledger_changes", false)?;
        builder = builder.set_default("execution.remote.emit_global_resource_changes", false)?;
        builder = builder.set_default("execution.remote.emit_storage_changes", false)?;
        builder = builder.set_default("execution.remote.accountinfo_aext_mode", "none")?;
        builder =
            builder.set_default("execution.remote.vote_witness_seed_old_from_account", true)?;
        builder = builder.set_default("execution.remote.account_create_enabled", false)?;
        builder = builder.set_default("execution.remote.delegation_reward_enabled", false)?;
        // Phase 0.3: Default false - Rust computes only, Java apply handles persistence
        builder = builder.set_default("execution.remote.rust_persist_enabled", false)?;

        // Phase 2.A: Proposal contracts (16/17/18)
        builder = builder.set_default("execution.remote.proposal_create_enabled", false)?;
        builder = builder.set_default("execution.remote.proposal_approve_enabled", false)?;
        builder = builder.set_default("execution.remote.proposal_delete_enabled", false)?;
        builder = builder.set_default("execution.remote.proposal_expire_time_ms", 259200000u64)?; // 3 days

        // Phase 2.B: Account management contracts (19/46)
        builder = builder.set_default("execution.remote.set_account_id_enabled", false)?;
        builder =
            builder.set_default("execution.remote.account_permission_update_enabled", false)?;

        // Phase 2.C: Contract metadata contracts (33/45/48)
        builder = builder.set_default("execution.remote.update_setting_enabled", false)?;
        builder = builder.set_default("execution.remote.update_energy_limit_enabled", false)?;
        builder = builder.set_default("execution.remote.clear_abi_enabled", false)?;

        // Phase 2.C2: UpdateBrokerage contract (49)
        builder = builder.set_default("execution.remote.update_brokerage_enabled", false)?;

        // Phase 2.D: Resource/Freeze/Delegation contracts (56/57/58/59)
        builder =
            builder.set_default("execution.remote.withdraw_expire_unfreeze_enabled", false)?;
        builder = builder.set_default("execution.remote.delegate_resource_enabled", false)?;
        builder = builder.set_default("execution.remote.undelegate_resource_enabled", false)?;
        builder = builder.set_default("execution.remote.cancel_all_unfreeze_v2_enabled", false)?;

        // Phase 2.E: TRC-10 Extension contracts (9/14/15)
        builder = builder.set_default("execution.remote.participate_asset_issue_enabled", false)?;
        builder = builder.set_default("execution.remote.unfreeze_asset_enabled", false)?;
        builder = builder.set_default("execution.remote.update_asset_enabled", false)?;

        // Phase 2.F: Exchange contracts (41/42/43/44)
        builder = builder.set_default("execution.remote.exchange_create_enabled", false)?;
        builder = builder.set_default("execution.remote.exchange_inject_enabled", false)?;
        builder = builder.set_default("execution.remote.exchange_withdraw_enabled", false)?;
        builder = builder.set_default("execution.remote.exchange_transaction_enabled", false)?;

        // Phase 2.G: Market (DEX) contracts (52/53)
        builder = builder.set_default("execution.remote.market_sell_asset_enabled", false)?;
        builder = builder.set_default("execution.remote.market_cancel_order_enabled", false)?;

        // Genesis block timestamp for headSlot computation
        builder =
            builder.set_default("execution.remote.genesis_block_timestamp", 1529891469000i64)?;

        let config = builder.build()?;
        config.try_deserialize()
    }
}

impl Default for RemoteExecutionConfig {
    fn default() -> Self {
        Self {
            system_enabled: true,
            witness_create_enabled: true,
            witness_update_enabled: true,
            vote_witness_enabled: false,
            trc10_enabled: false,
            freeze_balance_enabled: false, // Default false until validated
            unfreeze_balance_enabled: false, // Default false until validated
            freeze_balance_v2_enabled: false, // Default false until validated
            unfreeze_balance_v2_enabled: false, // Default false until validated
            withdraw_balance_enabled: false, // Default false until validated
            emit_freeze_ledger_changes: false, // Default false for CSV parity
            emit_global_resource_changes: false, // Default false for backward compatibility
            emit_storage_changes: false,
            accountinfo_aext_mode: "none".to_string(), // Default to current behavior
            vote_witness_seed_old_from_account: true,  // Default true to match embedded semantics
            account_create_enabled: false,             // Default false for safe rollout
            delegation_reward_enabled: false, // Deprecated: delegation reward is always computed
            genesis_guard_representatives_base58: Vec::new(), // Empty = use hardcoded fallback
            // Phase 0.3: Default false - Rust computes only, Java apply handles persistence
            rust_persist_enabled: false,
            // Phase 2.A: Proposal contracts (16/17/18)
            proposal_create_enabled: false, // Default false for safe rollout
            proposal_approve_enabled: false, // Default false for safe rollout
            proposal_delete_enabled: false, // Default false for safe rollout
            proposal_expire_time_ms: 259200000, // 3 days in milliseconds
            // Phase 2.B: Account management contracts (19/46)
            set_account_id_enabled: false, // Default false for safe rollout
            account_permission_update_enabled: false, // Default false for safe rollout
            // Phase 2.C: Contract metadata contracts (33/45/48)
            update_setting_enabled: false, // Default false for safe rollout
            update_energy_limit_enabled: false, // Default false for safe rollout
            clear_abi_enabled: false,      // Default false for safe rollout
            // Phase 2.C2: UpdateBrokerage contract (49)
            update_brokerage_enabled: false, // Default false for safe rollout
            // Phase 2.D: Resource/Freeze/Delegation contracts (56/57/58/59)
            withdraw_expire_unfreeze_enabled: false, // Default false for safe rollout
            delegate_resource_enabled: false,        // Default false for safe rollout
            undelegate_resource_enabled: false,      // Default false for safe rollout
            cancel_all_unfreeze_v2_enabled: false,   // Default false for safe rollout
            // Phase 2.E: TRC-10 Extension contracts (9/14/15)
            participate_asset_issue_enabled: false, // Default false for safe rollout
            unfreeze_asset_enabled: false,          // Default false for safe rollout
            update_asset_enabled: false,            // Default false for safe rollout
            // Phase 2.F: Exchange contracts (41/42/43/44)
            exchange_create_enabled: false, // Default false for safe rollout
            exchange_inject_enabled: false, // Default false for safe rollout
            exchange_withdraw_enabled: false, // Default false for safe rollout
            exchange_transaction_enabled: false, // Default false for safe rollout
            // Phase 2.G: Market (DEX) contracts (52/53)
            market_sell_asset_enabled: false, // Default false for safe rollout
            market_cancel_order_enabled: false, // Default false for safe rollout
            market_strict_index_parity: false, // Default false for backward compatibility (defensive recovery)
            // Task 5: Dynamic property strictness
            strict_dynamic_properties: false, // Default false for backward compatibility
            // Genesis block timestamp for headSlot computation
            genesis_block_timestamp: default_genesis_block_timestamp(),
        }
    }
}
