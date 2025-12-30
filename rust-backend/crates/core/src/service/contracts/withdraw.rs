// WithdrawBalance contract handler
// Handles WithdrawBalanceContract (type 13) for witness reward withdrawal

use super::super::BackendService;
use super::delegation;
use super::proto::TransactionResultBuilder;
use tron_backend_execution::{TronTransaction, TronExecutionContext, TronExecutionResult, TronStateChange, WithdrawChange, EvmStateStore};
use tracing::{debug, info, warn};

/// FROZEN_PERIOD constant: 24 hours in milliseconds
const FROZEN_PERIOD_MS: i64 = 86_400_000;

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
        let owner_tron = tron_backend_common::to_tron_address(&owner_address);

        info!("Executing WITHDRAW_BALANCE_CONTRACT: owner={}", owner_tron);

        // Step 1: Validate owner account exists
        let owner_account = storage_adapter.get_account(&owner_address)
            .map_err(|e| format!("Failed to load owner account: {}", e))?
            .ok_or_else(|| format!("Account {} not found", owner_tron))?;

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
            info!(
                "Delegation reward computed for {}: {} SUN",
                owner_tron, delegation_reward
            );
        }

        // Step 5: Read allowance and add delegation reward
        let base_allowance = storage_adapter.get_account_allowance(&owner_address)
            .map_err(|e| format!("Failed to read allowance: {}", e))?;

        // Total allowance = base allowance + delegation reward
        let allowance = base_allowance.checked_add(delegation_reward)
            .ok_or("Overflow when adding delegation reward to allowance")?;

        if allowance <= 0 {
            warn!("Account {} has no reward to withdraw (allowance={})", owner_tron, allowance);
            return Err("witnessAccount does not have any reward".to_string());
        }

        debug!("Account {} has allowance={} to withdraw", owner_tron, allowance);

        // Step 5: Check for overflow when adding allowance to balance
        let old_balance_u64: u64 = owner_account.balance.try_into().unwrap_or(u64::MAX);
        let allowance_u64 = allowance as u64; // Safe since we checked allowance > 0

        let new_balance_u64 = old_balance_u64.checked_add(allowance_u64)
            .ok_or("Balance overflow when adding allowance")?;

        debug!("Balance update: {} + {} = {}", old_balance_u64, allowance_u64, new_balance_u64);

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
                .ok_or_else(|| format!("Account {} not found", owner_tron))?;

            owner_proto.allowance = 0;
            owner_proto.latest_withdraw_time = now_ms;

            storage_adapter
                .put_account_proto(&owner_address, &owner_proto)
                .map_err(|e| format!("Failed to persist owner account proto: {}", e))?;
        }

        info!("WithdrawBalance: owner={} withdrew {} SUN, new_balance={}",
              owner_tron, allowance, new_balance_u64);

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

        debug!("Emitting withdraw change: owner={}, amount={}, latest_withdraw_time={}",
               owner_tron, allowance, now_ms);

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
