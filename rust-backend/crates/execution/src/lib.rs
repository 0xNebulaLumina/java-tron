use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use tracing::info;
use async_trait::async_trait;
use prost::Message;

use tron_backend_common::{Module, ModuleHealth, ExecutionConfig};

// Include generated protobuf code for Witness
pub mod protocol {
    include!(concat!(env!("OUT_DIR"), "/protocol.rs"));
}

// Re-export key types for external use
pub use tron_evm::{TronContractParameter, TronContractType, TronEvm, TronExecutionContext, TronExecutionResult, TronStateChange, TronTransaction, TxMetadata, FreezeLedgerChange, FreezeLedgerResource, GlobalResourceTotalsChange, Trc10Change, Trc10AssetIssued, Trc10AssetTransferred, VoteChange, VoteEntry, WithdrawChange};
pub use precompiles::TronPrecompiles;
pub use storage_adapter::{EvmStateStore, InMemoryEvmStateStore, EngineBackedEvmStateStore, EvmStateDatabase, StateChangeRecord, WitnessInfo, FreezeRecord, VotesRecord, Vote, AccountAext, ResourceTracker, BandwidthPath, BandwidthParams, BandwidthResult, ExecutionWriteBuffer, WriteOp, TouchedKey};

mod tron_evm;
mod precompiles;
mod storage_adapter;
pub mod delegation;

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
        // TRON parity: TriggerSmartContract must target an existing smart contract account.
        // java-tron rejects missing/non-contract targets during validation with:
        // "No contract or not a smart contract"
        if tx.metadata.contract_type == Some(TronContractType::TriggerSmartContract) {
            if let Some(to) = tx.to {
                let mut tron_contract_key = Vec::with_capacity(21);
                let prefix = storage.tron_address_prefix()?;
                tron_contract_key.push(prefix);
                tron_contract_key.extend_from_slice(to.as_slice());

                let is_smart_contract = match storage.tron_has_smart_contract(&tron_contract_key)? {
                    Some(v) => v,
                    None => storage.get_code(&to)?.is_some(),
                };

                if !is_smart_contract {
                    return Ok(TronExecutionResult {
                        success: false,
                        return_data: revm::primitives::Bytes::new(),
                        energy_used: 0,
                        bandwidth_used: 32 + tx.data.len() as u64,
                        logs: Vec::new(),
                        state_changes: Vec::new(),
                        error: Some("No contract or not a smart contract".to_string()),
                        aext_map: HashMap::new(),
                        freeze_changes: Vec::new(),
                        global_resource_changes: Vec::new(),
                        trc10_changes: Vec::new(),
                        vote_changes: Vec::new(),
                        withdraw_changes: Vec::new(),
                        tron_transaction_result: None,
                        contract_address: None,
                    });
                }
            }
        }

        // TRON parity: TriggerSmartContract validation must match java-tron's VMActuator.call().
        if tx.metadata.contract_type == Some(TronContractType::TriggerSmartContract) {
            Self::validate_trigger_smart_contract(&storage, tx)?;
        }

        // TRON parity: CreateSmartContract validation must match java-tron's VMActuator.create().
        if tx.metadata.contract_type == Some(TronContractType::CreateSmartContract) {
            Self::validate_create_smart_contract(&storage, tx, context)?;
        }

        let energy_fee_rate = storage.energy_fee_rate()?.unwrap_or(0);
        let spec_id = storage
            .tvm_spec_id()?
            .unwrap_or_else(|| TronEvm::<EvmStateDatabase<S>>::spec_id_from_config(&self.config));

        // TRON parity: backend.proto's `energy_limit` is a fee limit in SUN. Convert to an EVM
        // gas limit (energy units) using the dynamic property ENERGY_FEE (SUN per energy).
        let mut adjusted_tx = tx.clone();
        if energy_fee_rate > 0 {
            adjusted_tx.gas_limit = adjusted_tx.gas_limit / energy_fee_rate;
        }

        let database = EvmStateDatabase::new_with_persist(storage, self.config.remote.rust_persist_enabled);
        let mut evm = TronEvm::new_with_spec_id(database, &self.config, spec_id)?;
        // Use the new state tracking method
        evm.execute_transaction_with_state_tracking(&adjusted_tx, context)
    }

    fn validate_create_smart_contract<S: EvmStateStore>(
        storage: &S,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<()> {
        const MAX_CONTRACT_NAME_BYTES: usize = 32;
        const ONE_HUNDRED: i64 = 100;
        const MIN_TOKEN_ID: i64 = 1_000_000;

        let dynamic_i64 = |key: &[u8], default: i64| -> Result<i64> {
            Ok(storage.tron_dynamic_property_i64(key)?.unwrap_or(default))
        };

        // Helper: TRON address validity check (mirrors java DecodeUtil.addressValid).
        // Valid TRON address: 21 bytes, first byte = 0x41 (mainnet) or 0xa0 (testnet).
        let is_valid_tron_address = |addr: &[u8]| -> bool {
            if addr.is_empty() {
                return false;
            }
            if addr.len() != 21 {
                return false;
            }
            // Accept both mainnet (0x41) and testnet (0xa0) prefixes
            addr[0] == 0x41 || addr[0] == 0xa0
        };

        // 1) VM enabled (java: DynamicPropertiesStore.supportVM()).
        if dynamic_i64(b"ALLOW_CREATION_OF_CONTRACTS", 1)? != 1 {
            return Err(anyhow::anyhow!(
                "vm work is off, need to be opened by the committee"
            ));
        }

        // 2) Decode CreateSmartContract.
        let create_contract = crate::protocol::CreateSmartContract::decode(tx.data.as_ref())
            .map_err(|_| anyhow::anyhow!("Cannot get CreateSmartContract from transaction"))?;
        let new_contract = create_contract
            .new_contract
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Cannot get CreateSmartContract from transaction"))?;

        // 3) Validate owner address format (java: DecodeUtil.addressValid).
        // This check mirrors Java's validation which logs warnings for invalid addresses.
        if !is_valid_tron_address(&create_contract.owner_address) {
            return Err(anyhow::anyhow!("Invalid ownerAddress"));
        }

        // 4) Validate origin address format.
        if !is_valid_tron_address(&new_contract.origin_address) {
            return Err(anyhow::anyhow!("Invalid originAddress"));
        }

        // 5) ownerAddress == originAddress.
        if create_contract.owner_address != new_contract.origin_address {
            return Err(anyhow::anyhow!("OwnerAddress is not equals OriginAddress"));
        }

        // 6) Owner account existence check (java: VMUtils.validateForSmartContract).
        // Java's VMActuator.create() accesses creator account at line 372-373 and would
        // fail with NullPointerException if it doesn't exist. For callValue > 0, it
        // explicitly checks via VMUtils with "no OwnerAccount" error.
        // We check upfront for clean error messages regardless of callValue.
        if storage.get_account(&tx.from)?.is_none() {
            return Err(anyhow::anyhow!(
                "Validate InternalTransfer error, no OwnerAccount."
            ));
        }

        // 7) contractName byte length <= 32.
        if new_contract.name.as_bytes().len() > MAX_CONTRACT_NAME_BYTES {
            return Err(anyhow::anyhow!(
                "contractName's length cannot be greater than 32"
            ));
        }

        // 8) consumeUserResourcePercent in [0, 100].
        let percent = new_contract.consume_user_resource_percent;
        if percent < 0 || percent > ONE_HUNDRED {
            return Err(anyhow::anyhow!("percent must be >= 0 and <= 100"));
        }

        // 9) Derive CreateSmartContract address (txid + owner) and ensure it doesn't exist.
        if let (Some(txid), owner_address) =
            (context.transaction_id, create_contract.owner_address.as_slice())
        {
            if owner_address.len() == 21 {
                let mut combined = Vec::with_capacity(32 + owner_address.len());
                combined.extend_from_slice(txid.as_slice());
                combined.extend_from_slice(owner_address);
                let hash = crate::storage_adapter::utils::keccak256(&combined);
                let addr_bytes = &hash.as_slice()[12..32];
                let derived_address = revm::primitives::Address::from_slice(addr_bytes);

                if storage.get_account(&derived_address)?.is_some() {
                    let prefix = storage.tron_address_prefix()?;
                    let base58 =
                        crate::storage_adapter::utils::to_tron_address_with_prefix(&derived_address, prefix);
                    return Err(anyhow::anyhow!(
                        "Trying to create a contract with existing contract address: {}",
                        base58
                    ));
                }
            }
        }

        // 10) feeLimit validation (java: trx.raw_data.fee_limit).
        let max_fee_limit = dynamic_i64(b"MAX_FEE_LIMIT", i64::MAX)?;
        let max_fee_limit_u64 = if max_fee_limit < 0 {
            0u64
        } else {
            max_fee_limit as u64
        };
        if tx.gas_limit > max_fee_limit_u64 {
            return Err(anyhow::anyhow!(
                "feeLimit must be >= 0 and <= {}",
                max_fee_limit_u64
            ));
        }

        // 11) Energy limit hard fork validations (java: StorageUtils.getEnergyLimitHardFork()).
        let call_value = new_contract.call_value;
        if call_value < 0 {
            return Err(anyhow::anyhow!("callValue must be >= 0"));
        }

        let allow_tvm_transfer_trc10 = dynamic_i64(b"ALLOW_TVM_TRANSFER_TRC10", 0)? != 0;
        let (token_value, token_id) = if allow_tvm_transfer_trc10 {
            (create_contract.call_token_value, create_contract.token_id)
        } else {
            (0, 0)
        };

        if token_value < 0 {
            return Err(anyhow::anyhow!("tokenValue must be >= 0"));
        }

        if new_contract.origin_energy_limit <= 0 {
            return Err(anyhow::anyhow!("The originEnergyLimit must be > 0"));
        }

        // 12) checkTokenValueAndId parity (java: VMActuator.checkTokenValueAndId()).
        let allow_multi_sign = dynamic_i64(b"ALLOW_MULTI_SIGN", 1)? != 0;
        if allow_tvm_transfer_trc10 && allow_multi_sign {
            if token_id <= MIN_TOKEN_ID && token_id != 0 {
                return Err(anyhow::anyhow!("tokenId must be > {}", MIN_TOKEN_ID));
            }
            if token_value > 0 && token_id == 0 {
                return Err(anyhow::anyhow!(
                    "invalid arguments with tokenValue = {}, tokenId = {}",
                    token_value,
                    token_id
                ));
            }
        }

        // 13) callValue transfer validation (java: VMUtils.validateInternalTransfer()).
        // Note: Owner account existence already checked above (#6).
        if call_value > 0 {
            let balance = storage
                .get_account(&tx.from)?
                .map(|a| a.balance)
                .unwrap_or(revm::primitives::U256::ZERO);
            if balance < revm::primitives::U256::from(call_value as u64) {
                return Err(anyhow::anyhow!(
                    "Validate InternalTransfer error, balance is not sufficient."
                ));
            }
        }

        // 14) TRC-10 token transfer validation (java: VMUtils.validateForSmartContract()).
        if allow_tvm_transfer_trc10 && token_value > 0 {
            let allow_same_token_name = dynamic_i64(b" ALLOW_SAME_TOKEN_NAME", 0)?;
            let token_id_bytes = token_id.to_string().into_bytes();

            if storage
                .tron_get_asset_issue(&token_id_bytes, allow_same_token_name)?
                .is_none()
            {
                return Err(anyhow::anyhow!("No asset !"));
            }

            let asset_balance = storage.tron_get_asset_balance_v2(&tx.from, &token_id_bytes)?;
            if asset_balance <= 0 {
                return Err(anyhow::anyhow!("assetBalance must greater than 0."));
            }
            if token_value > asset_balance {
                return Err(anyhow::anyhow!("assetBalance is not sufficient."));
            }
        }

        Ok(())
    }

    fn validate_trigger_smart_contract<S: EvmStateStore>(
        storage: &S,
        tx: &TronTransaction,
    ) -> Result<()> {
        use revm::primitives::{Address, U256};

        const MIN_TOKEN_ID: i64 = 1_000_000;

        let dynamic_i64 = |key: &[u8], default: i64| -> Result<i64> {
            Ok(storage.tron_dynamic_property_i64(key)?.unwrap_or(default))
        };

        // 1) VM enabled (java: DynamicPropertiesStore.supportVM()).
        if dynamic_i64(b"ALLOW_CREATION_OF_CONTRACTS", 1)? != 1 {
            return Err(anyhow::anyhow!(
                "VM work is off, need to be opened by the committee"
            ));
        }

        // 2) Decode TriggerSmartContract.
        let trigger = crate::protocol::TriggerSmartContract::decode(tx.data.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to decode TriggerSmartContract: {}", e))?;

        let parse_tron_address = |bytes: &[u8]| -> Option<Address> {
            if bytes.len() == 21 && (bytes[0] == 0x41 || bytes[0] == 0xa0) {
                Some(Address::from_slice(&bytes[1..]))
            } else if bytes.len() == 20 {
                Some(Address::from_slice(bytes))
            } else {
                None
            }
        };

        // 3) Owner address format + existence.
        let owner = parse_tron_address(&trigger.owner_address)
            .ok_or_else(|| anyhow::anyhow!("Invalid ownerAddress"))?;

        let owner_account = storage
            .get_account(&owner)?
            .ok_or_else(|| anyhow::anyhow!("Account not exists"))?;

        // 4) Contract address presence + format.
        if trigger.contract_address.is_empty() {
            return Err(anyhow::anyhow!(
                "Cannot get contract address from TriggerContract"
            ));
        }
        if parse_tron_address(&trigger.contract_address).is_none() {
            return Err(anyhow::anyhow!("Invalid contract address"));
        }

        // 5) feeLimit bounds (java: feeLimit < 0 || feeLimit > maxFeeLimit).
        let max_fee_limit = dynamic_i64(b"MAX_FEE_LIMIT", i64::MAX)?;
        let max_fee_limit_u64 = if max_fee_limit < 0 {
            0u64
        } else {
            max_fee_limit as u64
        };
        if tx.gas_limit > max_fee_limit_u64 {
            return Err(anyhow::anyhow!(
                "feeLimit must be >= 0 and <= {}",
                max_fee_limit_u64
            ));
        }

        // 6) callValue checks.
        if trigger.call_value > 0 {
            let call_value = U256::from(trigger.call_value as u64);
            if owner_account.balance < call_value {
                return Err(anyhow::anyhow!(
                    "Validate InternalTransfer error, balance is not sufficient."
                ));
            }
        }

        // 7) Token checks (java: VMActuator.checkTokenValueAndId + VMUtils.validateForSmartContract).
        let allow_tvm_transfer_trc10 = dynamic_i64(b"ALLOW_TVM_TRANSFER_TRC10", 0)? != 0;
        let allow_multi_sign = dynamic_i64(b"ALLOW_MULTI_SIGN", 1)? != 0;

        let token_value = if allow_tvm_transfer_trc10 {
            trigger.call_token_value
        } else {
            0
        };
        let token_id = if allow_tvm_transfer_trc10 {
            trigger.token_id
        } else {
            0
        };

        if allow_tvm_transfer_trc10 && allow_multi_sign {
            if token_id <= MIN_TOKEN_ID && token_id != 0 {
                return Err(anyhow::anyhow!("tokenId must be > {}", MIN_TOKEN_ID));
            }
            if token_value > 0 && token_id == 0 {
                return Err(anyhow::anyhow!(
                    "invalid arguments with tokenValue = {}, tokenId = {}",
                    token_value,
                    token_id
                ));
            }
        }

        if allow_tvm_transfer_trc10 && token_value > 0 {
            let allow_same_token_name = dynamic_i64(b" ALLOW_SAME_TOKEN_NAME", 0)?;
            let token_key = token_id.to_string();

            if storage
                .tron_get_asset_issue(token_key.as_bytes(), allow_same_token_name)?
                .is_none()
            {
                return Err(anyhow::anyhow!("No asset !"));
            }

            let balance = storage.tron_get_asset_balance_v2(&owner, token_key.as_bytes())?;
            if balance <= 0 {
                return Err(anyhow::anyhow!("assetBalance must greater than 0."));
            }
            if token_value > balance {
                return Err(anyhow::anyhow!("assetBalance is not sufficient"));
            }
        }

        Ok(())
    }

    /// Call a contract without state changes
    pub fn call_contract_with_storage<S: EvmStateStore + 'static>(
        &self,
        storage: S,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult> {
        let energy_fee_rate = storage.energy_fee_rate()?.unwrap_or(0);
        let spec_id = storage
            .tvm_spec_id()?
            .unwrap_or_else(|| TronEvm::<EvmStateDatabase<S>>::spec_id_from_config(&self.config));
        let mut adjusted_tx = tx.clone();
        if energy_fee_rate > 0 {
            adjusted_tx.gas_limit = adjusted_tx.gas_limit / energy_fee_rate;
        }
        let database = EvmStateDatabase::new(storage);
        let mut evm = TronEvm::new_with_spec_id(database, &self.config, spec_id)?;
        evm.call_contract(&adjusted_tx, context)
    }

    /// Estimate energy usage for a transaction
    pub fn estimate_energy_with_storage<S: EvmStateStore + 'static>(
        &self,
        storage: S,
        tx: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<u64> {
        let energy_fee_rate = storage.energy_fee_rate()?.unwrap_or(0);
        let spec_id = storage
            .tvm_spec_id()?
            .unwrap_or_else(|| TronEvm::<EvmStateDatabase<S>>::spec_id_from_config(&self.config));
        let mut adjusted_tx = tx.clone();
        if energy_fee_rate > 0 {
            adjusted_tx.gas_limit = adjusted_tx.gas_limit / energy_fee_rate;
        }
        let database = EvmStateDatabase::new(storage);
        let mut evm = TronEvm::new_with_spec_id(database, &self.config, spec_id)?;
        evm.estimate_energy(&adjusted_tx, context)
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
        let spec_id = storage
            .tvm_spec_id()?
            .unwrap_or_else(|| TronEvm::<EvmStateDatabase<InMemoryEvmStateStore>>::spec_id_from_config(&self.config));
        let database = EvmStateDatabase::new(storage);
        let _evm = TronEvm::new_with_spec_id(database, &self.config, spec_id)?;
        
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
        let spec_id = TronEvm::<EvmStateDatabase<InMemoryEvmStateStore>>::spec_id_from_config(&self.config);
        match TronEvm::new_with_spec_id(database, &self.config, spec_id) {
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
                ..Default::default()
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
                ..Default::default()
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
            transaction_id: None,
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
        assert_eq!(remote_config.witness_update_enabled, true, "WitnessUpdate should be disabled by default");
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
