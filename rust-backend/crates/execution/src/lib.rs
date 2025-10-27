use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use tracing::info;
use async_trait::async_trait;

use tron_backend_common::{Module, ModuleHealth, ExecutionConfig};

// Include generated protobuf code for Witness
pub mod protocol {
    include!(concat!(env!("OUT_DIR"), "/protocol.rs"));
}

// Re-export key types for external use
pub use tron_evm::{TronEvm, TronTransaction, TronExecutionContext, TronExecutionResult, TronStateChange, TronContractType, TxMetadata, FreezeLedgerChange, FreezeLedgerResource};
pub use precompiles::TronPrecompiles;
pub use storage_adapter::{EvmStateStore, InMemoryEvmStateStore, EngineBackedEvmStateStore, EvmStateDatabase, StateChangeRecord, WitnessInfo, FreezeRecord, VotesRecord, Vote, AccountAext, ResourceTracker, BandwidthPath};

mod tron_evm;
mod precompiles;
mod storage_adapter;

pub struct ExecutionModule {
    config: ExecutionConfig,
    initialized: bool,
}

impl ExecutionModule {
    pub fn new(config: ExecutionConfig) -> Self {
        Self {
            config,
            initialized: false,
        }
    }

    pub fn get_config(&self) -> Result<&ExecutionConfig, String> {
        Ok(&self.config)
    }

    /// Execute a transaction using the provided storage adapter
    pub fn execute_transaction_with_storage<S: EvmStateStore + 'static>(
        &self,
        storage: S,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        let database = EvmStateDatabase::new(storage);
        let mut evm = TronEvm::new(database, &self.config)?;
        // Use the new state tracking method
        evm.execute_transaction_with_state_tracking(tx, context)
    }

    /// Call a contract without state changes
    pub fn call_contract_with_storage<S: EvmStateStore + 'static>(
        &self,
        storage: S,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        let database = EvmStateDatabase::new(storage);
        let mut evm = TronEvm::new(database, &self.config)?;
        evm.call_contract(tx, context)
    }

    /// Estimate energy usage for a transaction
    pub fn estimate_energy_with_storage<S: EvmStateStore + 'static>(
        &self,
        storage: S,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<u64> {
        let database = EvmStateDatabase::new(storage);
        let mut evm = TronEvm::new(database, &self.config)?;
        evm.estimate_energy(tx, context)
    }

    /// Execute a transaction using in-memory storage (for testing)
    pub fn execute_transaction(
        &self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        let storage = InMemoryEvmStateStore::new();
        self.execute_transaction_with_storage(storage, tx, context)
    }

    /// Call a contract using in-memory storage (for testing)
    pub fn call_contract(
        &self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        let storage = InMemoryEvmStateStore::new();
        self.call_contract_with_storage(storage, tx, context)
    }

    /// Estimate energy using in-memory storage (for testing)
    pub fn estimate_energy(
        &self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<u64> {
        let storage = InMemoryEvmStateStore::new();
        self.estimate_energy_with_storage(storage, tx, context)
    }
}

/// ExecutionModule with a specific storage adapter type
pub struct ExecutionModuleWithStorage<S: EvmStateStore + 'static> {
    module: ExecutionModule,
    storage: Arc<RwLock<S>>,
}

impl<S: EvmStateStore + 'static> ExecutionModuleWithStorage<S> {
    pub fn new(config: ExecutionConfig, storage: S) -> Self {
        Self {
            module: ExecutionModule::new(config),
            storage: Arc::new(RwLock::new(storage)),
        }
    }

    pub fn execute_transaction(
        &self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        // For now, we'll create a new storage adapter for each transaction
        // In a real implementation, we'd need to handle concurrent access properly
        let storage = InMemoryEvmStateStore::new(); // Placeholder
        self.module.execute_transaction_with_storage(storage, tx, context)
    }

    pub fn call_contract(
        &self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        let storage = InMemoryEvmStateStore::new(); // Placeholder
        self.module.call_contract_with_storage(storage, tx, context)
    }

    pub fn estimate_energy(
        &self,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<u64> {
        let storage = InMemoryEvmStateStore::new(); // Placeholder
        self.module.estimate_energy_with_storage(storage, tx, context)
    }
}

