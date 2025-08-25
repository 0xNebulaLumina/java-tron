use std::collections::{HashMap, HashSet};
use anyhow::Result;
use revm::primitives::{Account, AccountInfo, Bytecode, B256, U256, Address};
use revm::{Database, DatabaseCommit};
use tron_backend_storage::StorageEngine;

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

/// Multi-database unified storage adapter that routes data to appropriate databases
/// This matches java-tron's database organization while providing a unified interface for EVM execution
pub struct StorageModuleAdapter {
    storage_engine: StorageEngine,
}

impl StorageModuleAdapter {
    pub fn new(storage_engine: StorageEngine) -> Self {
        Self {
            storage_engine,
        }
    }

    /// Get the appropriate database name for account data
    fn account_database(&self) -> &str {
        "account"
    }

    /// Get the appropriate database name for contract code
    fn code_database(&self) -> &str {
        "code"
    }

    /// Get the appropriate database name for contract storage
    fn contract_state_database(&self) -> &str {
        "contract-state"
    }

    /// Get the appropriate database name for contract metadata
    fn contract_database(&self) -> &str {
        "contract"
    }

    /// Convert Address to storage key for accounts (matching java-tron format)
    /// Java-tron stores accounts using 21-byte addresses with 0x41 prefix
    /// REVM uses 20-byte addresses, so we need to add the 0x41 prefix
    fn account_key(&self, address: &Address) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.push(0x41); // Tron address prefix
        key.extend_from_slice(address.as_slice()); // 20-byte address
        key
    }

    /// Convert Address to storage key for code (raw address, matching java-tron)
    fn code_key(&self, address: &Address) -> Vec<u8> {
        address.as_slice().to_vec()
    }

    /// Convert Address and storage key to contract storage key (matching java-tron's Storage.compose format)
    fn contract_storage_key(&self, address: &Address, storage_key: &U256) -> Vec<u8> {
        // Match java-tron's Storage.compose() method:
        // addrHash[0:16] + storageKey[16:32] (32 bytes total)
        let addr_hash = keccak256(address.as_slice());
        let storage_key_bytes = storage_key.to_be_bytes::<32>();

        let mut composed_key = Vec::with_capacity(32);
        composed_key.extend_from_slice(&addr_hash.as_slice()[0..16]); // First 16 bytes of address hash
        composed_key.extend_from_slice(&storage_key_bytes[16..32]);   // Last 16 bytes of storage key
        composed_key
    }

    /// Serialize AccountInfo to bytes in java-tron Account protobuf format
    fn serialize_account(&self, address: &Address, account: &AccountInfo) -> Vec<u8> {
        // Create a protobuf Account message compatible with java-tron
        // The Account protobuf in java-tron has the following structure:
        // message Account {
        //   bytes address = 1;           // field 1, length-delimited
        //   AccountType type = 2;        // field 2, varint (0 = Normal)
        //   int64 balance = 4;           // field 4, varint
        //   int64 create_time = 9;       // field 9, varint
        //   // ... other fields
        // }
        
        let mut data = Vec::new();
        
        // Field 1: address (length-delimited)
        // Include the full 21-byte Tron address with 0x41 prefix
        let tron_address = self.account_key(address); // This adds 0x41 prefix
        data.push(0x0a); // field 1, length-delimited
        self.write_varint(&mut data, tron_address.len() as u64);
        data.extend_from_slice(&tron_address);
        
        // Field 2: type (AccountType.Normal = 0)
        data.push(0x10); // field 2, varint
        data.push(0x00); // value = 0 (Normal)
        
        // Field 4: balance (varint)
        // Convert U256 balance to u64 (TRON uses long for balance)
        // ALWAYS include balance field, even if 0, for Java compatibility
        let balance_u64 = account.balance.to::<u64>();
        data.push(0x20); // field 4, varint
        self.write_varint(&mut data, balance_u64);
        
        // Field 9: create_time (use current timestamp)
        // Use current time in milliseconds
        use std::time::{SystemTime, UNIX_EPOCH};
        let create_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if create_time > 0 {
            data.push(0x48); // field 9, varint
            self.write_varint(&mut data, create_time);
        }
        
        data
    }
    
    /// Write a varint to the output buffer
    fn write_varint(&self, output: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            output.push(((value & 0x7F) | 0x80) as u8);
            value >>= 7;
        }
        output.push(value as u8);
    }

    /// Deserialize AccountInfo from protobuf bytes (java-tron Account message)
    fn deserialize_account(&self, data: &[u8]) -> Result<AccountInfo> {
        // Parse protobuf Account message
        // For now, we'll implement a simple parser for the balance field
        // TODO: Use proper protobuf parsing library for full compatibility

        // This is a simplified parser that extracts the balance field from the protobuf
        // The Account protobuf has balance as field 4 (varint)
        let balance = self.extract_balance_from_protobuf(data)?;

        // For now, use default values for other fields
        // In a full implementation, we'd parse all fields from the protobuf
        Ok(AccountInfo {
            balance: U256::from(balance),
            nonce: 0, // TRON doesn't use nonce, so we can use 0
            code_hash: revm::primitives::B256::ZERO, // TODO: Extract from protobuf if needed
            code: None,
        })
    }

    /// Extract balance field from Account protobuf message
    /// This is a simplified parser for the balance field (field number 4)
    fn extract_balance_from_protobuf(&self, data: &[u8]) -> Result<u64> {
        let mut pos = 0;

        while pos < data.len() {
            if pos >= data.len() {
                break;
            }

            // Read field header (varint)
            let (field_header, new_pos) = self.read_varint(data, pos)?;
            pos = new_pos;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            if field_number == 4 && wire_type == 0 { // balance field (varint)
                let (balance, _) = self.read_varint(data, pos)?;
                return Ok(balance);
            } else {
                // Skip this field
                pos = self.skip_field(data, pos, wire_type)?;
            }
        }

        // If balance field not found, return 0
        Ok(0)
    }

    /// Read a varint from protobuf data
    fn read_varint(&self, data: &[u8], mut pos: usize) -> Result<(u64, usize)> {
        let mut result = 0u64;
        let mut shift = 0;

        while pos < data.len() {
            let byte = data[pos];
            pos += 1;

            result |= ((byte & 0x7F) as u64) << shift;

            if (byte & 0x80) == 0 {
                return Ok((result, pos));
            }

            shift += 7;
            if shift >= 64 {
                return Err(anyhow::anyhow!("Varint too long"));
            }
        }

        Err(anyhow::anyhow!("Unexpected end of data while reading varint"))
    }

    /// Skip a field in protobuf data
    fn skip_field(&self, data: &[u8], pos: usize, wire_type: u64) -> Result<usize> {
        match wire_type {
            0 => { // Varint
                let (_, new_pos) = self.read_varint(data, pos)?;
                Ok(new_pos)
            },
            1 => { // 64-bit
                Ok(pos + 8)
            },
            2 => { // Length-delimited
                let (length, new_pos) = self.read_varint(data, pos)?;
                Ok(new_pos + length as usize)
            },
            5 => { // 32-bit
                Ok(pos + 4)
            },
            _ => Err(anyhow::anyhow!("Unknown wire type: {}", wire_type))
        }
    }
}

