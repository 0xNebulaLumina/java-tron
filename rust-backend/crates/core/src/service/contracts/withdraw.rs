// WithdrawBalance contract handler
// Handles WithdrawBalanceContract (type 13) for witness reward withdrawal

use super::super::BackendService;
use super::delegation;
use super::proto::TransactionResultBuilder;
use tron_backend_execution::{TronTransaction, TronExecutionContext, TronExecutionResult, TronStateChange, WithdrawChange, EvmStateStore};
use tracing::{debug, info, warn};

/// FROZEN_PERIOD constant: 24 hours in milliseconds
const FROZEN_PERIOD_MS: i64 = 86_400_000;

// Genesis witnesses (guard representatives) from java-tron configs.
// - Mainnet: `main_net_config.conf` genesis.block.witnesses
// - Testnet: `framework/src/test/resources/config-test.conf` genesis.block.witnesses
const TESTNET_GENESIS_GUARD_REPS: [[u8; 21]; 11] = [
    [
        0xa0, 0x29, 0x9f, 0x3d, 0xb8, 0x0a, 0x24, 0xb2, 0x0a, 0x25, 0x4b, 0x89, 0xce, 0x63,
        0x9d, 0x59, 0x13, 0x2f, 0x15, 0x7f, 0x13,
    ],
    [
        0xa0, 0x80, 0x73, 0x37, 0xf1, 0x80, 0xb6, 0x2a, 0x77, 0x57, 0x63, 0x77, 0xc1, 0xd0,
        0xc9, 0xc2, 0x4d, 0xf5, 0xc0, 0xdd, 0x62,
    ],
    [
        0xa0, 0x54, 0x30, 0xa3, 0xf0, 0x89, 0x15, 0x4e, 0x9e, 0x18, 0x2d, 0xdd, 0x6f, 0xe1,
        0x36, 0xa6, 0x23, 0x21, 0xaf, 0x22, 0xa7,
    ],
    [
        0xa0, 0x8b, 0xea, 0xa1, 0xa8, 0xe2, 0xd4, 0x53, 0x67, 0xaf, 0x7b, 0xae, 0x7c, 0x49,
        0x0b, 0x99, 0x32, 0xa4, 0xfa, 0x43, 0x01,
    ],
    [
        0xa0, 0xb0, 0x70, 0xb2, 0xb5, 0x8f, 0x43, 0x28, 0xe2, 0x93, 0xdc, 0x9d, 0x60, 0x12,
        0xf5, 0x9c, 0x26, 0x3d, 0x3a, 0x1d, 0xf6,
    ],
    [
        0xa0, 0x0a, 0x93, 0x09, 0x75, 0x85, 0x08, 0x41, 0x30, 0x39, 0xe4, 0xbc, 0x5a, 0x3d,
        0x11, 0x3f, 0x3e, 0xcc, 0x55, 0x03, 0x1d,
    ],
    [
        0xa0, 0x6a, 0x17, 0xa4, 0x96, 0x48, 0xa8, 0xad, 0x32, 0x05, 0x5c, 0x06, 0xf6, 0x0f,
        0xa1, 0x4a, 0xe4, 0x6d, 0xf9, 0x4c, 0xc1,
    ],
    [
        0xa0, 0xec, 0x65, 0x25, 0x97, 0x9a, 0x35, 0x1a, 0x54, 0xfa, 0x09, 0xfe, 0xa6, 0x4b,
        0xeb, 0x4c, 0xce, 0x33, 0xff, 0xbb, 0x7a,
    ],
    [
        0xa0, 0xfa, 0xb5, 0xfb, 0xf6, 0xaf, 0xb6, 0x81, 0xe4, 0xe3, 0x7e, 0x9d, 0x33, 0xbd,
        0xdb, 0x7e, 0x92, 0x3d, 0x61, 0x32, 0xe5,
    ],
    [
        0xa0, 0x14, 0xee, 0xbe, 0x4d, 0x30, 0xa6, 0xac, 0xb5, 0x05, 0xc8, 0xb0, 0x0b, 0x21,
        0x8b, 0xdc, 0x47, 0x33, 0x43, 0x3c, 0x68,
    ],
    [
        0xa0, 0x47, 0x11, 0xbf, 0x7a, 0xfb, 0xdf, 0x44, 0x55, 0x7d, 0xef, 0xbd, 0xf4, 0xc4,
        0xe7, 0xaa, 0x61, 0x38, 0xc6, 0x33, 0x1f,
    ],
];

