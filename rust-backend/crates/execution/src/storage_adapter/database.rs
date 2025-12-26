use std::collections::{HashMap, HashSet};
use revm_primitives::{Address, U256, B256, Bytecode, Account, AccountInfo};
use revm::{Database, DatabaseCommit};
use tron_backend_common::to_tron_address;
use super::{EvmStateStore, StateChangeRecord, utils::keccak256};

/// Snapshot hook callback for capturing modified accounts
pub type SnapshotHook = Box<dyn Fn(&HashSet<Address>) + Send + Sync>;

/// REVM Database wrapper over an EVM state store.
/// Provides caching and state tracking for transaction execution.
///
/// ## Phase 0.3: Write Consistency Model
///
/// The `persist_enabled` flag controls whether this database persists state changes
/// directly to the underlying storage during `commit()`.
///
/// - When `false` (default): Changes are only tracked in memory and returned via
///   `get_state_change_records()`. Java's RuntimeSpiImpl handles actual persistence.
///   This is the recommended mode for avoiding double-writes.
///
/// - When `true`: Changes are persisted directly to storage during `commit()`.
///   Use this only when Java apply is disabled or for specific testing scenarios.
pub struct EvmStateDatabase<S: EvmStateStore> {
    storage: S,
    // Cache for performance
    account_cache: HashMap<Address, Option<AccountInfo>>,
    code_cache: HashMap<Address, Option<Bytecode>>,
    storage_cache: HashMap<(Address, U256), U256>,
    // Track changes for commit
    account_snapshots: HashMap<Address, Option<Account>>,
    storage_snapshots: HashMap<Address, HashMap<U256, U256>>,
    // Track detailed state changes with old and new values
    state_change_records: Vec<StateChangeRecord>,
    // Snapshots for revert support
    snapshots: Vec<(HashMap<Address, Option<Account>>, HashMap<Address, HashMap<U256, U256>>)>,
    // Track modified accounts for shadow verification
    modified_accounts: HashSet<Address>,
    // Snapshot hooks for state comparison
    snapshot_hooks: Vec<SnapshotHook>,
    // Phase 0.3: Whether to persist changes directly to storage during commit
    // Default: false (Rust computes only, Java apply handles persistence)
    persist_enabled: bool,
}

impl<S: EvmStateStore> EvmStateDatabase<S> {
    /// Create a new database with persistence disabled by default.
    /// This follows Phase 0.3 Option A: Rust computes only, Java apply handles persistence.
    pub fn new(storage: S) -> Self {
        Self {
            storage,
            account_cache: HashMap::new(),
            code_cache: HashMap::new(),
            storage_cache: HashMap::new(),
            account_snapshots: HashMap::new(),
            storage_snapshots: HashMap::new(),
            state_change_records: Vec::new(),
            snapshots: Vec::new(),
            modified_accounts: HashSet::new(),
            snapshot_hooks: Vec::new(),
            persist_enabled: false, // Phase 0.3: Default to compute-only mode
        }
    }

    /// Create a new database with explicit persistence control.
    ///
    /// ## Arguments
    /// - `storage`: The underlying storage implementation
    /// - `persist_enabled`: Whether to persist changes directly during commit
    ///   - `false`: Compute only, Java apply handles persistence (recommended)
    ///   - `true`: Persist directly to storage (legacy mode, risk of double-write)
    pub fn new_with_persist(storage: S, persist_enabled: bool) -> Self {
        Self {
            storage,
            account_cache: HashMap::new(),
            code_cache: HashMap::new(),
            storage_cache: HashMap::new(),
            account_snapshots: HashMap::new(),
            storage_snapshots: HashMap::new(),
            state_change_records: Vec::new(),
            snapshots: Vec::new(),
            modified_accounts: HashSet::new(),
            snapshot_hooks: Vec::new(),
            persist_enabled,
        }
    }

    /// Check if persistence is enabled for this database.
    pub fn is_persist_enabled(&self) -> bool {
        self.persist_enabled
    }

    /// Enable or disable persistence.
    ///
    /// Note: Changing this after commits have been made may lead to inconsistent state.
    /// Prefer setting this at construction time via `new_with_persist()`.
    pub fn set_persist_enabled(&mut self, enabled: bool) {
        if enabled != self.persist_enabled {
            tracing::info!(
                "EvmStateDatabase persist_enabled changed from {} to {}",
                self.persist_enabled,
                enabled
            );
        }
        self.persist_enabled = enabled;
    }
    
