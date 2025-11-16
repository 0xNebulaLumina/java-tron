● Based on my investigation, I've identified the root cause of the blackhole balance discrepancy:

  Root Cause Analysis

  The Issue:
  1. Empty Blocks 2132-2139: Java processed these blocks but didn't execute any transactions on Rust
  2. No Blackhole Sync: For block 2140, ResourceSyncService shows includes_blackhole=false, meaning Java didn't sync the blackhole account to Rust before execution
  3. Stale Read: When Rust executed the AccountUpdateContract, it read the blackhole balance from its RocksDB storage and got an OLD value: 9223372046947975808 (which appeared in logs from ~06:13:34,
  several blocks earlier)

  The Mechanism:
  The architecture has a critical synchronization gap:

  Java Side                    Rust Side
  ---------                    ---------
  AccountStore.put()     →     (snapshot chain only)
    ↓
  Snapshot flush/merge   →     writes to underlying DB
    ↓
  IF backend is REMOTE   →     writes directly to Rust RocksDB

  The Problem Flow:
  1. After transaction execution at block 2131, Java receives state changes from Rust
  2. Java writes these to AccountStore.put() → snapshot chain
  3. During blocks 2132-2139 (empty blocks), Java's snapshot mechanism flushes/merges snapshots
  4. The underlying storage backend (StorageBackendDbSource) is in REMOTE mode
  5. Snapshot flush writes to Rust's RocksDB without going through ResourceSyncService
  6. This creates a race condition or timing issue where old snapshot data overwrites current Rust storage
  7. At block 2140, includes_blackhole=false means ResourceSyncService doesn't re-sync the blackhole
  8. Rust reads the stale value from its RocksDB

  Recommended Fix

  The issue is that two different paths write to Rust storage:
  - Path 1: ResourceSyncService.flushResourceDeltas() (explicit pre-execution sync)
  - Path 2: Snapshot flush → StorageBackend → Rust RocksDB (implicit during snapshot management)

  Option 1: Always Sync Blackhole (Quick Fix)
  Modify ResourceSyncService to ALWAYS include the blackhole account in pre-execution sync, even for transactions that don't charge bandwidth fees:

  // In ResourceSyncService or the code that collects dirty accounts
  // Always add blackhole to dirty accounts set
  dirtyAccounts.add(accountStore.getBlackholeAddress());

  Option 2: Disable Remote Writes from Snapshot Flush (Proper Fix)
  Prevent the snapshot/checkpoint mechanism from writing to remote storage. Snapshots should only manage local state; ResourceSyncService should be the ONLY path that writes to Rust:

  1. Modify TronStoreWithRevoking to use a LOCAL-only backend for snapshot chain
  2. Ensure SnapshotRoot.merge() doesn't trigger remote writes
  3. Make ResourceSyncService the single source of truth for Rust storage writes

  Option 3: Comprehensive Pre-Execution Sync
  Before EVERY remote execution call, sync ALL accounts that might be read:
  - Transaction sender/receiver
  - Blackhole account
  - Any accounts touched by bandwidth/energy processors