const MAINNET_GENESIS_GUARD_REPS: [[u8; 21]; 27] = [
    [
        0x41, 0x50, 0x95, 0xd4, 0xf4, 0xd2, 0x6e, 0xbc, 0x67, 0x2c, 0xa1, 0x2f, 0xc0, 0xe3,
        0xa4, 0x8d, 0x6c, 0xe3, 0xb1, 0x69, 0xd2,
    ],
    [
        0x41, 0xd3, 0x2b, 0x3f, 0xa8, 0xca, 0x0b, 0x48, 0x96, 0x25, 0x7f, 0xdf, 0x18, 0x21,
        0xac, 0x8d, 0x11, 0x6d, 0xa8, 0x4c, 0x45,
    ],
    [
        0x41, 0xdf, 0x3b, 0xd4, 0xe0, 0x46, 0x35, 0x34, 0xcb, 0x7f, 0x1f, 0x3f, 0xfc, 0x2e,
        0xc1, 0x4a, 0xc4, 0x69, 0x3d, 0xc3, 0xb2,
    ],
    [
        0x41, 0x27, 0xa6, 0x41, 0x9b, 0xbe, 0x59, 0xf4, 0xe6, 0x4a, 0x06, 0x4d, 0x71, 0x07,
        0x87, 0xe5, 0x78, 0xa1, 0x50, 0xd6, 0xa7,
    ],
    [
        0x41, 0x08, 0xb5, 0x5b, 0x26, 0x11, 0xec, 0x82, 0x9d, 0x30, 0x8a, 0x62, 0xb3, 0x33,
        0x9f, 0xba, 0x9d, 0xd5, 0xc2, 0x71, 0x51,
    ],
    [
        0x41, 0x64, 0x19, 0x76, 0x5b, 0xac, 0xf1, 0xdc, 0x44, 0x1f, 0x72, 0x2c, 0xab, 0xc8,
        0xb6, 0x61, 0x14, 0x05, 0x58, 0xbb, 0x5d,
    ],
    [
        0x41, 0x4b, 0x47, 0x78, 0xbe, 0xeb, 0xb4, 0x8a, 0xbe, 0x0b, 0xc1, 0xdf, 0x42, 0xe9,
        0x2e, 0x0f, 0xe6, 0x4d, 0x0c, 0x86, 0x85,
    ],
    [
        0x41, 0x16, 0x61, 0xf2, 0x53, 0x87, 0x37, 0x0c, 0x9c, 0xd3, 0xa9, 0xa5, 0xd9, 0x7e,
        0x60, 0xca, 0x90, 0xf4, 0x84, 0x4e, 0x7e,
    ],
    [
        0x41, 0xe4, 0x0d, 0xe6, 0x89, 0x5c, 0x14, 0x2a, 0xde, 0x8b, 0x86, 0x19, 0x40, 0x63,
        0xbc, 0xdb, 0xaa, 0x6c, 0x93, 0x60, 0xb6,
    ],
    [
        0x41, 0x20, 0x7a, 0xb1, 0x58, 0x5b, 0x9c, 0xc6, 0xc4, 0xc1, 0x23, 0x2f, 0x67, 0xe4,
        0xa1, 0x0e, 0x19, 0xa4, 0x42, 0xfe, 0x68,
    ],
    [
        0x41, 0x41, 0x0e, 0x46, 0x89, 0x19, 0x15, 0x5a, 0xa8, 0x47, 0xd8, 0x3b, 0x0c, 0x20,
        0x61, 0x48, 0x51, 0x1b, 0x6d, 0xc8, 0x48,
    ],
    [
        0x41, 0x86, 0xf5, 0x79, 0x3e, 0xb6, 0x78, 0xc6, 0x5d, 0x96, 0x73, 0xd5, 0x49, 0x8c,
        0x55, 0x04, 0x39, 0xd7, 0x62, 0xc1, 0xcc,
    ],
    [
        0x41, 0x70, 0x40, 0x58, 0x31, 0x33, 0xe8, 0x31, 0x95, 0x3e, 0xa4, 0xf6, 0x5a, 0x81,
        0x96, 0xfc, 0xff, 0xcf, 0xbf, 0x0d, 0x80,
    ],
    [
        0x41, 0x2e, 0xdc, 0xe1, 0x51, 0xc8, 0x1d, 0x9b, 0x4a, 0xae, 0x17, 0xf9, 0x74, 0xf7,
        0xf6, 0x46, 0x24, 0x2e, 0xff, 0x98, 0x9d,
    ],
    [
        0x41, 0xff, 0xd5, 0x64, 0x65, 0x65, 0x56, 0xa8, 0xb6, 0xb7, 0x93, 0x11, 0xa9, 0x32,
        0xe3, 0xd2, 0x16, 0xf4, 0xfc, 0x03, 0x0b,
    ],
    [
        0x41, 0x45, 0x93, 0xd2, 0x7b, 0x70, 0xd2, 0x14, 0x54, 0xb3, 0x9a, 0xb6, 0x0b, 0xf1,
        0x32, 0x91, 0xda, 0xe8, 0xdc, 0x03, 0x26,
    ],
    [
        0x41, 0x74, 0x6e, 0x6a, 0xf4, 0xac, 0x9d, 0xb3, 0x47, 0x3c, 0x0c, 0x95, 0x5f, 0x1f,
        0xca, 0x11, 0xd4, 0x01, 0x3f, 0x32, 0xed,
    ],
    [
        0x41, 0xe7, 0x2d, 0x83, 0x3e, 0x0c, 0x46, 0x83, 0x7c, 0x08, 0x02, 0x86, 0x4a, 0xcc,
        0x5f, 0x11, 0x9a, 0x0a, 0x90, 0x4d, 0x05,
    ],
    [
        0x41, 0xf8, 0xc7, 0xac, 0xc4, 0xc0, 0x8c, 0xf3, 0x6c, 0xa0, 0x8f, 0xc2, 0xa6, 0x1b,
        0x1f, 0x5a, 0x7c, 0x8d, 0xea, 0x7b, 0xec,
    ],
    [
        0x41, 0x1d, 0x7a, 0xba, 0x13, 0xea, 0x19, 0x9a, 0x63, 0xd1, 0x64, 0x7e, 0x58, 0xe3,
        0x9c, 0x16, 0xa9, 0xbb, 0x9d, 0xa6, 0x89,
    ],
    [
        0x41, 0x06, 0x94, 0x98, 0x1b, 0x11, 0x63, 0x04, 0xed, 0x21, 0xe0, 0x58, 0x96, 0xfb,
        0x16, 0xa6, 0xbc, 0x2e, 0x91, 0xc9, 0x2c,
    ],
    [
        0x41, 0x11, 0x55, 0xd1, 0x04, 0x15, 0xfa, 0xc1, 0x6a, 0x8f, 0x4c, 0xb2, 0xf3, 0x82,
        0xce, 0x0e, 0x0f, 0x0a, 0x7e, 0x64, 0xcc,
    ],
    [
        0x41, 0x31, 0x8b, 0x2b, 0x6b, 0x4c, 0x7f, 0xca, 0xa4, 0xb6, 0x2f, 0x25, 0xa2, 0x82,
        0x32, 0x9e, 0x19, 0x52, 0xa3, 0xc0, 0xd1,
    ],
    [
        0x41, 0xa8, 0x57, 0x36, 0x2c, 0x1b, 0x77, 0xcb, 0x04, 0xe8, 0xf2, 0xb5, 0x1b, 0x6e,
        0x97, 0x0f, 0x24, 0xfa, 0x5c, 0x1e, 0x5b,
    ],
    [
        0x41, 0xa8, 0xbb, 0x76, 0x80, 0xd8, 0x5f, 0x98, 0x21, 0xb3, 0xd8, 0x25, 0x05, 0xed,
        0xc4, 0x66, 0x3f, 0x6f, 0xbd, 0x8f, 0xde,
    ],
    [
        0x41, 0x27, 0xbf, 0x0d, 0x1a, 0x57, 0xf3, 0x35, 0xc1, 0x1b, 0xc5, 0xd0, 0x02, 0xdd,
        0x82, 0xe9, 0xe0, 0x72, 0x7c, 0xb9, 0x67,
    ],
    [
        0x41, 0x72, 0xfd, 0x5d, 0xfb, 0x8a, 0xb3, 0x6e, 0xb2, 0x8d, 0xf8, 0xe4, 0xae, 0xe9,
        0x79, 0x66, 0xa6, 0x0e, 0xbf, 0x9e, 0xfe,
    ],
];

