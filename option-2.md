# Option 2: Java Pre-computes Delegation Reward, Passes to Rust

## Overview

Instead of porting the complex `withdrawReward` logic to Rust, have Java compute the delegation reward using existing `MortgageService.queryReward()` and pass it to Rust as part of the execution request. Rust then uses this pre-computed value to calculate the correct total withdrawal amount.

## Current State

### The Problem
- Java's `WithdrawBalanceActuator` calls `mortgageService.withdrawReward()` which:
  1. Computes delegation rewards across cycles
  2. Adds rewards to `account.allowance`
  3. Updates delegation store state (beginCycle, endCycle, accountVote)
- Rust Phase 1 only reads existing `account.allowance`, missing the delegation rewards (85 SUN discrepancy)

### Key Insight
Java has `queryReward()` which computes the delegation reward **without modifying state**:
```java
// MortgageService.java:136-169
public long queryReward(byte[] address) {
    // Computes reward WITHOUT modifying delegation store
    // Returns: delegationReward + account.allowance
}
```

## Implementation Plan

### Phase 1: Extend Protobuf Request

**File: `framework/src/main/proto/backend.proto`**

Add a new field to carry pre-computed delegation reward:

```protobuf
message ExecuteTransactionRequest {
    // ... existing fields ...

    // Pre-computed delegation reward for WithdrawBalanceContract
    // Java computes this via MortgageService.queryReward() before calling Rust
    // Rust adds this to account.allowance for correct total withdrawal
    int64 pre_computed_delegation_reward = 20;
}
```

### Phase 2: Java Computes and Sends Delegation Reward

**File: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`**

Modify the `execute()` method to compute delegation reward for WithdrawBalanceContract:

```java
// In the switch case for WithdrawBalanceContract (around line 438)
case WithdrawBalanceContract:
    toAddress = new byte[0];
    data = new byte[0];
    txKind = TxKind.NON_VM;
    contractType = BackendOuterClass.ContractType.WITHDRAW_BALANCE_CONTRACT;

    // NEW: Pre-compute delegation reward
    long preComputedDelegationReward = 0;
    if (Boolean.parseBoolean(System.getProperty("remote.exec.precompute.delegation.reward", "true"))) {
        try {
            MortgageService mortgageService = context.getStoreFactory()
                .getChainBaseManager().getMortgageService();

            // queryReward returns: delegationReward + account.allowance
            // We need just the delegation portion, so subtract current allowance
            AccountStore accountStore = context.getStoreFactory()
                .getChainBaseManager().getAccountStore();
            AccountCapsule account = accountStore.get(fromAddress);
            long currentAllowance = account != null ? account.getAllowance() : 0;

            long totalQueryReward = mortgageService.queryReward(fromAddress);
            preComputedDelegationReward = totalQueryReward - currentAllowance;

            logger.debug("WithdrawBalanceContract: pre-computed delegation reward = {} " +
                "(queryReward={}, currentAllowance={})",
                preComputedDelegationReward, totalQueryReward, currentAllowance);
        } catch (Exception e) {
            logger.warn("Failed to pre-compute delegation reward, defaulting to 0: {}", e.getMessage());
            preComputedDelegationReward = 0;
        }
    }
    break;
```

Then when building the request:

```java
// When building ExecuteTransactionRequest
ExecuteTransactionRequest.Builder requestBuilder = ExecuteTransactionRequest.newBuilder()
    // ... existing fields ...
    ;