    pub fn snapshot(&mut self) -> U256 {
        let snapshot_id = U256::from(self.snapshots.len());
        self.snapshots.push((self.account_snapshots.clone(), self.storage_snapshots.clone()));
        snapshot_id
    }
    
    pub fn revert(&mut self, snapshot_id: U256) -> bool {
        let id = snapshot_id.to::<usize>();
        if id < self.snapshots.len() {
            let (accounts, storage) = self.snapshots[id].clone();
            self.account_snapshots = accounts;
            self.storage_snapshots = storage;
            self.snapshots.truncate(id);
            // Clear modified accounts on revert
            self.modified_accounts.clear();
            true
        } else {
            false
        }
    }

    /// Get the current state changes tracked by this database
    pub fn get_storage_snapshots(&self) -> &HashMap<Address, HashMap<U256, U256>> {
        &self.storage_snapshots
    }

    /// Get the current account changes tracked by this database
    pub fn get_account_snapshots(&self) -> &HashMap<Address, Option<Account>> {
        &self.account_snapshots
    }

    /// Get the detailed state change records with old and new values
    pub fn get_state_change_records(&self) -> &Vec<StateChangeRecord> {
        &self.state_change_records
    }

    /// Clear all state change records (useful after processing)
    pub fn clear_state_change_records(&mut self) {
        self.state_change_records.clear();
    }

    /// Add a snapshot hook for capturing modified accounts
    pub fn add_snapshot_hook<F>(&mut self, hook: F)
    where
        F: Fn(&HashSet<Address>) + Send + Sync + 'static
    {
        self.snapshot_hooks.push(Box::new(hook));
    }

    /// Get the current set of modified accounts
    pub fn get_modified_accounts(&self) -> &HashSet<Address> {
        &self.modified_accounts
    }

    /// Clear the modified accounts set
    pub fn clear_modified_accounts(&mut self) {
        self.modified_accounts.clear();
    }

    /// Trigger snapshot hooks with current modified accounts
    pub fn trigger_snapshot_hooks(&self) {
        for hook in &self.snapshot_hooks {
            hook(&self.modified_accounts);
        }
    }

    /// Mark an account as modified and trigger hooks if needed
    pub(crate) fn mark_account_modified(&mut self, address: Address) {
        let was_new = self.modified_accounts.insert(address);
        if was_new {
            // Trigger hooks when a new account is modified
            self.trigger_snapshot_hooks();
        }
    }
}

impl<S: EvmStateStore> Database for EvmStateDatabase<S> {
    type Error = anyhow::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        // Check cache first
        if let Some(cached) = self.account_cache.get(&address) {
            return Ok(cached.clone());
        }

        // Check pending changes
        if let Some(Some(account)) = self.account_snapshots.get(&address) {
            let info = AccountInfo {
                balance: account.info.balance,
                nonce: account.info.nonce,
                code_hash: account.info.code_hash,
                code: account.info.code.clone(),
            };
            self.account_cache.insert(address, Some(info.clone()));
            return Ok(Some(info));
        } else if self.account_snapshots.get(&address) == Some(&None) {
            // Account was deleted
            self.account_cache.insert(address, None);
            return Ok(None);
        }

        // Load from storage.
        //
        // Important: return `None` for accounts that don't exist. Creating a synthetic default
        // account here makes REVM treat the account as existing and can lead to persisting
        // empty accounts that were only touched.
        let result = self.storage.get_account(&address)?;

        // If the account exists, also load contract code (if any) from CodeStore.
        // TRON stores runtime bytecode keyed by address, not by code hash.
        let final_result = match result {
            Some(mut account) => {
                match self.storage.get_code(&address) {
                    Ok(Some(code)) => {
                        account.code_hash = code.hash_slow();
                        account.code = Some(code.clone());
                        self.code_cache.insert(address, Some(code));
                    }
                    Ok(None) => {
                        // Normalize EOAs to the canonical empty code hash.
                        account.code_hash = keccak256(&[]);
                        account.code = None;
                        self.code_cache.insert(address, None);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to load code for {:?}: {}", address, e);
                        if account.code_hash == B256::ZERO {
                            account.code_hash = keccak256(&[]);
                        }
                        account.code = None;
                        self.code_cache.insert(address, None);
                    }
                }
                Some(account)
            }
            None => None,
        };
        