fn is_genesis_guard_representative(owner_tron: &[u8], address_prefix: u8) -> bool {
    if owner_tron.len() != 21 {
        return false;
    }
    match address_prefix {
        0xa0 => TESTNET_GENESIS_GUARD_REPS
            .iter()
            .any(|addr| addr.as_slice() == owner_tron),
        _ => MAINNET_GENESIS_GUARD_REPS
            .iter()
            .any(|addr| addr.as_slice() == owner_tron),
    }
}

impl BackendService {
    /// Execute a WITHDRAW_BALANCE_CONTRACT
    ///
    /// Phase 1 Implementation (delegation_reward_enabled = false):
    /// - Uses Account.allowance only (skips delegation/mortgage queryReward)
    ///
    /// Phase 2 Implementation (delegation_reward_enabled = true):
    /// - First computes delegation rewards via withdraw_reward()
    /// - Adds computed rewards to allowance before reading
    ///
    /// Both phases:
    /// - Validates owner exists, cooldown satisfied, and has positive allowance
    /// - Updates balance by adding allowance
    /// - Emits WithdrawChange sidecar for Java to update allowance=0 and latestWithdrawTime
    ///
    /// Validation rules (matching embedded):
    /// - Owner account must exist
    /// - Cooldown: now - latestWithdrawTime >= witnessAllowanceFrozenTime * FROZEN_PERIOD
    /// - Allowance must be positive
    /// - No overflow when adding allowance to balance
    pub(crate) fn execute_withdraw_balance_contract(
        &self,
        storage_adapter: &mut tron_backend_execution::EngineBackedEvmStateStore,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        let owner_address = transaction.from;

        // 1) Validate owner address (java-tron: DecodeUtil.addressValid)
        let prefix = storage_adapter.address_prefix();
        let owner_from_field = transaction.metadata.from_raw.as_deref().unwrap_or(&[]);
        if owner_from_field.len() != 21 || owner_from_field[0] != prefix {
            return Err("Invalid address".to_string());
        }
        let readable_owner_address = hex::encode(owner_from_field);

        debug!(
            "Executing WITHDRAW_BALANCE_CONTRACT: owner={}",
            readable_owner_address
        );

        // 2) Validate owner account exists
        let owner_account = storage_adapter
            .get_account(&owner_address)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .ok_or_else(|| format!("Account[{}] not exists", readable_owner_address))?;

        // 3) Validate not guard representative (genesis witness)
        if is_genesis_guard_representative(owner_from_field, prefix) {
            return Err(format!(
                "Account[{}] is a guard representative and is not allowed to withdraw Balance",
                readable_owner_address
            ));
        }

        debug!("Owner account loaded: balance={}", owner_account.balance);

        // Step 2: Read dynamic properties for cooldown check
        let now_ms = storage_adapter.get_latest_block_header_timestamp()
            .map_err(|e| format!("Failed to read block timestamp: {}", e))?;

        let witness_allowance_frozen_time = storage_adapter.get_witness_allowance_frozen_time()
            .map_err(|e| format!("Failed to read witness allowance frozen time: {}", e))?;

        let latest_withdraw_time = storage_adapter.get_account_latest_withdraw_time(&owner_address)
            .map_err(|e| format!("Failed to read latest withdraw time: {}", e))?;

        debug!("Cooldown check: now_ms={}, latest_withdraw_time={}, frozen_time_days={}",
               now_ms, latest_withdraw_time, witness_allowance_frozen_time);

        // Calculate cooldown period in milliseconds
        let cooldown_ms = witness_allowance_frozen_time * FROZEN_PERIOD_MS;
        let time_since_last_withdraw = now_ms - latest_withdraw_time;

        if time_since_last_withdraw < cooldown_ms {
            warn!("Cooldown not satisfied: time_since_last_withdraw={} < cooldown_ms={}",
                  time_since_last_withdraw, cooldown_ms);
            return Err(format!("The last withdraw time is {}, less than 24 hours", latest_withdraw_time));
        }

        debug!("Cooldown satisfied: {} ms since last withdraw (required: {} ms)",
               time_since_last_withdraw, cooldown_ms);

        // Step 4: Check if delegation reward computation is enabled
        // If enabled, compute delegation rewards and add to allowance
        let delegation_reward = self.compute_delegation_reward_if_enabled(storage_adapter, &owner_address)?;

        if delegation_reward > 0 {
            debug!(
                "Delegation reward computed for {}: {} SUN",
                readable_owner_address, delegation_reward
            );
        }

        // Step 5: Read allowance and add delegation reward
        let base_allowance = storage_adapter
            .get_account_allowance(&owner_address)
            .map_err(|e| format!("Failed to read allowance: {}", e))?;

        if base_allowance <= 0 && delegation_reward <= 0 {
            warn!(
                "Account {} has no reward to withdraw (allowance={}, delegation_reward={})",
                readable_owner_address, base_allowance, delegation_reward
            );
            return Err("witnessAccount does not have any reward".to_string());
        }

        // java-tron validate() checks overflow on balance + allowance (LongMath.checkedAdd).
        let old_balance_u64: u64 = owner_account
            .balance
            .try_into()
            .map_err(|_| "Invalid balance".to_string())?;
        let old_balance: i64 = old_balance_u64
            .try_into()
            .map_err(|_| "Invalid balance".to_string())?;
        if old_balance.checked_add(base_allowance).is_none() {
            return Err(format!(
                "overflow: checkedAdd({}, {})",
                old_balance, base_allowance
            ));
        }

        // Total allowance = base allowance + delegation reward
        let allowance = base_allowance
            .checked_add(delegation_reward)
            .ok_or("Overflow when adding delegation reward to allowance")?;

        debug!(
            "Account {} has allowance={} to withdraw",
            readable_owner_address, allowance
        );

        let new_balance = old_balance.checked_add(allowance).ok_or_else(|| {
            format!("overflow: checkedAdd({}, {})", old_balance, allowance)
        })?;
        let new_balance_u64: u64 = new_balance
            .try_into()
            .map_err(|_| "Invalid balance".to_string())?;

        debug!(
            "Balance update: {} + {} = {}",
            old_balance, allowance, new_balance_u64
        );

        // Step 6: Create new account with updated balance
        let mut new_owner = owner_account.clone();
        new_owner.balance = revm_primitives::U256::from(new_balance_u64);

        // Persist new owner account
        storage_adapter.set_account(owner_address, new_owner.clone())
            .map_err(|e| format!("Failed to persist owner account: {}", e))?;

        // In rust_persist_enabled mode, persist the allowance reset + latestWithdrawTime update
        // directly to the Account proto. This matches java-tron's embedded persistence and is
        // required for rust-only conformance fixtures (no Java apply path).
        let config = self.get_execution_config()?;
        if config.remote.rust_persist_enabled {
            let mut owner_proto = storage_adapter
                .get_account_proto(&owner_address)
                .map_err(|e| format!("Failed to load owner account proto: {}", e))?
                .ok_or_else(|| format!("Account[{}] not exists", readable_owner_address))?;

            owner_proto.allowance = 0;
            owner_proto.latest_withdraw_time = now_ms;

            storage_adapter
                .put_account_proto(&owner_address, &owner_proto)
                .map_err(|e| format!("Failed to persist owner account proto: {}", e))?;
        }

        debug!(
            "WithdrawBalance: owner={} withdrew {} SUN, new_balance={}",
            readable_owner_address, allowance, new_balance_u64
        );

        // Step 7: Emit AccountChange for balance delta
        let state_changes = vec![
            TronStateChange::AccountChange {
                address: owner_address,
                old_account: Some(owner_account),
                new_account: Some(new_owner),
            }
        ];

        // Step 8: Emit WithdrawChange sidecar for Java to update allowance and latestWithdrawTime
        // Java will set Account.allowance = 0 and Account.latestWithdrawTime = now_ms
        let withdraw_changes = vec![
            WithdrawChange {
                owner_address,
                amount: allowance,
                latest_withdraw_time: now_ms,
            }
        ];

        debug!(
            "Emitting withdraw change: owner={}, amount={}, latest_withdraw_time={}",
            readable_owner_address, allowance, now_ms
        );

        // Step 9: Calculate bandwidth usage
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);