// Add pre-computed delegation reward for WithdrawBalanceContract
if (contractType == BackendOuterClass.ContractType.WITHDRAW_BALANCE_CONTRACT) {
    requestBuilder.setPreComputedDelegationReward(preComputedDelegationReward);
}
```

### Phase 3: Rust Uses Pre-computed Delegation Reward

**File: `rust-backend/crates/core/src/service/contracts/withdraw.rs`**

Modify `execute_withdraw_balance_contract` to use the pre-computed value:

```rust
pub(crate) fn execute_withdraw_balance_contract(
    &self,
    storage_adapter: &mut EngineBackedEvmStateStore,
    transaction: &TronTransaction,
    context: &TronExecutionContext,
    pre_computed_delegation_reward: i64,  // NEW parameter
) -> Result<TronExecutionResult, String> {
    let owner_address = transaction.from;
    let owner_tron = tron_backend_common::to_tron_address(&owner_address);

    info!("Executing WITHDRAW_BALANCE_CONTRACT: owner={}, pre_computed_delegation_reward={}",
          owner_tron, pre_computed_delegation_reward);

    // ... existing validation code (account exists, is witness, cooldown check) ...

    // Step 4: Read base allowance from storage
    let base_allowance = storage_adapter.get_account_allowance(&owner_address)
        .map_err(|e| format!("Failed to read allowance: {}", e))?;

    // NEW: Calculate total allowance including pre-computed delegation reward
    let total_allowance = base_allowance + pre_computed_delegation_reward;

    if total_allowance <= 0 {
        warn!("Account {} has no reward to withdraw (base_allowance={}, delegation_reward={})",
              owner_tron, base_allowance, pre_computed_delegation_reward);
        return Err("witnessAccount does not have any reward".to_string());
    }

    info!("Account {} total allowance: {} (base={} + delegation={})",
          owner_tron, total_allowance, base_allowance, pre_computed_delegation_reward);

    // Step 5: Check for overflow when adding total allowance to balance
    let old_balance_u64: u64 = owner_account.balance.try_into().unwrap_or(u64::MAX);
    let total_allowance_u64 = total_allowance as u64;

    let new_balance_u64 = old_balance_u64.checked_add(total_allowance_u64)
        .ok_or("Balance overflow when adding allowance")?;

    debug!("Balance update: {} + {} = {}", old_balance_u64, total_allowance_u64, new_balance_u64);

    // ... rest of existing code (update account, emit changes) ...

    // Update WithdrawChange to reflect total amount withdrawn
    let withdraw_changes = vec![
        WithdrawChange {
            owner_address,
            amount: total_allowance,  // Use total, not just base
            latest_withdraw_time: now_ms,
        }
    ];

    // ...
}
```

### Phase 4: Update gRPC Handler

**File: `rust-backend/crates/core/src/service/grpc.rs`**

Pass the pre-computed delegation reward through:

```rust
// In handle_execute_transaction or similar
fn handle_execute_transaction(
    &self,
    request: ExecuteTransactionRequest,
) -> Result<ExecuteTransactionResponse, Status> {
    // ... existing parsing ...

    let pre_computed_delegation_reward = request.pre_computed_delegation_reward;

    // When calling execute_withdraw_balance_contract
    if contract_type == ContractType::WithdrawBalanceContract {
        return self.execute_withdraw_balance_contract(
            storage_adapter,
            &transaction,
            &context,
            pre_computed_delegation_reward,  // Pass through
        );
    }

    // ...
}
```

### Phase 5: Java Applies Delegation State Updates

**File: `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`**

Modify `applyWithdrawChanges` to also call `withdrawReward` for state updates:

```java
private void applyWithdrawChanges(ExecutionProgramResult result, TransactionContext context) {
    // ... existing validation code ...

    try {
        ChainBaseManager chainBaseManager = context.getStoreFactory().getChainBaseManager();
        AccountStore accountStore = chainBaseManager.getAccountStore();
        MortgageService mortgageService = chainBaseManager.getMortgageService();

        for (WithdrawChange withdrawChange : result.getWithdrawChanges()) {
            byte[] ownerAddress = withdrawChange.getOwnerAddress();

            // NEW: Call withdrawReward to update delegation store state
            // This handles: setBeginCycle, setEndCycle, setAccountVote
            // The reward was already computed and applied by Rust (via pre-computed value)
            // but we still need to update the delegation store state
            mortgageService.withdrawReward(ownerAddress);

            // Re-read account after withdrawReward (it may have been modified)
            AccountCapsule accountCapsule = accountStore.get(ownerAddress);
            if (accountCapsule == null) {
                logger.warn("Account not found after withdrawReward: {}",
                    ByteArray.toHexString(ownerAddress));
                continue;
            }

            // Set allowance to 0 and update latestWithdrawTime
            // Note: Balance was already updated by Rust's AccountChange
            accountCapsule.setAllowance(0);
            accountCapsule.setLatestWithdrawTime(withdrawChange.getLatestWithdrawTime());

            accountStore.put(accountCapsule.createDbKey(), accountCapsule);

            logger.debug("Applied WithdrawChange: owner={}, amount={}, latestWithdrawTime={}",
                ByteArray.toHexString(ownerAddress),
                withdrawChange.getAmount(),
                withdrawChange.getLatestWithdrawTime());
        }

        logger.info("Successfully applied {} WithdrawChanges for transaction: {}",
            result.getWithdrawChanges().size(), context.getTrxCap().getTransactionId());

    } catch (Exception e) {
        logger.error("Failed to apply WithdrawChanges: {}", e.getMessage(), e);
    }
}
```

### Phase 6: Configuration

**File: Java system properties**

```bash
# Enable pre-computation of delegation reward (default: true)
-Dremote.exec.precompute.delegation.reward=true
```

**File: `rust-backend/crates/common/src/config.rs`**

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteExecutionConfig {
    // ... existing fields ...

    /// Whether to expect pre-computed delegation reward from Java
    /// When true, uses the value from request; when false, falls back to allowance-only
    #[serde(default = "default_true")]
    pub use_precomputed_delegation_reward: bool,
}

fn default_true() -> bool { true }
```

## Files to Modify

| File | Action | Description |
|------|--------|-------------|
| `framework/src/main/proto/backend.proto` | Modify | Add `pre_computed_delegation_reward` field |
| `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java` | Modify | Compute delegation reward, add to request |
| `rust-backend/crates/core/src/service/contracts/withdraw.rs` | Modify | Use pre-computed value |
| `rust-backend/crates/core/src/service/grpc.rs` | Modify | Parse and pass through pre-computed value |
| `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java` | Modify | Call `withdrawReward` for state updates |
| `rust-backend/proto/backend.proto` | Modify | Add field to Rust proto |