        self.account_cache.insert(address, final_result.clone());
        Ok(final_result)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<revm::primitives::Bytecode, Self::Error> {
        // TRON stores contract code keyed by address in CodeStore, not by code hash.
        // We load code eagerly in `basic()`; this is a fallback.
        let empty_hash = keccak256(&[]);
        if code_hash == empty_hash {
            return Ok(Bytecode::default());
        }

        // Try to find a cached code blob with matching hash.
        for code_opt in self.code_cache.values() {
            if let Some(code) = code_opt {
                if code.hash_slow() == code_hash {
                    return Ok(code.clone());
                }
            }
        }

        // Try to find an address in the account cache with a matching code hash.
        if let Some((address, _)) = self
            .account_cache
            .iter()
            .find(|(_, info)| info.as_ref().map(|i| i.code_hash) == Some(code_hash))
        {
            if let Some(code) = self.storage.get_code(address)? {
                self.code_cache.insert(*address, Some(code.clone()));
                return Ok(code);
            }
        }

        Ok(Bytecode::default())
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let key = (address, index);

        // Check cache first
        if let Some(&cached) = self.storage_cache.get(&key) {
            return Ok(cached);
        }

        // Check pending changes
        if let Some(storage_map) = self.storage_snapshots.get(&address) {
            if let Some(&value) = storage_map.get(&index) {
                self.storage_cache.insert(key, value);
                return Ok(value);
            }
        }

        // Load from storage
        let value = self.storage.get_storage(&address, &index)?;
        self.storage_cache.insert(key, value);
        Ok(value)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        // For simplicity, return a deterministic hash based on block number
        // In a real implementation, this would look up the actual block hash
        let mut hash = [0u8; 32];
        hash[24..32].copy_from_slice(&number.to_be_bytes());
        Ok(B256::from(hash))
    }
}

impl<S: EvmStateStore> DatabaseCommit for EvmStateDatabase<S> {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        for (address, account) in changes {
            // Mark account as modified for shadow verification
            self.mark_account_modified(address);

            // Get old account info for comparison using comprehensive fallback pattern
            let was_nonexistent_in_snapshots = self.account_snapshots.get(&address) == Some(&None);
            let old_account_info = self.account_snapshots.get(&address)
                .and_then(|acc_opt| acc_opt.as_ref())
                .map(|acc| acc.info.clone())
                .or_else(|| {
                    // If not in our changes, try to get from account cache
                    self.account_cache.get(&address).cloned().flatten()
                })
                .or_else(|| {
                    // If not in cache, try to load from storage
                    self.storage.get_account(&address).ok().flatten()
                });

            // Track account-level changes
            let new_account_info = account.info.clone();
            let is_account_creation = old_account_info.is_none() || was_nonexistent_in_snapshots;
            let is_empty_created_account = is_account_creation
                && new_account_info.balance == revm::primitives::U256::ZERO
                && new_account_info.nonce == 0
                && new_account_info.code.is_none()
                && !account.is_selfdestructed()
                && account
                    .storage
                    .iter()
                    .all(|(_, slot)| slot.present_value == revm::primitives::U256::ZERO);

            // TRON parity: do not persist newly-created empty/touched accounts.
            //
            // java-tron does not create an AccountStore entry for a failed CREATE that produced no
            // code/balance, and conformance fixtures assert the address is absent.
            if is_empty_created_account {
                tracing::debug!(
                    "Skipping persistence for empty created account {:?} (tron: {})",
                    address,
                    to_tron_address(&address)
                );
                // Ensure we don't keep an in-memory snapshot that would make the account appear.
                self.account_snapshots.insert(address, None);
                continue;
            }

            let account_changed = match &old_account_info {
                Some(old_info) => {
                    old_info.balance != new_account_info.balance ||
                    old_info.nonce != new_account_info.nonce ||
                    old_info.code_hash != new_account_info.code_hash ||
                    old_info.code != new_account_info.code
                },
                None => true, // New account
            } || was_nonexistent_in_snapshots; // Force account creation tracking
            
            // **ENHANCED FIX: Always track account creation, even with zero balance**
            // This ensures that bandwidth processing in Java can find the account
            let force_track_creation = is_account_creation && (
                new_account_info.balance > revm::primitives::U256::ZERO ||
                new_account_info.nonce > 0 ||
                new_account_info.code.is_some()
            );