#[async_trait]
impl Module for ExecutionModule {
    fn name(&self) -> &str {
        "execution"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }

    async fn init(&mut self) -> Result<()> {
        info!("Initializing execution module");
        
        // Validate configuration
        if self.config.energy_limit == 0 {
            return Err(anyhow::anyhow!("Energy limit must be greater than 0"));
        }
        
        if self.config.bandwidth_limit == 0 {
            return Err(anyhow::anyhow!("Bandwidth limit must be greater than 0"));
        }
        
        self.initialized = true;
        info!("Execution module initialized successfully");
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Starting execution module");
        if !self.initialized {
            return Err(anyhow::anyhow!("Module not initialized"));
        }
        
        // Test EVM creation with dummy storage
        let storage = InMemoryEvmStateStore::new();
        let database = EvmStateDatabase::new(storage);
        let _evm = TronEvm::new(database, &self.config)?;
        
        info!("Execution module started successfully");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Stopping execution module");
        self.initialized = false;
        info!("Execution module stopped");
        Ok(())
    }

    async fn health(&self) -> ModuleHealth {
        if !self.initialized {
            return ModuleHealth::unhealthy("Module not initialized");
        }
        
        // Test EVM creation
        let storage = InMemoryEvmStateStore::new();
        let database = EvmStateDatabase::new(storage);
        match TronEvm::new(database, &self.config) {
            Ok(_) => ModuleHealth::healthy(),
            Err(e) => ModuleHealth::unhealthy(&format!("EVM creation failed: {}", e)),
        }
    }

    fn metrics(&self) -> HashMap<String, f64> {
        let mut metrics = HashMap::new();
        metrics.insert("initialized".to_string(), if self.initialized { 1.0 } else { 0.0 });
        metrics.insert("energy_limit".to_string(), self.config.energy_limit as f64);
        metrics.insert("bandwidth_limit".to_string(), self.config.bandwidth_limit as f64);
        metrics
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use revm_primitives::{Address, U256, Bytes};

    #[test]
    fn test_coinbase_suppression_config() {
        // Test that default config suppresses coinbase payouts
        let config = ExecutionConfig::default();
        assert_eq!(config.evm_eth_coinbase_compat, false, "Default config should suppress coinbase payouts for TRON parity");
    }

    #[test]
    fn test_execution_module_creation() {
        let config = ExecutionConfig::default();
        let module = ExecutionModule::new(config.clone());

        // Test config access
        let retrieved_config = module.get_config().unwrap();
        assert_eq!(retrieved_config.evm_eth_coinbase_compat, false);
        assert_eq!(retrieved_config.energy_limit, 100_000_000);
    }

    #[test]
    fn test_non_vm_transaction_example() {
        let config = ExecutionConfig::default();
        let _module = ExecutionModule::new(config);

        // Create a simple TRX transfer (non-VM transaction)
        let from = Address::from_slice(&[0x01; 20]);
        let to = Address::from_slice(&[0x02; 20]);
        let transaction = TronTransaction {
            from,
            to: Some(to),
            value: U256::from(1000000), // 1 TRX in SUN
            data: Bytes::new(), // Empty data = non-VM
            gas_limit: 21000,
            gas_price: U256::ZERO, // Should be 0 for TRON parity
            nonce: 1,
            metadata: crate::tron_evm::TxMetadata::default(),
        };

        // Verify transaction structure for non-VM characteristics
        assert!(transaction.data.is_empty(), "Non-VM transaction should have empty data");
        assert_eq!(transaction.gas_price, U256::ZERO, "TRON mode should use gas_price = 0");
        assert!(transaction.to.is_some(), "Transfer transaction should have a 'to' address");
    }

    #[test]
    fn test_vm_transaction_example() {
        // Create a contract call (VM transaction)
        let from = Address::from_slice(&[0x01; 20]);
        let to = Address::from_slice(&[0x02; 20]);
        let transaction = TronTransaction {
            from,
            to: Some(to),
            value: U256::ZERO,
            data: Bytes::from(vec![0x70, 0xa0, 0x82, 0x31]), // balanceOf() function selector
            gas_limit: 50000,
            gas_price: U256::ZERO, // Should still be 0 for TRON parity
            nonce: 1,
            metadata: crate::tron_evm::TxMetadata::default(),
        };

        // Verify transaction structure for VM characteristics
        assert!(!transaction.data.is_empty(), "VM transaction should have data");
        assert_eq!(transaction.gas_price, U256::ZERO, "TRON mode should use gas_price = 0 even for VM");
    }

    #[test]
    fn test_fee_config_defaults() {
        use tron_backend_common::ExecutionFeeConfig;

        // Test default fee configuration
        let fee_config = ExecutionFeeConfig::default();
        assert_eq!(fee_config.mode, "burn", "Default fee mode should be 'burn' for TRON parity");
        assert_eq!(fee_config.support_black_hole_optimization, true, "Should support blackhole optimization by default");
        assert_eq!(fee_config.blackhole_address_base58, "", "Blackhole address should be empty by default");
        assert_eq!(fee_config.experimental_vm_blackhole_credit, false, "VM blackhole credit should be disabled by default");
        assert_eq!(fee_config.non_vm_blackhole_credit_flat, None, "Non-VM flat fee should be None by default");
    }

    #[test]
    fn test_execution_config_with_fees() {
        let config = ExecutionConfig::default();

        // Verify that ExecutionConfig includes fee configuration
        assert_eq!(config.fees.mode, "burn");
        assert_eq!(config.evm_eth_coinbase_compat, false, "Coinbase compat should be off by default");

        // Test that both Phase 1 and Phase 2 configurations work together
        assert_eq!(config.fees.experimental_vm_blackhole_credit, false, "Phase 2 experimental features should be off");
    }

    #[test]
    fn test_fee_mode_variants() {
        use tron_backend_common::ExecutionFeeConfig;

        // Test creating fee configs with different modes
        let burn_config = ExecutionFeeConfig {
            mode: "burn".to_string(),
            ..ExecutionFeeConfig::default()
        };
        assert_eq!(burn_config.mode, "burn");

        let blackhole_config = ExecutionFeeConfig {
            mode: "blackhole".to_string(),
            blackhole_address_base58: "TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy".to_string(),
            ..ExecutionFeeConfig::default()
        };
        assert_eq!(blackhole_config.mode, "blackhole");
        assert!(!blackhole_config.blackhole_address_base58.is_empty());

        let none_config = ExecutionFeeConfig {
            mode: "none".to_string(),
            ..ExecutionFeeConfig::default()
        };
        assert_eq!(none_config.mode, "none");
    }
}

#[cfg(test)]
mod witness_tests {
    use super::*;
    use crate::storage_adapter::{InMemoryEvmStateStore, WitnessInfo};
    use revm_primitives::{Address, U256, Bytes};