## Data Flow Diagram

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              JAVA SIDE                                   │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  1. RemoteExecutionSPI.execute()                                        │
│     │                                                                   │
│     ├─► mortgageService.queryReward(owner)                              │
│     │   Returns: base_allowance + delegation_reward                     │
│     │                                                                   │
│     ├─► accountStore.get(owner).getAllowance()                          │
│     │   Returns: base_allowance                                         │
│     │                                                                   │
│     ├─► delegation_reward = queryReward - base_allowance                │
│     │                                                                   │
│     └─► Build ExecuteTransactionRequest {                               │
│             ...                                                         │
│             pre_computed_delegation_reward: delegation_reward           │
│         }                                                               │
│                                                                         │
└────────────────────────────────┬────────────────────────────────────────┘
                                 │ gRPC
                                 ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                              RUST SIDE                                   │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  2. execute_withdraw_balance_contract()                                 │
│     │                                                                   │
│     ├─► base_allowance = storage.get_account_allowance(owner)           │
│     │                                                                   │
│     ├─► total_allowance = base_allowance + pre_computed_delegation_reward│
│     │                                                                   │
│     ├─► new_balance = old_balance + total_allowance                     │
│     │                                                                   │
│     ├─► storage.set_account(owner, { balance: new_balance, ... })       │
│     │                                                                   │
│     └─► Return ExecutionResult {                                        │
│             account_changes: [...],                                     │
│             withdraw_changes: [{ amount: total_allowance, ... }]        │
│         }                                                               │
│                                                                         │
└────────────────────────────────┬────────────────────────────────────────┘
                                 │ gRPC response
                                 ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                              JAVA SIDE                                   │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  3. RuntimeSpiImpl.applyWithdrawChanges()                               │
│     │                                                                   │
│     ├─► mortgageService.withdrawReward(owner)                           │
│     │   Updates delegation store state:                                 │
│     │   - setBeginCycle(owner, endCycle)                                │
│     │   - setEndCycle(owner, endCycle + 1)                              │
│     │   - setAccountVote(endCycle, owner, account)                      │
│     │   Note: This also adds to allowance, but we'll reset it anyway    │
│     │                                                                   │
│     ├─► accountCapsule.setAllowance(0)                                  │
│     │                                                                   │
│     ├─► accountCapsule.setLatestWithdrawTime(now)                       │
│     │                                                                   │
│     └─► accountStore.put(owner, accountCapsule)                         │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

## Advantages

1. **Minimal Rust Changes**: No need to port complex delegation logic to Rust
2. **Reuses Existing Java Code**: `queryReward()` already exists and is tested
3. **Lower Risk**: Less chance of algorithm mismatch
4. **Faster Implementation**: Estimated 1-2 days vs 5-8 days for Option 1

## Disadvantages

1. **Extra gRPC Field**: Adds data to every WithdrawBalance request
2. **Java Dependency**: Rust depends on Java for correct computation
3. **Double Work**: Java calls `queryReward()` before and `withdrawReward()` after
4. **State Timing**: Delegation state is read before execution but updated after

## Edge Cases to Handle

### Case 1: queryReward Returns 0
```java
if (totalQueryReward == 0) {
    // No delegation reward, just use base allowance
    preComputedDelegationReward = 0;
}
```

### Case 2: Account Doesn't Exist
```java
AccountCapsule account = accountStore.get(fromAddress);
if (account == null) {
    // No account means no allowance either
    preComputedDelegationReward = 0;
}
```

### Case 3: Delegation Not Allowed
```java
// MortgageService.queryReward already handles this:
// if (!dynamicPropertiesStore.allowChangeDelegation()) return 0;
```

### Case 4: Account Has No Votes
```java
// MortgageService.queryReward handles this:
// if (CollectionUtils.isEmpty(accountCapsule.getVotesList()))
//     return reward + accountCapsule.getAllowance();
```

## Testing Strategy

1. **Unit Test**: Verify `queryReward - allowance = delegation_reward` formula
2. **Integration Test**: Compare output with embedded execution
3. **Regression Test**: Full blockchain replay with CSV comparison
4. **Edge Cases**: Test all edge cases listed above

## Complexity Assessment

| Component | Effort | Risk |
|-----------|--------|------|
| Proto changes | Low | Low |
| Java pre-computation | Low | Low |
| Rust integration | Low | Low |
| Java state updates | Medium | Medium |
| Testing | Medium | - |

## Estimated Effort

- **Development**: 1-2 days
- **Testing**: 1 day
- **Total**: 2-3 days

## Rollout Strategy

1. Implement with feature flag (`remote.exec.precompute.delegation.reward=false`)
2. Test in development environment
3. Enable flag, compare with embedded execution
4. Deploy to production with monitoring
5. Make flag default to `true` after confidence period
