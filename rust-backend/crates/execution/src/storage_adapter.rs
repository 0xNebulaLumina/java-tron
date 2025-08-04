use std::collections::{HashMap, HashSet};
use anyhow::Result;
use revm::primitives::{Account, AccountInfo, Bytecode, B256, U256, Address};
use revm::{Database, DatabaseCommit};

/// Storage adapter trait for different storage backends
pub trait StorageAdapter: Send + Sync {
    /// Get account information
    fn get_account(&self, address: &Address) -> Result<Option<AccountInfo>>;
    
    /// Get account code
    fn get_code(&self, address: &Address) -> Result<Option<Bytecode>>;
    
    /// Get storage value
    fn get_storage(&self, address: &Address, key: &U256) -> Result<U256>;
    
    /// Set account information
    fn set_account(&mut self, address: Address, account: AccountInfo) -> Result<()>;
    
    /// Set account code
    fn set_code(&mut self, address: Address, code: Bytecode) -> Result<()>;
    
    /// Set storage value
    fn set_storage(&mut self, address: Address, key: U256, value: U256) -> Result<()>;
    
    /// Remove account
    fn remove_account(&mut self, address: &Address) -> Result<()>;
}

/// In-memory storage adapter for testing
#[derive(Debug, Clone)]
pub struct InMemoryStorageAdapter {
    accounts: HashMap<Address, AccountInfo>,
    codes: HashMap<Address, Bytecode>,
    storage: HashMap<(Address, U256), U256>,
}

impl InMemoryStorageAdapter {
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            codes: HashMap::new(),
            storage: HashMap::new(),
        }
    }
}

impl StorageAdapter for InMemoryStorageAdapter {
    fn get_account(&self, address: &Address) -> Result<Option<AccountInfo>> {
        Ok(self.accounts.get(address).cloned())
    }
    
    fn get_code(&self, address: &Address) -> Result<Option<Bytecode>> {
        Ok(self.codes.get(address).cloned())
    }
    
    fn get_storage(&self, address: &Address, key: &U256) -> Result<U256> {
        Ok(self.storage.get(&(*address, *key)).copied().unwrap_or(U256::ZERO))
    }
    
    fn set_account(&mut self, address: Address, account: AccountInfo) -> Result<()> {
        self.accounts.insert(address, account);
        Ok(())
    }
    
    fn set_code(&mut self, address: Address, code: Bytecode) -> Result<()> {
        self.codes.insert(address, code);
        Ok(())
    }
    
    fn set_storage(&mut self, address: Address, key: U256, value: U256) -> Result<()> {
        if value == U256::ZERO {
            self.storage.remove(&(address, key));
        } else {
            self.storage.insert((address, key), value);
        }
        Ok(())
    }
    
    fn remove_account(&mut self, address: &Address) -> Result<()> {
        self.accounts.remove(address);
        self.codes.remove(address);
        
        // Remove all storage for this address
        self.storage.retain(|(addr, _), _| addr != address);
        
        Ok(())
    }
}

/// Snapshot hook callback for capturing modified accounts
pub type SnapshotHook = Box<dyn Fn(&HashSet<Address>) + Send + Sync>;

/// Represents different types of state changes with old and new values
#[derive(Debug, Clone)]
pub enum StateChangeRecord {
    /// Storage slot change within a contract
    StorageChange {
        address: Address,
        key: U256,
        old_value: U256,
        new_value: U256,
    },
    /// Account-level change (balance, nonce, code, etc.)
    AccountChange {
        address: Address,
        old_account: Option<AccountInfo>,
        new_account: Option<AccountInfo>,
    },
}

/// Database adapter that implements REVM's Database trait
pub struct StorageAdapterDatabase<S: StorageAdapter> {
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
}

impl<S: StorageAdapter> StorageAdapterDatabase<S> {
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
        }
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
    fn mark_account_modified(&mut self, address: Address) {
        let was_new = self.modified_accounts.insert(address);
        if was_new {
            // Trigger hooks when a new account is modified
            self.trigger_snapshot_hooks();
        }
    }
}

impl<S: StorageAdapter> Database for StorageAdapterDatabase<S> {
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