        // Build Transaction.Result with withdraw_amount for receipt passthrough
        let tron_transaction_result = TransactionResultBuilder::new()
            .with_withdraw_amount(allowance)
            .build();

        debug!("WithdrawBalance completed successfully: state_changes=1, energy_used=0, bandwidth_used={}, withdraw_changes=1, tron_transaction_result_len={}",
               bandwidth_used, tron_transaction_result.len());

        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0, // System contract: zero energy
            bandwidth_used,
            state_changes,
            logs: vec![],
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![], // Not applicable
            global_resource_changes: vec![], // Not applicable
            trc10_changes: vec![], // Not applicable
            vote_changes: vec![], // Not applicable
            withdraw_changes, // WithdrawChange sidecar for Java apply
            tron_transaction_result: Some(tron_transaction_result), // Phase 0.4: Receipt passthrough with withdraw_amount
            contract_address: None, // Not applicable for withdraw contracts
        })
    }

    /// Compute delegation reward if enabled in config.
    ///
    /// When `delegation_reward_enabled` is true, calls the full withdraw_reward()
    /// computation which reads from DelegationStore and updates delegation state.
    /// When false (Phase 1), returns 0 and skips delegation computation.
    ///
    /// # Arguments
    /// * `storage_adapter` - Storage adapter
    /// * `address` - Account address
    ///
    /// # Returns
    /// * `Ok(reward)` - Delegation reward in SUN (0 if disabled)
    fn compute_delegation_reward_if_enabled(
        &self,
        storage_adapter: &tron_backend_execution::EngineBackedEvmStateStore,
        address: &revm_primitives::Address,
    ) -> Result<i64, String> {
        // Check if delegation reward computation is enabled
        let config = self.get_execution_config()?;

        if !config.remote.delegation_reward_enabled {
            debug!(
                "Delegation reward computation disabled, skipping for {}",
                tron_backend_common::to_tron_address(address)
            );
            return Ok(0);
        }

        debug!(
            "Delegation reward computation enabled, computing for {}",
            tron_backend_common::to_tron_address(address)
        );

        // Call the full delegation reward computation
        delegation::withdraw_reward(storage_adapter, address)
    }
}
