//! Delegation reward computation for WithdrawBalanceContract.
//!
//! This module ports the `MortgageService.withdrawReward()` logic from Java to Rust,
//! enabling Rust to compute delegation rewards independently.
//!
//! Java references:
//! - MortgageService.java:89-134 (withdrawReward)
//! - MortgageService.java:199-230 (computeReward)
//! - DelegationStore.java (all key formats and storage)

use num_bigint::BigInt;
use revm::primitives::Address;
use tracing::{debug, info, warn};
use tron_backend_execution::delegation::{
    AccountVoteSnapshot, DelegationVote, DECIMAL_OF_VI_REWARD, DELEGATION_STORE_REMARK,
};
use tron_backend_execution::{EngineBackedEvmStateStore, EvmStateStore};

/// Compute and apply delegation reward for an address.
///
/// This is the Rust implementation of MortgageService.withdrawReward().
/// It computes delegation rewards across cycles and updates the delegation store state.
///
/// # Arguments
/// * `storage_adapter` - Storage adapter for reading/writing delegation data
/// * `address` - Account address to compute rewards for
///
/// # Returns
/// * `Ok(reward)` - Total delegation reward computed (in SUN)
/// * `Err(e)` - Error if computation fails
///
/// # Java Reference
/// MortgageService.java:89-134
pub fn withdraw_reward(
    storage_adapter: &tron_backend_execution::EngineBackedEvmStateStore,
    address: &Address,
) -> Result<i64, String> {
    let address_tron = tron_backend_common::to_tron_address(address);

    // Step 1: Check if delegation is allowed
    if !storage_adapter
        .allow_change_delegation()
        .map_err(|e| format!("Failed to check allow_change_delegation: {}", e))?
    {
        debug!(
            "Delegation not allowed, skipping reward computation for {}",
            address_tron
        );
        return Ok(0);
    }

    // Step 2: Get account - return 0 if account doesn't exist
    let account_exists = storage_adapter
        .get_account(address)
        .map_err(|e| format!("Failed to get account: {}", e))?
        .is_some();

    if !account_exists {
        debug!(
            "Account {} not found, skipping reward computation",
            address_tron
        );
        return Ok(0);
    }

    // Step 3: Get cycle information
    let mut begin_cycle = storage_adapter
        .get_delegation_begin_cycle(address)
        .map_err(|e| format!("Failed to get begin_cycle: {}", e))?;
    let mut end_cycle = storage_adapter
        .get_delegation_end_cycle(address)
        .map_err(|e| format!("Failed to get end_cycle: {}", e))?;
    let current_cycle = storage_adapter
        .get_current_cycle_number()
        .map_err(|e| format!("Failed to get current_cycle: {}", e))?;

    debug!(
        "Delegation cycles for {}: begin={}, end={}, current={}",
        address_tron, begin_cycle, end_cycle, current_cycle
    );

    let mut reward: i64 = 0;

    // Step 4: Check if begin_cycle is in the future
    if begin_cycle > current_cycle {
        debug!(
            "begin_cycle {} > current_cycle {}, no reward",
            begin_cycle, current_cycle
        );
        return Ok(0);
    }

    // Step 5: Handle same-cycle case
    // Java: if (beginCycle == currentCycle) { ... check accountVote ... }
    if begin_cycle == current_cycle {
        let account_vote = storage_adapter
            .get_delegation_account_vote(begin_cycle, address)
            .map_err(|e| format!("Failed to get account_vote: {}", e))?;

        if account_vote.is_some() {
            debug!(
                "Account vote exists for {} at current cycle {}, no reward",
                address_tron, begin_cycle
            );
            return Ok(0);
        }
    }

    // Step 6: Withdraw the latest cycle reward
    // Java: if (beginCycle + 1 == endCycle && beginCycle < currentCycle) { ... }
    if begin_cycle + 1 == end_cycle && begin_cycle < current_cycle {
        if let Some(account_vote) = storage_adapter
            .get_delegation_account_vote(begin_cycle, address)
            .map_err(|e| format!("Failed to get account_vote for latest cycle: {}", e))?
        {
            let latest_reward =
                compute_reward(storage_adapter, begin_cycle, end_cycle, &account_vote)?;

            debug!(
                "Latest cycle reward for {} cycle {}: {} SUN",
                address_tron, begin_cycle, latest_reward
            );

            // Note: In Java, adjustAllowance is called here, but we accumulate and return total
            reward = latest_reward;
        }
        begin_cycle += 1;
    }

    // Step 7: Update end_cycle to current
    end_cycle = current_cycle;

    // Step 8: Get current votes from account
    let votes = storage_adapter
        .get_delegation_votes_from_account(address)
        .map_err(|e| format!("Failed to get votes from account: {}", e))?;

    // Step 9: Handle case with no votes
    // Java: if (CollectionUtils.isEmpty(accountCapsule.getVotesList())) { ... }
    if votes.is_empty() {
        debug!(
            "Account {} has no votes, setting begin_cycle to {}",
            address_tron,
            end_cycle + 1
        );
        storage_adapter
            .set_delegation_begin_cycle(address, end_cycle + 1)
            .map_err(|e| format!("Failed to set begin_cycle: {}", e))?;
        return Ok(reward);
    }

    // Step 10: Compute reward for remaining cycles
    if begin_cycle < end_cycle {
        let account_snapshot = AccountVoteSnapshot::new(*address, votes.clone());
        let remaining_reward =
            compute_reward(storage_adapter, begin_cycle, end_cycle, &account_snapshot)?;

        debug!(
            "Remaining cycles reward for {} ({} to {}): {} SUN",
            address_tron, begin_cycle, end_cycle, remaining_reward
        );

        reward += remaining_reward;
    }

    // Step 11: Update delegation store state
    storage_adapter
        .set_delegation_begin_cycle(address, end_cycle)
        .map_err(|e| format!("Failed to set begin_cycle: {}", e))?;

    storage_adapter
        .set_delegation_end_cycle(address, end_cycle + 1)
        .map_err(|e| format!("Failed to set end_cycle: {}", e))?;

    let account_snapshot = AccountVoteSnapshot::new(*address, votes);
    storage_adapter
        .set_delegation_account_vote(end_cycle, address, &account_snapshot)
        .map_err(|e| format!("Failed to set account_vote: {}", e))?;

    debug!(
        "withdraw_reward completed for {}: total reward {} SUN, new begin_cycle {}, end_cycle {}",
        address_tron,
        reward,
        end_cycle,
        end_cycle + 1
    );

    Ok(reward)
}