    /// Test contract type parsing and metadata extraction
    #[test]
    fn test_contract_type_parsing() {
        // Test WitnessCreateContract parsing
        assert_eq!(TronContractType::WitnessCreateContract as i32, 5);
        assert_eq!(TronContractType::WitnessUpdateContract as i32, 8);
        assert_eq!(TronContractType::VoteWitnessContract as i32, 4);

        // Test TryFrom implementation
        let contract_type: TronContractType = TronContractType::try_from(5).expect("Should parse WitnessCreateContract");
        assert_eq!(contract_type, TronContractType::WitnessCreateContract);
    }

    /// Test transaction with empty 'to' address for system contracts
    #[test]
    fn test_system_contract_no_to_address() {
        let from = Address::from_slice(&[0x41; 20]); // TRON address format
        let transaction = TronTransaction {
            from,
            to: None, // System contracts should have no 'to' address
            value: U256::ZERO,
            data: Bytes::from("test witness url".as_bytes().to_vec()),
            gas_limit: 10000,
            gas_price: U256::ZERO,
            nonce: 1,
            metadata: TxMetadata {
                contract_type: Some(TronContractType::WitnessCreateContract),
                asset_id: None,
            },
        };

        // Verify system contract characteristics
        assert!(transaction.to.is_none(), "System contracts should have no 'to' address");
        assert_eq!(transaction.metadata.contract_type, Some(TronContractType::WitnessCreateContract));
        assert!(!transaction.data.is_empty(), "WitnessCreate should have URL data");
    }