impl StorageAdapter for StorageModuleAdapter {
    fn get_account(&self, address: &Address) -> Result<Option<AccountInfo>> {
        let key = self.account_key(address);
        // Convert to Tron format address for debugging consistency with Java logs
        let address_tron = to_tron_address(address);
        tracing::info!("Getting account for address {:?} (tron: {}), key: {}", 
                      address, address_tron, hex::encode(&key));

        match self.storage_engine.get(self.account_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found account data, length: {}, first 32 bytes: {}",
                               data.len(), hex::encode(&data[..std::cmp::min(32, data.len())]));
                match self.deserialize_account(&data) {
                    Ok(account) => {
                        tracing::info!("Successfully deserialized account - balance: {}, nonce: {}",
                                      account.balance, account.nonce);
                        Ok(Some(account))
                    },
                    Err(e) => {
                        tracing::error!("Failed to deserialize account data: {}", e);
                        // Provide default account as fallback
                        let default_balance = revm::primitives::U256::from(0u64);
                        let default_account = AccountInfo {
                            balance: default_balance,
                            nonce: 0,
                            code_hash: revm::primitives::B256::ZERO,
                            code: None,
                        };
                        tracing::warn!("Providing default account due to deserialization error, balance: {}", default_balance);
                        Ok(Some(default_account))
                    }
                }
            },
            None => {
                tracing::info!("No account data found for address {:?} with key {} - account does not exist", address, hex::encode(&key));
                // Return None to indicate account doesn't exist
                // This allows the Database implementation to handle account creation properly
                Ok(None)
            },
        }
    }

    fn get_code(&self, address: &Address) -> Result<Option<Bytecode>> {
        let key = self.code_key(address);
        match self.storage_engine.get(self.code_database(), &key)? {
            Some(data) => Ok(Some(Bytecode::new_raw(data.into()))),
            None => Ok(None),
        }
    }

    fn get_storage(&self, address: &Address, key: &U256) -> Result<U256> {
        let storage_key = self.contract_storage_key(address, key);
        match self.storage_engine.get(self.contract_state_database(), &storage_key)? {
            Some(data) => {
                if data.len() == 32 {
                    Ok(U256::from_be_bytes::<32>(data.try_into().unwrap()))
                } else {
                    Ok(U256::ZERO)
                }
            }
            None => Ok(U256::ZERO),
        }
    }

    fn set_account(&mut self, address: Address, account: AccountInfo) -> Result<()> {
        let key = self.account_key(&address);
        let data = self.serialize_account(&address, &account);
        let address_tron = to_tron_address(&address);
        tracing::info!("Setting account for address {:?} (tron: {}), balance: {}, key: {}, data_len: {}, data_hex: {}", 
                       address, address_tron, account.balance, hex::encode(&key), 
                       data.len(), hex::encode(&data));
        self.storage_engine.put(self.account_database(), &key, &data)?;
        
        // Immediately verify the write by reading it back
        if let Ok(Some(read_data)) = self.storage_engine.get(self.account_database(), &key) {
            if read_data == data {
                tracing::info!("Verified account write for {} - data matches", address_tron);
            } else {
                tracing::error!("Account write verification failed for {} - data mismatch!", address_tron);
            }
        } else {
            tracing::error!("Account write verification failed for {} - could not read back!", address_tron);
        }
        
        Ok(())
    }

    fn set_code(&mut self, address: Address, code: Bytecode) -> Result<()> {
        let key = self.code_key(&address);
        self.storage_engine.put(self.code_database(), &key, &code.bytes())?;
        Ok(())
    }

    fn set_storage(&mut self, address: Address, key: U256, value: U256) -> Result<()> {
        let storage_key = self.contract_storage_key(&address, &key);
        let data = value.to_be_bytes::<32>();
        self.storage_engine.put(self.contract_state_database(), &storage_key, &data)?;
        Ok(())
    }

    fn remove_account(&mut self, address: &Address) -> Result<()> {
        // Remove account data
        let account_key = self.account_key(address);
        self.storage_engine.delete(self.account_database(), &account_key)?;

        // Remove code
        let code_key = self.code_key(address);
        self.storage_engine.delete(self.code_database(), &code_key)?;

        // Note: We don't remove storage slots here as it would require iteration
        // In a real implementation, we might want to track storage slots separately
        // or use a different key scheme that allows prefix deletion

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
        
        // If account doesn't exist, create a default account for Tron compatibility
        // This ensures that REVM can proceed with execution and account creation is tracked
        let final_result = match result {
            Some(account) => Some(account),
            None => {
                let address_tron = to_tron_address(&address);
                tracing::info!("Creating default account for non-existent address {:?} (tron: {})", address, address_tron);
                // Create a default account with zero balance
                let default_account = AccountInfo {
                    balance: revm::primitives::U256::ZERO,
                    nonce: 0,
                    code_hash: revm::primitives::B256::ZERO,
                    code: None,
                };
                
                // **CRITICAL FIX: Pre-register this as a new account in snapshots**
                // This ensures that when REVM commits changes, it will detect this as account creation
                // even if the balance remains zero
                // Mark as "was non-existent" but don't persist yet - let commit() handle it with final balance
                self.account_snapshots.insert(address, None);
                
                // **IMPORTANT: Don't record state change or persist here**
                // The account creation will be tracked in commit() with the final balance
                // This way Java sees the account created with its actual balance, not 0
                tracing::info!("Marked {:?} (tron: {}) as non-existent for tracking, will persist in commit() with final balance", address, address_tron);

                Some(default_account)
            }
        };
        
        self.account_cache.insert(address, final_result.clone());
        Ok(final_result)
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
                new_account_info.code.is_some() ||
                was_nonexistent_in_snapshots
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
                
                self.state_change_records.push(StateChangeRecord::AccountChange {
                    address,
                    old_account: old_account_info.clone(),
                    new_account: Some(new_account_info.clone()),
                });
            }

            // **CRITICAL FIX: Persist account changes to underlying storage**
            if account_changed || force_track_creation {
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
            }

            // **CRITICAL FIX: Persist code changes if present**
            if let Some(code) = &account.info.code {
                if let Err(e) = self.storage.set_code(address, code.clone()) {
                    tracing::error!("Failed to persist code for {:?}: {}", address, e);
                } else {
                    tracing::debug!("Successfully persisted code for {:?}", address);
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

                // **CRITICAL FIX: Persist account deletion to underlying storage**
                if let Err(e) = self.storage.remove_account(&address) {
                    tracing::error!("Failed to remove account {:?}: {}", address, e);
                } else {
                    tracing::debug!("Successfully removed account {:?}", address);
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

                // Only record and persist the change if the value actually changed
                if old_value != new_value {
                    self.state_change_records.push(StateChangeRecord::StorageChange {
                        address,
                        key,
                        old_value,
                        new_value,
                    });

                    // **CRITICAL FIX: Persist storage changes to underlying storage**
                    if let Err(e) = self.storage.set_storage(address, key, new_value) {
                        tracing::error!("Failed to persist storage change for {:?}[{:?}]: {}", address, key, e);
                    } else {
                        tracing::debug!("Successfully persisted storage change for {:?}[{:?}] = {:?}",
                                       address, key, new_value);
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

/// Helper function for keccak256 hashing
pub fn keccak256(data: &[u8]) -> B256 {
    use sha3::{Digest, Keccak256};
    let mut hasher = Keccak256::new();
    hasher.update(data);
    B256::from_slice(&hasher.finalize())
}

/// Convert an EVM address to a proper Tron format address (base58 with checksum)
fn to_tron_address(address: &Address) -> String {
    use sha2::{Digest, Sha256};
    
    // Create 21-byte address with 0x41 prefix
    let mut tron_addr = Vec::with_capacity(21);
    tron_addr.push(0x41);
    tron_addr.extend_from_slice(address.as_slice());
    
    // Calculate double SHA256 for checksum
    let mut hasher1 = Sha256::new();
    hasher1.update(&tron_addr);
    let hash1 = hasher1.finalize();
    
    let mut hasher2 = Sha256::new();
    hasher2.update(&hash1);
    let hash2 = hasher2.finalize();
    
    // Take first 4 bytes as checksum
    let mut addr_with_checksum = tron_addr;
    addr_with_checksum.extend_from_slice(&hash2[..4]);
    
    // Encode with base58
    bs58::encode(&addr_with_checksum).into_string()
}

/// Convert a Tron format address (base58 with checksum) back to EVM address for testing
#[cfg(test)]
fn from_tron_address(tron_address: &str) -> Result<Address, anyhow::Error> {
    use sha2::{Digest, Sha256};
    
    // Decode base58
    let decoded = bs58::decode(tron_address).into_vec()
        .map_err(|e| anyhow::anyhow!("Invalid base58: {}", e))?;
    
    if decoded.len() != 25 {
        return Err(anyhow::anyhow!("Invalid Tron address length: expected 25 bytes, got {}", decoded.len()));
    }
    
    // Split address and checksum
    let (addr_bytes, checksum) = decoded.split_at(21);
    
    // Verify checksum
    let mut hasher1 = Sha256::new();
    hasher1.update(addr_bytes);
    let hash1 = hasher1.finalize();
    
    let mut hasher2 = Sha256::new();
    hasher2.update(&hash1);
    let hash2 = hasher2.finalize();
    
    if &hash2[..4] != checksum {
        return Err(anyhow::anyhow!("Invalid checksum"));
    }
    
    // Check 0x41 prefix
    if addr_bytes[0] != 0x41 {
        return Err(anyhow::anyhow!("Invalid Tron address prefix: expected 0x41, got 0x{:02x}", addr_bytes[0]));
    }
    
    // Return the 20-byte EVM address (without the 0x41 prefix)
    let mut evm_addr = [0u8; 20];
    evm_addr.copy_from_slice(&addr_bytes[1..]);
    Ok(Address::from(evm_addr))
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

    #[test]
    fn test_tron_address_conversion() {
        // Test the specific example provided
        let tron_address = "TB16q6kpSEW2WqvTJ9ua7HAoP9ugQ2HdHZ";
        let expected_evm_hex = "0x0B53CE4AA6F0C2F3C849F11F682702EC99622E2E";
        
        // Convert Tron address to EVM address
        let evm_address = from_tron_address(tron_address).expect("Failed to parse Tron address");
        let actual_evm_hex = format!("0x{}", hex::encode(evm_address.as_slice()).to_uppercase());
        
        assert_eq!(actual_evm_hex, expected_evm_hex, 
                   "EVM address mismatch: expected {}, got {}", expected_evm_hex, actual_evm_hex);
        
        // Convert EVM address back to Tron address
        let converted_tron_address = to_tron_address(&evm_address);
        
        assert_eq!(converted_tron_address, tron_address,
                   "Tron address mismatch: expected {}, got {}", tron_address, converted_tron_address);
    }

    #[test]
    fn test_tron_address_roundtrip() {
        // Test multiple addresses for round-trip conversion
        let test_cases = vec![
            // Add the specific example
            ("TB16q6kpSEW2WqvTJ9ua7HAoP9ugQ2HdHZ", "0x0B53CE4AA6F0C2F3C849F11F682702EC99622E2E"),
        ];
        
        for (tron_addr, evm_hex) in test_cases {
            // Parse expected EVM address
            let expected_evm = Address::from_slice(&hex::decode(&evm_hex[2..]).expect("Invalid hex"));
            
            // Test Tron -> EVM conversion
            let parsed_evm = from_tron_address(tron_addr).expect("Failed to parse Tron address");
            assert_eq!(parsed_evm, expected_evm, "Tron->EVM conversion failed");
            
            // Test EVM -> Tron conversion
            let converted_tron = to_tron_address(&expected_evm);
            assert_eq!(converted_tron, tron_addr, "EVM->Tron conversion failed");
            
            // Test full round-trip
            let roundtrip_evm = from_tron_address(&converted_tron).expect("Round-trip failed");
            assert_eq!(roundtrip_evm, expected_evm, "Round-trip conversion failed");
        }
    }
}