            // Record account change if there was a change or if we're forcing account creation tracking
            if account_changed || force_track_creation {
                if is_account_creation || force_track_creation {
                    let address_tron = to_tron_address(&address);
                    tracing::info!("Recording account creation for address {:?} (tron: {}) with balance: {} (forced: {})", 
                                 address, address_tron, new_account_info.balance, force_track_creation);
                } else {
                    tracing::debug!("Recording account modification for address {:?} - old balance: {:?}, new balance: {}", 
                                  address, 
                                  old_account_info.as_ref().map(|info| info.balance),
                                  new_account_info.balance);
                }
                
                // If the account did not exist at the start (tracked in snapshots),
                // do not emit a synthetic zeroed old_account; leave it as None to signal creation.
                let old_account_to_record = if was_nonexistent_in_snapshots {
                    None
                } else {
                    old_account_info.clone()
                };

                self.state_change_records.push(StateChangeRecord::AccountChange {
                    address,
                    old_account: old_account_to_record,
                    new_account: Some(new_account_info.clone()),
                });
            }

            // Phase 0.3: Only persist account changes if persist_enabled is true
            // When false (default), changes are tracked in state_change_records and
            // Java's RuntimeSpiImpl handles persistence via applyStateChangesToLocalDatabase
            if self.persist_enabled && (account_changed || force_track_creation) {
                if let Err(e) = self.storage.set_account(address, new_account_info.clone()) {
                    tracing::error!("Failed to persist account changes for {:?}: {}", address, e);
                } else {
                    let address_tron = to_tron_address(&address);
                    if is_account_creation || force_track_creation {
                        tracing::info!("Successfully persisted account creation for {:?} (tron: {}) - balance: {}",
                                     address, address_tron, new_account_info.balance);
                    } else {
                        tracing::info!("Successfully persisted account changes for {:?} (tron: {}) - balance: {}",
                                       address, address_tron, new_account_info.balance);
                    }
                }
            } else if !self.persist_enabled && (account_changed || force_track_creation) {
                let address_tron = to_tron_address(&address);
                tracing::debug!(
                    "Skipping Rust persistence for {:?} (tron: {}) - persist_enabled=false, Java apply will handle",
                    address, address_tron
                );
            }

            // Phase 0.3: Only persist code changes if persist_enabled is true
            if self.persist_enabled {
                if let Some(code) = &account.info.code {
                    if let Err(e) = self.storage.set_code(address, code.clone()) {
                        tracing::error!("Failed to persist code for {:?}: {}", address, e);
                    } else {
                        tracing::debug!("Successfully persisted code for {:?}", address);
                    }
                }
            }

            // Update the account snapshots (keep existing in-memory tracking)
            self.account_snapshots.insert(address, Some(account.clone()));

            // Handle self-destruct
            if account.is_selfdestructed() {
                // Record account deletion
                if old_account_info.is_some() {
                    self.state_change_records.push(StateChangeRecord::AccountChange {
                        address,
                        old_account: old_account_info,
                        new_account: None, // Account deleted
                    });
                }

                // Phase 0.3: Only persist account deletion if persist_enabled is true
                if self.persist_enabled {
                    if let Err(e) = self.storage.remove_account(&address) {
                        tracing::error!("Failed to remove account {:?}: {}", address, e);
                    } else {
                        tracing::debug!("Successfully removed account {:?}", address);
                    }
                }

                self.account_snapshots.insert(address, None);
            }

            // Clone storage before iterating to avoid borrow checker issues
            let new_values: HashMap<U256, U256> = account.storage.iter()
                .map(|(k, slot)| (*k, slot.present_value))
                .collect();

            // Store storage changes and track detailed state changes
            for (key, new_value) in new_values {
                // Get the old value before updating
                let old_value = self.storage_snapshots
                    .get(&address)
                    .and_then(|storage| storage.get(&key))
                    .copied()
                    .unwrap_or_else(|| {
                        // If not in our changes, try to get from storage cache
                        self.storage_cache.get(&(address, key)).copied()
                            .unwrap_or_else(|| {
                                // If not in cache, try to load from storage
                                self.storage.get_storage(&address, &key).unwrap_or(U256::ZERO)
                            })
                    });

                // Only record the change if the value actually changed
                if old_value != new_value {
                    self.state_change_records.push(StateChangeRecord::StorageChange {
                        address,
                        key,
                        old_value,
                        new_value,
                    });

                    // Phase 0.3: Only persist storage changes if persist_enabled is true
                    if self.persist_enabled {
                        if let Err(e) = self.storage.set_storage(address, key, new_value) {
                            tracing::error!("Failed to persist storage change for {:?}[{:?}]: {}", address, key, e);
                        } else {
                            tracing::debug!("Successfully persisted storage change for {:?}[{:?}] = {:?}",
                                           address, key, new_value);
                        }
                    }
                }

                // Update the storage snapshots (keep existing in-memory tracking)
                self.storage_snapshots
                    .entry(address)
                    .or_insert_with(HashMap::new)
                    .insert(key, new_value);
            }
        }
    }
}