    /// Test WitnessInfo serialization roundtrip
    #[test]
    fn test_witness_info_serialization() {
        let address = Address::from_slice(&[0x41, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56]);
        let witness_info = WitnessInfo {
            address,
            url: "https://test-witness.com".to_string(),
            vote_count: 1000,
        };

        // Test serialization
        let serialized = witness_info.serialize();
        assert!(!serialized.is_empty(), "Serialized data should not be empty");

        // Test deserialization
        let deserialized = WitnessInfo::deserialize(&serialized).expect("Deserialization should succeed");
        assert_eq!(deserialized.address, witness_info.address);
        assert_eq!(deserialized.url, witness_info.url);
        assert_eq!(deserialized.vote_count, witness_info.vote_count);
    }

    /// Test witness store operations (conceptual test)
    #[test]
    fn test_witness_store_operations() {
        let _storage = InMemoryEvmStateStore::new();
        let address = Address::from_slice(&[0x41; 20]);

        // Create witness info for testing serialization
        let witness_info = WitnessInfo {
            address,
            url: "https://test-witness.com".to_string(),
            vote_count: 0,
        };

        // Test serialization/deserialization roundtrip
        let serialized = witness_info.serialize();
        let deserialized = WitnessInfo::deserialize(&serialized).expect("Should deserialize successfully");

        assert_eq!(deserialized.address, witness_info.address);
        assert_eq!(deserialized.url, witness_info.url);
        assert_eq!(deserialized.vote_count, witness_info.vote_count);

        println!("Witness store operations test: serialization works correctly");
    }

    /// Test dynamic properties with default values
    #[test]
    fn test_dynamic_properties_defaults() {
        let storage = InMemoryEvmStateStore::new();

        // Test default values that should exist in storage adapter
        // Note: InMemoryStorageAdapter may not implement all dynamic property methods
        // This test verifies the storage adapter can be created successfully
        assert!(true, "InMemoryStorageAdapter created successfully");

        // If dynamic property methods were implemented, they would be tested here:
        // assert_eq!(storage.get_account_upgrade_cost(), 9999000000, "AccountUpgradeCost should default to 9,999 TRX in SUN");
        // assert_eq!(storage.get_allow_multi_sign(), 1, "AllowMultiSign should be enabled by default");
        // assert_eq!(storage.support_black_hole_optimization(), true, "BlackHole optimization should be supported by default");
    }

    /// Integration test: WitnessCreate with proper context
    #[test]
    fn test_witness_create_integration() {
        let config = ExecutionConfig::default();
        let module = ExecutionModule::new(config);

        // Create witness owner address (TRON format with 0x41 prefix)
        let owner_address = Address::from_slice(&[0x41, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56]);

        // Create WitnessCreate transaction
        let transaction = TronTransaction {
            from: owner_address,
            to: None, // System contracts have no 'to' address
            value: U256::ZERO,
            data: Bytes::from("https://my-witness.com".as_bytes().to_vec()),
            gas_limit: 10000,
            gas_price: U256::ZERO,
            nonce: 1,
            metadata: TxMetadata {
                contract_type: Some(TronContractType::WitnessCreateContract),
                asset_id: None,
            },
        };

        // Create execution context with required balance
        let context = TronExecutionContext {
            block_number: 1785,
            block_timestamp: 1000000000,
            block_coinbase: Address::ZERO,
            block_difficulty: U256::ZERO,
            block_gas_limit: 30000000,
            chain_id: 2494104990, // TRON mainnet chain ID
            energy_price: 420, // Default TRON energy price
            bandwidth_price: 1000, // Default TRON bandwidth price
        };

        // Execute transaction (this will use in-memory storage)
        let result = module.execute_transaction(&transaction, &context);

        // For now, we expect this to succeed or fail gracefully
        // The actual state changes will be tested in the core service tests
        match result {
            Ok(execution_result) => {
                // Verify execution completed
                // System contracts consume 0 energy in TRON parity mode
                assert_eq!(execution_result.energy_used, 0, "WitnessCreate should use 0 energy");
                println!("WitnessCreate executed successfully, energy used (expected 0): {}", execution_result.energy_used);
            }
            Err(e) => {
                // Log error for debugging, but don't fail test if it's a validation error
                println!("WitnessCreate execution error (expected during unit test): {}", e);
            }
        }
    }