/// Compute reward from begin_cycle to end_cycle.
///
/// Handles both old and new reward algorithms based on the
/// NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE dynamic property.
///
/// # Arguments
/// * `storage_adapter` - Storage adapter
/// * `begin_cycle` - Start cycle (inclusive)
/// * `end_cycle` - End cycle (exclusive)
/// * `account` - Account vote snapshot
///
/// # Returns
/// * `Ok(reward)` - Computed reward in SUN
///
/// # Java Reference
/// MortgageService.java:199-230
pub fn compute_reward(
    storage_adapter: &EngineBackedEvmStateStore,
    begin_cycle: i64,
    end_cycle: i64,
    account: &AccountVoteSnapshot,
) -> Result<i64, String> {
    if begin_cycle >= end_cycle {
        return Ok(0);
    }

    let mut reward: i64 = 0;
    let new_algorithm_cycle = storage_adapter
        .get_new_reward_algorithm_effective_cycle()
        .map_err(|e| format!("Failed to get new_algorithm_cycle: {}", e))?;

    debug!(
        "compute_reward: begin={}, end={}, new_algorithm_cycle={}",
        begin_cycle, end_cycle, new_algorithm_cycle
    );

    // Old algorithm for cycles before new_algorithm_cycle
    if begin_cycle < new_algorithm_cycle {
        let old_end = std::cmp::min(end_cycle, new_algorithm_cycle);
        let old_reward = compute_old_reward(storage_adapter, begin_cycle, old_end, &account.votes)?;
        debug!(
            "Old algorithm reward ({} to {}): {} SUN",
            begin_cycle, old_end, old_reward
        );
        reward += old_reward;
    }

    // New algorithm (Vi-based) for cycles after new_algorithm_cycle
    let new_begin = std::cmp::max(begin_cycle, new_algorithm_cycle);
    if new_begin < end_cycle {
        let new_reward = compute_new_reward(storage_adapter, new_begin, end_cycle, &account.votes)?;
        debug!(
            "New algorithm reward ({} to {}): {} SUN",
            new_begin, end_cycle, new_reward
        );
        reward += new_reward;
    }

    Ok(reward)
}

