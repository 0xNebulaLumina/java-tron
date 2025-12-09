# Option 1: Implement Full `withdrawReward` in Rust

## Overview

Port the complete `MortgageService.withdrawReward()` logic from Java to Rust, enabling Rust to compute delegation rewards independently without relying on Java.

## Current State

### Java Implementation (`MortgageService.java:89-134`)
```java
public void withdrawReward(byte[] address) {
    if (!dynamicPropertiesStore.allowChangeDelegation()) {
        return;
    }
    AccountCapsule accountCapsule = accountStore.get(address);
    long beginCycle = delegationStore.getBeginCycle(address);
    long endCycle = delegationStore.getEndCycle(address);
    long currentCycle = dynamicPropertiesStore.getCurrentCycleNumber();
    long reward = 0;

    // ... complex reward computation across cycles ...

    adjustAllowance(address, reward);
    delegationStore.setBeginCycle(address, endCycle);
    delegationStore.setEndCycle(address, endCycle + 1);
    delegationStore.setAccountVote(endCycle, address, accountCapsule);
}
```

### Rust Current State (`withdraw.rs:82-83`)
```rust
// Phase 1: Only reads existing allowance, skips delegation rewards
let allowance = storage_adapter.get_account_allowance(&owner_address)?;
```

## Implementation Plan

### Phase 1: Add DelegationStore Access to Storage Adapter

**File: `rust-backend/crates/execution/src/storage_adapter/engine.rs`**

Add new methods to `EngineBackedEvmStateStore`:

```rust
// --- Delegation Store Access Methods ---

/// Check if delegation changes are allowed (dynamic property)
pub fn allow_change_delegation(&self) -> Result<bool, String>;

/// Get the current cycle number from dynamic properties
pub fn get_current_cycle_number(&self) -> Result<i64, String>;

/// Get the begin cycle for an address from delegation store
pub fn get_delegation_begin_cycle(&self, address: &Address) -> Result<i64, String>;

/// Get the end cycle for an address from delegation store
pub fn get_delegation_end_cycle(&self, address: &Address) -> Result<i64, String>;

/// Get account vote snapshot for a specific cycle
pub fn get_account_vote(&self, cycle: i64, address: &Address) -> Result<Option<AccountVoteSnapshot>, String>;

/// Get total reward for a witness in a cycle
pub fn get_delegation_reward(&self, cycle: i64, witness_address: &Address) -> Result<i64, String>;

/// Get total witness vote count for a cycle
pub fn get_witness_vote(&self, cycle: i64, witness_address: &Address) -> Result<i64, String>;

/// Get witness Vi (vote index) for new reward algorithm
pub fn get_witness_vi(&self, cycle: i64, witness_address: &Address) -> Result<BigInt, String>;

/// Get the cycle number when new reward algorithm takes effect
pub fn get_new_reward_algorithm_effective_cycle(&self) -> Result<i64, String>;

/// Get brokerage rate for a witness in a cycle (percentage 0-100)
pub fn get_brokerage(&self, cycle: i64, witness_address: &Address) -> Result<i32, String>;

// --- Delegation Store Write Methods ---

/// Set the begin cycle for an address
pub fn set_delegation_begin_cycle(&mut self, address: &Address, cycle: i64) -> Result<(), String>;

/// Set the end cycle for an address
pub fn set_delegation_end_cycle(&mut self, address: &Address, cycle: i64) -> Result<(), String>;

/// Set account vote snapshot for a cycle
pub fn set_account_vote(&mut self, cycle: i64, address: &Address, account: &AccountVoteSnapshot) -> Result<(), String>;
```

### Phase 2: Define Data Structures

**File: `rust-backend/crates/execution/src/types.rs`** (or new file)

```rust
/// Vote entry: witness address and vote count
#[derive(Clone, Debug)]
pub struct Vote {
    pub vote_address: Address,
    pub vote_count: i64,
}

/// Account vote snapshot for delegation tracking
#[derive(Clone, Debug)]
pub struct AccountVoteSnapshot {
    pub address: Address,
    pub votes: Vec<Vote>,
    pub allowance: i64,
}

/// Constants for delegation
pub const DELEGATION_STORE_REMARK: i64 = -1;
pub const DECIMAL_OF_VI_REWARD: u128 = 1_000_000_000_000_000_000; // 10^18
```

### Phase 3: Implement Storage Keys

**File: `rust-backend/crates/storage/src/keys.rs`** (or equivalent)