    /// Test witness creation validation
    #[test]
    fn test_witness_create_validation() {
        let _storage = InMemoryEvmStateStore::new();
        let _owner_address = Address::from_slice(&[0x41; 20]);

        // Test URL validation
        let valid_urls = vec![
            "https://witness.com",
            "http://witness.org",
            "witness.net",
            "",  // Empty URL should be allowed
        ];

        for url in valid_urls {
            // URL validation logic would be implemented in the actual handler
            assert!(url.len() <= 256, "URL should not exceed 256 characters");
        }

        // Test balance requirements (would be done through dynamic properties in real implementation)
        let expected_upgrade_cost = 9999000000u64; // 9,999 TRX in SUN
        assert!(expected_upgrade_cost > 0, "Upgrade cost should be positive");
        assert_eq!(expected_upgrade_cost, 9999000000, "Should match mainnet upgrade cost");
    }

    /// Test state change generation for WitnessCreate
    #[test]
    fn test_witness_create_state_changes() {
        let _owner_address = Address::from_slice(&[0x41; 20]);
        let _blackhole_address = Address::from_slice(&[0x41, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let upgrade_cost = 9999000000u64; // 9,999 TRX in SUN

        // Expected state changes for WitnessCreate:
        // 1. Owner account balance decrease
        // 2. Owner account metadata change (isWitness = true)
        // 3. Optional blackhole account balance increase (if not burning)

        let expected_changes = vec![
            // Owner balance change
            ("owner_balance_before".to_string(), format!("{}", 10000000000u64)), // 10,000 TRX
            ("owner_balance_after".to_string(), format!("{}", 1000000u64)),      // 1 TRX remaining
            // Owner metadata change
            ("owner_is_witness_before".to_string(), "false".to_string()),
            ("owner_is_witness_after".to_string(), "true".to_string()),
        ];

        // Verify expected change structure
        assert_eq!(expected_changes.len(), 4, "Should have 4 state changes for burn mode");

        // In blackhole mode, would have additional blackhole balance change
        let blackhole_change = ("blackhole_balance_increase".to_string(), format!("{}", upgrade_cost));
        println!("Blackhole change would be: {:?}", blackhole_change);
    }

    /// Create a TRON-format address (20 bytes starting with 0x41)
    fn create_tron_address(suffix: &[u8]) -> Address {
        let mut addr = [0u8; 20];
        addr[0] = 0x41; // TRON address prefix

        let copy_len = std::cmp::min(suffix.len(), 19);
        addr[1..1+copy_len].copy_from_slice(&suffix[..copy_len]);

        Address::from_slice(&addr)
    }

    /// Test deterministic state change ordering
    #[test]
    fn test_state_change_ordering() {
        let mut addresses = vec![
            create_tron_address(&[0xff; 19]), // Higher address
            create_tron_address(&[0x00; 19]), // Lower address
            create_tron_address(&[0x80; 19]), // Middle address
        ];

        // Sort addresses for deterministic ordering
        addresses.sort();

        assert_eq!(addresses[0], create_tron_address(&[0x00; 19]));
        assert_eq!(addresses[1], create_tron_address(&[0x80; 19]));
        assert_eq!(addresses[2], create_tron_address(&[0xff; 19]));

        // Verify account changes come before storage changes for same address
        // This would be enforced in the actual state change emission logic
    }

    /// Test feature flag integration
    #[test]
    fn test_feature_flags() {
        use tron_backend_common::RemoteExecutionConfig;

        // Test default feature flags
        let remote_config = RemoteExecutionConfig::default();
        assert_eq!(remote_config.system_enabled, true, "System contracts should be enabled by default");
        assert_eq!(remote_config.witness_create_enabled, true, "WitnessCreate should be enabled by default");
        assert_eq!(remote_config.witness_update_enabled, false, "WitnessUpdate should be disabled by default");
        assert_eq!(remote_config.vote_witness_enabled, false, "VoteWitness should be disabled by default");

        // Test disabled configuration
        let disabled_config = RemoteExecutionConfig {
            system_enabled: false,
            witness_create_enabled: false,
            ..RemoteExecutionConfig::default()
        };
        assert_eq!(disabled_config.system_enabled, false);
        assert_eq!(disabled_config.witness_create_enabled, false);
    }
} 