/// Old reward algorithm: iterate through each cycle.
///
/// For each cycle, computes reward as:
///   user_vote / total_vote * total_reward
///
/// # Java Reference
/// MortgageService.java:171-188, 260-269
fn compute_old_reward(
    storage_adapter: &EngineBackedEvmStateStore,
    begin_cycle: i64,
    end_cycle: i64,
    votes: &[DelegationVote],
) -> Result<i64, String> {
    let mut total_reward: i64 = 0;

    for cycle in begin_cycle..end_cycle {
        for vote in votes {
            let witness_addr = &vote.vote_address;

            // Get total reward for this witness in this cycle
            let cycle_reward = storage_adapter
                .get_delegation_reward(cycle, witness_addr)
                .map_err(|e| format!("Failed to get delegation_reward: {}", e))?;

            if cycle_reward <= 0 {
                continue;
            }

            // Get total votes for this witness in this cycle
            let total_vote = storage_adapter
                .get_delegation_witness_vote(cycle, witness_addr)
                .map_err(|e| format!("Failed to get witness_vote: {}", e))?;

            if total_vote == DELEGATION_STORE_REMARK || total_vote == 0 {
                continue;
            }

            // Calculate user's share of the reward
            let user_vote = vote.vote_count;
            let vote_rate = user_vote as f64 / total_vote as f64;
            let user_reward = (vote_rate * cycle_reward as f64) as i64;

            total_reward += user_reward;
        }
    }

    Ok(total_reward)
}

/// New reward algorithm: uses Vi (vote index) for efficient computation.
///
/// For each vote, computes:
///   delta_vi = Vi(end_cycle - 1) - Vi(begin_cycle - 1)
///   reward += delta_vi * user_vote / DECIMAL_OF_VI_REWARD
///
/// # Java Reference
/// MortgageService.java:215-227
fn compute_new_reward(
    storage_adapter: &EngineBackedEvmStateStore,
    begin_cycle: i64,
    end_cycle: i64,
    votes: &[DelegationVote],
) -> Result<i64, String> {
    let mut reward: i64 = 0;

    for vote in votes {
        let witness_addr = &vote.vote_address;

        // Get Vi values at cycle boundaries
        let begin_vi = storage_adapter
            .get_delegation_witness_vi(begin_cycle - 1, witness_addr)
            .map_err(|e| format!("Failed to get begin_vi: {}", e))?;

        let end_vi = storage_adapter
            .get_delegation_witness_vi(end_cycle - 1, witness_addr)
            .map_err(|e| format!("Failed to get end_vi: {}", e))?;

        // Calculate delta_vi
        let delta_vi = &end_vi - &begin_vi;

        if delta_vi <= BigInt::from(0) {
            continue;
        }

        // Calculate user's contribution
        let user_vote = BigInt::from(vote.vote_count);
        let decimal = BigInt::from(DECIMAL_OF_VI_REWARD);
        let contribution = (&delta_vi * &user_vote) / &decimal;

        // Convert to i64 (safe for reasonable values)
        let contribution_i64 = contribution
            .to_string()
            .parse::<i64>()
            .unwrap_or_else(|_| {
                warn!(
                    "BigInt conversion overflow for contribution: {}",
                    contribution
                );
                0
            });

        reward += contribution_i64;
    }

    Ok(reward)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delegation_vote_creation() {
        let addr = Address::from_slice(&[0x12; 20]);
        let vote = DelegationVote::new(addr, 1000);
        assert_eq!(vote.vote_count, 1000);
    }

    #[test]
    fn test_account_vote_snapshot() {
        let owner = Address::from_slice(&[0x01; 20]);
        let witness = Address::from_slice(&[0x02; 20]);

        let snapshot =
            AccountVoteSnapshot::new(owner, vec![DelegationVote::new(witness, 500)]);

        assert!(snapshot.has_votes());
        assert_eq!(snapshot.votes.len(), 1);
    }
}