DelegationStore uses composite keys. Need to implement key generation:

```rust
/// Generate key for begin_cycle: "begin_cycle" + address
pub fn delegation_begin_cycle_key(address: &[u8]) -> Vec<u8>;

/// Generate key for end_cycle: "end_cycle" + address
pub fn delegation_end_cycle_key(address: &[u8]) -> Vec<u8>;

/// Generate key for account_vote: cycle (8 bytes BE) + address
pub fn delegation_account_vote_key(cycle: i64, address: &[u8]) -> Vec<u8>;

/// Generate key for reward: cycle (8 bytes BE) + witness_address
pub fn delegation_reward_key(cycle: i64, witness_address: &[u8]) -> Vec<u8>;

/// Generate key for witness_vote: cycle (8 bytes BE) + witness_address
pub fn delegation_witness_vote_key(cycle: i64, witness_address: &[u8]) -> Vec<u8>;

/// Generate key for witness_vi: cycle (8 bytes BE) + witness_address
pub fn delegation_witness_vi_key(cycle: i64, witness_address: &[u8]) -> Vec<u8>;

/// Generate key for brokerage: cycle (8 bytes BE) + witness_address
pub fn delegation_brokerage_key(cycle: i64, witness_address: &[u8]) -> Vec<u8>;
```

### Phase 4: Implement Reward Computation

**File: `rust-backend/crates/core/src/service/contracts/withdraw.rs`**

```rust
impl BackendService {
    /// Compute delegation reward for an address across cycles
    /// Ports MortgageService.withdrawReward() logic
    fn withdraw_reward(
        &self,
        storage_adapter: &mut EngineBackedEvmStateStore,
        address: &Address,
    ) -> Result<i64, String> {
        // Check if delegation is allowed
        if !storage_adapter.allow_change_delegation()? {
            return Ok(0);
        }

        let account = storage_adapter.get_account(address)?
            .ok_or("Account not found")?;

        let mut begin_cycle = storage_adapter.get_delegation_begin_cycle(address)?;
        let mut end_cycle = storage_adapter.get_delegation_end_cycle(address)?;
        let current_cycle = storage_adapter.get_current_cycle_number()?;
        let mut reward: i64 = 0;

        if begin_cycle > current_cycle {
            return Ok(0);
        }

        // Check if in same cycle as begin
        if begin_cycle == current_cycle {
            let account_vote = storage_adapter.get_account_vote(begin_cycle, address)?;
            if account_vote.is_some() {
                return Ok(0);
            }
        }

        // Withdraw latest cycle reward
        if begin_cycle + 1 == end_cycle && begin_cycle < current_cycle {
            if let Some(account_vote) = storage_adapter.get_account_vote(begin_cycle, address)? {
                reward = self.compute_reward(storage_adapter, begin_cycle, end_cycle, &account_vote)?;
                // Note: adjustAllowance will be done at the end
            }
            begin_cycle += 1;
        }

        // Update end_cycle to current
        end_cycle = current_cycle;

        // Check if account has votes
        let votes = self.get_account_votes(storage_adapter, address)?;
        if votes.is_empty() {
            storage_adapter.set_delegation_begin_cycle(address, end_cycle + 1)?;
            return Ok(reward);
        }

        // Compute reward for remaining cycles
        if begin_cycle < end_cycle {
            let account_snapshot = AccountVoteSnapshot {
                address: *address,
                votes: votes.clone(),
                allowance: account.allowance,
            };
            reward += self.compute_reward(storage_adapter, begin_cycle, end_cycle, &account_snapshot)?;
        }

        // Update delegation store state
        storage_adapter.set_delegation_begin_cycle(address, end_cycle)?;
        storage_adapter.set_delegation_end_cycle(address, end_cycle + 1)?;

        let account_snapshot = AccountVoteSnapshot {
            address: *address,
            votes,
            allowance: account.allowance,
        };
        storage_adapter.set_account_vote(end_cycle, address, &account_snapshot)?;

        Ok(reward)
    }

    /// Compute reward from begin_cycle to end_cycle
    /// Handles both old and new reward algorithms
    fn compute_reward(
        &self,
        storage_adapter: &EngineBackedEvmStateStore,
        begin_cycle: i64,
        end_cycle: i64,
        account: &AccountVoteSnapshot,
    ) -> Result<i64, String> {
        if begin_cycle >= end_cycle {
            return Ok(0);
        }

        let mut reward: i64 = 0;
        let new_algorithm_cycle = storage_adapter.get_new_reward_algorithm_effective_cycle()?;

        // Old algorithm for cycles before new_algorithm_cycle
        if begin_cycle < new_algorithm_cycle {
            let old_end = std::cmp::min(end_cycle, new_algorithm_cycle);
            reward += self.compute_old_reward(storage_adapter, begin_cycle, old_end, &account.votes)?;
        }

        // New algorithm (Vi-based) for cycles after new_algorithm_cycle
        let new_begin = std::cmp::max(begin_cycle, new_algorithm_cycle);
        if new_begin < end_cycle {
            reward += self.compute_new_reward(storage_adapter, new_begin, end_cycle, &account.votes)?;
        }

        Ok(reward)
    }

    /// Old reward algorithm: iterate through each cycle
    fn compute_old_reward(
        &self,
        storage_adapter: &EngineBackedEvmStateStore,
        begin_cycle: i64,
        end_cycle: i64,
        votes: &[Vote],
    ) -> Result<i64, String> {
        let mut total_reward: i64 = 0;

        for cycle in begin_cycle..end_cycle {
            for vote in votes {
                let witness_addr = &vote.vote_address;
                let total_reward_for_witness = storage_adapter.get_delegation_reward(cycle, witness_addr)?;

                if total_reward_for_witness <= 0 {
                    continue;
                }

                let total_vote = storage_adapter.get_witness_vote(cycle, witness_addr)?;
                if total_vote == DELEGATION_STORE_REMARK || total_vote == 0 {
                    continue;
                }

                let user_vote = vote.vote_count;
                let vote_rate = user_vote as f64 / total_vote as f64;
                total_reward += (vote_rate * total_reward_for_witness as f64) as i64;
            }
        }

        Ok(total_reward)
    }

    /// New reward algorithm: uses Vi (vote index) for efficient computation
    fn compute_new_reward(
        &self,
        storage_adapter: &EngineBackedEvmStateStore,
        begin_cycle: i64,
        end_cycle: i64,
        votes: &[Vote],
    ) -> Result<i64, String> {
        let mut reward: i64 = 0;

        for vote in votes {
            let witness_addr = &vote.vote_address;

            // Get Vi values at cycle boundaries
            let begin_vi = storage_adapter.get_witness_vi(begin_cycle - 1, witness_addr)?;
            let end_vi = storage_adapter.get_witness_vi(end_cycle - 1, witness_addr)?;

            let delta_vi = end_vi - begin_vi;
            if delta_vi <= BigInt::zero() {
                continue;
            }

            let user_vote = BigInt::from(vote.vote_count);
            let contribution = (delta_vi * user_vote) / BigInt::from(DECIMAL_OF_VI_REWARD);
            reward += contribution.to_i64().unwrap_or(0);
        }

        Ok(reward)
    }
}
```