        // Load from storage
        let result = self.storage.get_account(&address)?;
        self.account_cache.insert(address, result.clone());
        Ok(result)
    }

    fn code_by_hash(&mut self, _code_hash: B256) -> Result<revm::primitives::Bytecode, Self::Error> {
        // For simplicity, return empty bytecode
        // In a real implementation, this would look up code by hash
        Ok(Bytecode::new())
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

impl<S: StorageAdapter> DatabaseCommit for StorageAdapterDatabase<S> {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        for (address, account) in changes {
            // Mark account as modified for shadow verification
            self.mark_account_modified(address);

            // Get old account info for comparison using comprehensive fallback pattern
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
            let account_changed = match &old_account_info {
                Some(old_info) => {
                    old_info.balance != new_account_info.balance ||
                    old_info.nonce != new_account_info.nonce ||
                    old_info.code_hash != new_account_info.code_hash ||
                    old_info.code != new_account_info.code
                },
                None => true, // New account
            };

            // Record account change if there was a change
            if account_changed {
                self.state_change_records.push(StateChangeRecord::AccountChange {
                    address,
                    old_account: old_account_info.clone(),
                    new_account: Some(new_account_info),
                });
            }

            // Update the account snapshots
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
                }

                // Update the storage snapshots
                self.storage_snapshots
                    .entry(address)
                    .or_insert_with(HashMap::new)
                    .insert(key, new_value);
            }
        }
    }
}

/// Helper function for keccak256 hashing
pub fn keccak256(data: &[u8]) -> B256 {
    use sha3::{Digest, Keccak256};
    let mut hasher = Keccak256::new();
    hasher.update(data);
    B256::from_slice(&hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use revm::primitives::{AccountInfo, Bytecode};

    #[test]
    fn test_snapshot_hooks() {
        let storage = InMemoryStorageAdapter::new();
        let mut db = StorageAdapterDatabase::new(storage);

        // Track modified accounts via hook
        let modified_accounts = Arc::new(Mutex::new(Vec::new()));
        let hook_accounts = modified_accounts.clone();

        db.add_snapshot_hook(move |accounts: &HashSet<Address>| {
            let mut hook_accounts = hook_accounts.lock().unwrap();
            hook_accounts.extend(accounts.iter().cloned());
        });

        // Create test account
        let test_address = Address::from([1u8; 20]);
        let account = Account {
            info: AccountInfo {
                balance: U256::from(1000),
                nonce: 1,
                code_hash: B256::ZERO,
                code: Some(Bytecode::new()),
            },
            storage: HashMap::new(),
            status: revm::primitives::AccountStatus::Loaded,
        };

        // Commit changes (this should trigger the hook)
        let mut changes = HashMap::new();
        changes.insert(test_address, account);
        db.commit(changes);

        // Verify hook was called
        let captured_accounts = modified_accounts.lock().unwrap();
        assert!(captured_accounts.contains(&test_address));

        // Verify modified accounts tracking
        assert!(db.get_modified_accounts().contains(&test_address));
    }

    #[test]
    fn test_modified_accounts_tracking() {
        let storage = InMemoryStorageAdapter::new();
        let mut db = StorageAdapterDatabase::new(storage);

        let addr1 = Address::from([1u8; 20]);
        let addr2 = Address::from([2u8; 20]);

        // Initially no modified accounts
        assert_eq!(db.get_modified_accounts().len(), 0);

        // Mark accounts as modified
        db.mark_account_modified(addr1);
        db.mark_account_modified(addr2);

        // Verify tracking
        assert_eq!(db.get_modified_accounts().len(), 2);
        assert!(db.get_modified_accounts().contains(&addr1));
        assert!(db.get_modified_accounts().contains(&addr2));

        // Clear and verify
        db.clear_modified_accounts();
        assert_eq!(db.get_modified_accounts().len(), 0);
    }

    #[test]
    fn test_snapshot_revert_clears_modified_accounts() {
        let storage = InMemoryStorageAdapter::new();
        let mut db = StorageAdapterDatabase::new(storage);

        let test_address = Address::from([1u8; 20]);

        // Create snapshot
        let snapshot_id = db.snapshot();

        // Mark account as modified
        db.mark_account_modified(test_address);
        assert!(db.get_modified_accounts().contains(&test_address));

        // Revert snapshot
        assert!(db.revert(snapshot_id));

        // Verify modified accounts were cleared
        assert_eq!(db.get_modified_accounts().len(), 0);
    }
}