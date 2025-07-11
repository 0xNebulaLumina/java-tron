use std::collections::HashMap;
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

/// Database adapter that implements REVM's Database trait
pub struct StorageAdapterDatabase<S: StorageAdapter> {
    storage: S,
    // Cache for performance
    account_cache: HashMap<Address, Option<AccountInfo>>,
    code_cache: HashMap<Address, Option<Bytecode>>,
    storage_cache: HashMap<(Address, U256), U256>,
    // Track changes for commit
    accounts: HashMap<Address, Option<Account>>,
    storage_changes: HashMap<Address, HashMap<U256, U256>>,
    // Snapshots for revert support
    snapshots: Vec<(HashMap<Address, Option<Account>>, HashMap<Address, HashMap<U256, U256>>)>,
}

impl<S: StorageAdapter> StorageAdapterDatabase<S> {
    pub fn new(storage: S) -> Self {
        Self {
            storage,
            account_cache: HashMap::new(),
            code_cache: HashMap::new(),
            storage_cache: HashMap::new(),
            accounts: HashMap::new(),
            storage_changes: HashMap::new(),
            snapshots: Vec::new(),
        }
    }
    
    pub fn snapshot(&mut self) -> U256 {
        let snapshot_id = U256::from(self.snapshots.len());
        self.snapshots.push((self.accounts.clone(), self.storage_changes.clone()));
        snapshot_id
    }
    
    pub fn revert(&mut self, snapshot_id: U256) -> bool {
        let id = snapshot_id.to::<usize>();
        if id < self.snapshots.len() {
            let (accounts, storage) = self.snapshots[id].clone();
            self.accounts = accounts;
            self.storage_changes = storage;
            self.snapshots.truncate(id);
            true
        } else {
            false
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
        if let Some(Some(account)) = self.accounts.get(&address) {
            let info = AccountInfo {
                balance: account.info.balance,
                nonce: account.info.nonce,
                code_hash: account.info.code_hash,
                code: account.info.code.clone(),
            };
            self.account_cache.insert(address, Some(info.clone()));
            return Ok(Some(info));
        } else if self.accounts.get(&address) == Some(&None) {
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
        if let Some(storage_map) = self.storage_changes.get(&address) {
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
            // Clone storage before iterating to avoid borrow checker issues
            let storage_changes: HashMap<U256, U256> = account.storage.iter()
                .map(|(k, slot)| (*k, slot.present_value))
                .collect();
            
            // Store account changes
            self.accounts.insert(address, Some(account.clone()));
            
            // Store storage changes
            for (key, value) in storage_changes {
                self.storage_changes
                    .entry(address)
                    .or_insert_with(HashMap::new)
                    .insert(key, value);
            }
            
            // Handle self-destruct
            if account.is_selfdestructed() {
                self.accounts.insert(address, None);
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