### Phase 5: Integrate into WithdrawBalance Execution

**File: `rust-backend/crates/core/src/service/contracts/withdraw.rs`**

Modify `execute_withdraw_balance_contract`:

```rust
pub(crate) fn execute_withdraw_balance_contract(
    &self,
    storage_adapter: &mut EngineBackedEvmStateStore,
    transaction: &TronTransaction,
    context: &TronExecutionContext,
) -> Result<TronExecutionResult, String> {
    let owner_address = transaction.from;

    // ... existing validation code ...

    // NEW: Compute and apply delegation reward BEFORE reading allowance
    let delegation_reward = self.withdraw_reward(storage_adapter, &owner_address)?;

    if delegation_reward > 0 {
        // Add delegation reward to account's allowance
        let mut account = storage_adapter.get_account(&owner_address)?
            .ok_or("Account not found after withdraw_reward")?;
        account.allowance += delegation_reward;
        storage_adapter.set_account(&owner_address, account)?;
    }

    // Now read the UPDATED allowance (includes delegation reward)
    let allowance = storage_adapter.get_account_allowance(&owner_address)?;

    // ... rest of existing code ...
}
```

### Phase 6: Add gRPC Storage Access

**File: `rust-backend/proto/storage.proto`**

Add new RPC methods for delegation store access:

```protobuf
service StorageService {
    // ... existing methods ...

    // Delegation store methods
    rpc GetDelegationBeginCycle(GetDelegationCycleRequest) returns (GetDelegationCycleResponse);
    rpc GetDelegationEndCycle(GetDelegationCycleRequest) returns (GetDelegationCycleResponse);
    rpc GetAccountVote(GetAccountVoteRequest) returns (GetAccountVoteResponse);
    rpc GetDelegationReward(GetDelegationDataRequest) returns (GetDelegationDataResponse);
    rpc GetWitnessVote(GetDelegationDataRequest) returns (GetDelegationDataResponse);
    rpc GetWitnessVi(GetDelegationDataRequest) returns (GetWitnessViResponse);
    rpc GetBrokerage(GetDelegationDataRequest) returns (GetDelegationDataResponse);

    rpc SetDelegationBeginCycle(SetDelegationCycleRequest) returns (SetDelegationCycleResponse);
    rpc SetDelegationEndCycle(SetDelegationCycleRequest) returns (SetDelegationCycleResponse);
    rpc SetAccountVote(SetAccountVoteRequest) returns (SetAccountVoteResponse);
}

message GetDelegationCycleRequest {
    bytes address = 1;
}

message GetDelegationCycleResponse {
    int64 cycle = 1;
}

message GetAccountVoteRequest {
    int64 cycle = 1;
    bytes address = 2;
}

message GetAccountVoteResponse {
    bool found = 1;
    bytes account_data = 2; // Serialized account vote snapshot
}

message GetDelegationDataRequest {
    int64 cycle = 1;
    bytes witness_address = 2;
}

message GetDelegationDataResponse {
    int64 value = 1;
}

message GetWitnessViResponse {
    bytes vi_bytes = 1; // BigInt as bytes
}
```

### Phase 7: Configuration

**File: `rust-backend/config.toml`**

```toml
[remote_execution]
# Enable full delegation reward computation (Phase 2)
# When false, falls back to Phase 1 (allowance only)
delegation_reward_enabled = true
```

**File: `rust-backend/crates/common/src/config.rs`**

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteExecutionConfig {
    // ... existing fields ...

    /// Enable delegation reward computation in WithdrawBalance
    #[serde(default)]
    pub delegation_reward_enabled: bool,
}
```

## Files to Modify/Create

| File | Action | Description |
|------|--------|-------------|
| `crates/execution/src/storage_adapter/engine.rs` | Modify | Add 15+ delegation store access methods |
| `crates/execution/src/types.rs` | Create/Modify | Add `Vote`, `AccountVoteSnapshot` structs |
| `crates/storage/src/keys.rs` | Create/Modify | Add delegation key generation functions |
| `crates/core/src/service/contracts/withdraw.rs` | Modify | Add `withdraw_reward`, `compute_reward` methods |
| `proto/storage.proto` | Modify | Add delegation gRPC methods |
| `crates/storage/src/grpc_impl.rs` | Modify | Implement new gRPC handlers |
| `crates/common/src/config.rs` | Modify | Add `delegation_reward_enabled` config |
| `config.toml` | Modify | Add delegation config section |

## Complexity Assessment

| Component | Effort | Risk |
|-----------|--------|------|
| Storage adapter methods | Medium | Low |
| Key generation | Low | Medium (must match Java exactly) |
| Reward computation algorithms | High | High (complex math, edge cases) |
| Vi-based new algorithm | High | High (BigInt precision) |
| Delegation state updates | Medium | Medium |
| gRPC protocol changes | Medium | Low |
| Testing | High | - |

## Testing Strategy

1. **Unit Tests**: Test each reward computation function independently
2. **Integration Tests**: Compare Rust output with Java for same input data
3. **Regression Tests**: Run full blockchain replay comparing embedded vs remote
4. **Edge Cases**:
   - Account with no votes
   - Account in same cycle as begin
   - Transition between old and new reward algorithms
   - Zero rewards, negative values
   - BigInt overflow scenarios

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Algorithm mismatch | High | Extensive comparison testing with Java |
| BigInt precision differences | High | Use same precision constants as Java |
| Key format mismatch | High | Document and test key generation |
| State update ordering | Medium | Follow exact Java ordering |
| Performance regression | Medium | Profile delegation store access patterns |

## Estimated Effort

- **Development**: 3-5 days
- **Testing**: 2-3 days
- **Total**: 5-8 days

## Rollout Strategy

1. Implement behind feature flag (`delegation_reward_enabled = false`)
2. Enable in test environment, compare with embedded execution
3. Fix discrepancies until 100% parity
4. Enable in production with monitoring
5. Remove feature flag after confidence period
