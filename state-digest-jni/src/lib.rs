use jni::objects::{JClass, JObject, JByteArray};
use jni::sys::{jbyteArray, jlong, jstring};
use jni::JNIEnv;
use sha3::{Digest, Keccak256};
use std::collections::BTreeMap;

/// Error types for StateDigest operations
#[derive(thiserror::Error, Debug)]
pub enum StateDigestError {
    #[error("Invalid input data: {0}")]
    InvalidInput(String),
    #[error("JNI error: {0}")]
    JniError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// Represents a single account state change
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountState {
    pub address: Vec<u8>,
    pub balance: Vec<u8>,
    pub nonce: u64,
    pub code_hash: Vec<u8>,
    pub storage_slots: BTreeMap<Vec<u8>, Vec<u8>>,
}

/// StateDigest utility for creating deterministic hashes of modified accounts
pub struct StateDigest {
    accounts: BTreeMap<Vec<u8>, AccountState>,
}

impl StateDigest {
    /// Create a new empty StateDigest
    pub fn new() -> Self {
        Self {
            accounts: BTreeMap::new(),
        }
    }

    /// Add or update an account in the state digest
    pub fn add_account(&mut self, account: AccountState) {
        self.accounts.insert(account.address.clone(), account);
    }

    /// Remove an account from the state digest
    pub fn remove_account(&mut self, address: &[u8]) {
        self.accounts.remove(address);
    }

    /// Get the number of accounts in the digest
    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }

    /// Compute the deterministic hash of all modified accounts
    /// Format: Keccak256(address‖balance‖nonce‖codeHash‖sorted(storageSlots))
    pub fn compute_hash(&self) -> Result<Vec<u8>, StateDigestError> {
        let mut hasher = Keccak256::new();

        // Process accounts in sorted order by address for determinism
        for (address, account) in &self.accounts {
            // Hash: address
            hasher.update(address);

            // Hash: balance
            hasher.update(&account.balance);

            // Hash: nonce (as 8-byte big-endian)
            hasher.update(&account.nonce.to_be_bytes());

            // Hash: code hash
            hasher.update(&account.code_hash);

            // Hash: sorted storage slots
            for (key, value) in &account.storage_slots {
                hasher.update(key);
                hasher.update(value);
            }
        }

        Ok(hasher.finalize().to_vec())
    }

    /// Compute hash as hex string
    pub fn compute_hash_hex(&self) -> Result<String, StateDigestError> {
        let hash = self.compute_hash()?;
        Ok(hex::encode(hash))
    }

    /// Serialize the state digest to JSON
    /// Converts byte array keys to hex strings for JSON compatibility
    pub fn to_json(&self) -> Result<String, StateDigestError> {
        // Convert BTreeMap<Vec<u8>, AccountState> to BTreeMap<String, AccountState>
        let string_keyed_accounts: BTreeMap<String, &AccountState> = self.accounts
            .iter()
            .map(|(key, value)| (hex::encode(key), value))
            .collect();

        serde_json::to_string(&string_keyed_accounts)
            .map_err(|e| StateDigestError::SerializationError(e.to_string()))
    }

    /// Deserialize the state digest from JSON
    /// Converts hex string keys back to byte arrays
    pub fn from_json(json: &str) -> Result<Self, StateDigestError> {
        // First deserialize to BTreeMap<String, AccountState>
        let string_keyed_accounts: BTreeMap<String, AccountState> = serde_json::from_str(json)
            .map_err(|e| StateDigestError::SerializationError(e.to_string()))?;

        // Convert back to BTreeMap<Vec<u8>, AccountState>
        let mut accounts = BTreeMap::new();
        for (hex_key, account) in string_keyed_accounts {
            let key = hex::decode(&hex_key)
                .map_err(|e| StateDigestError::SerializationError(format!("Invalid hex key '{}': {}", hex_key, e)))?;
            accounts.insert(key, account);
        }

        Ok(Self { accounts })
    }

    /// Clear all accounts
    pub fn clear(&mut self) {
        self.accounts.clear();
    }
}

impl Default for StateDigest {
    fn default() -> Self {
        Self::new()
    }
}

// JNI exports for Java integration

/// Create a new StateDigest instance
/// Returns a pointer to the StateDigest as jlong
#[no_mangle]
pub extern "system" fn Java_org_tron_core_execution_spi_StateDigestJni_createStateDigest(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    let digest = Box::new(StateDigest::new());
    Box::into_raw(digest) as jlong
}

/// Destroy a StateDigest instance
#[no_mangle]
pub extern "system" fn Java_org_tron_core_execution_spi_StateDigestJni_destroyStateDigest(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle != 0 {
        unsafe {
            let _ = Box::from_raw(handle as *mut StateDigest);
        }
    }
}

/// Add an account to the StateDigest
#[no_mangle]
pub extern "system" fn Java_org_tron_core_execution_spi_StateDigestJni_addAccount(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    address: jbyteArray,
    balance: jbyteArray,
    nonce: jlong,
    code_hash: jbyteArray,
    storage_keys: JObject,
    storage_values: JObject,
) {
    let result = || -> Result<(), StateDigestError> {
        if handle == 0 {
            return Err(StateDigestError::InvalidInput("Invalid handle".to_string()));
        }

        let digest = unsafe { &mut *(handle as *mut StateDigest) };

        // Convert Java byte arrays to Rust Vec<u8>
        let address_obj = unsafe { JObject::from_raw(address) };
        let balance_obj = unsafe { JObject::from_raw(balance) };
        let code_hash_obj = unsafe { JObject::from_raw(code_hash) };

        let address_array = JByteArray::from(address_obj);
        let balance_array = JByteArray::from(balance_obj);
        let code_hash_array = JByteArray::from(code_hash_obj);

        let address_bytes = env
            .convert_byte_array(address_array)
            .map_err(|e| StateDigestError::JniError(e.to_string()))?;
        let balance_bytes = env
            .convert_byte_array(balance_array)
            .map_err(|e| StateDigestError::JniError(e.to_string()))?;
        let code_hash_bytes = env
            .convert_byte_array(code_hash_array)
            .map_err(|e| StateDigestError::JniError(e.to_string()))?;

        // TODO: Convert storage_keys and storage_values from Java arrays
        // For now, use empty storage slots
        let storage_slots = BTreeMap::new();

        let account = AccountState {
            address: address_bytes,
            balance: balance_bytes,
            nonce: nonce as u64,
            code_hash: code_hash_bytes,
            storage_slots,
        };

        digest.add_account(account);
        Ok(())
    };

    if let Err(e) = result() {
        log::error!("Failed to add account: {}", e);
        // TODO: Throw Java exception
    }
}

/// Compute the hash of the StateDigest
#[no_mangle]
pub extern "system" fn Java_org_tron_core_execution_spi_StateDigestJni_computeHash(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jbyteArray {
    let result = || -> Result<jbyteArray, StateDigestError> {
        if handle == 0 {
            return Err(StateDigestError::InvalidInput("Invalid handle".to_string()));
        }

        let digest = unsafe { &*(handle as *const StateDigest) };
        let hash = digest.compute_hash()?;

        let java_array = env
            .byte_array_from_slice(&hash)
            .map_err(|e| StateDigestError::JniError(e.to_string()))?;

        Ok(java_array.into_raw())
    };

    match result() {
        Ok(array) => array,
        Err(e) => {
            log::error!("Failed to compute hash: {}", e);
            // TODO: Throw Java exception
            std::ptr::null_mut()
        }
    }
}

/// Compute the hash of the StateDigest as hex string
#[no_mangle]
pub extern "system" fn Java_org_tron_core_execution_spi_StateDigestJni_computeHashHex(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let result = || -> Result<jstring, StateDigestError> {
        if handle == 0 {
            return Err(StateDigestError::InvalidInput("Invalid handle".to_string()));
        }

        let digest = unsafe { &*(handle as *const StateDigest) };
        let hash_hex = digest.compute_hash_hex()?;

        let java_string = env
            .new_string(&hash_hex)
            .map_err(|e| StateDigestError::JniError(e.to_string()))?;

        Ok(java_string.into_raw())
    };

    match result() {
        Ok(string) => string,
        Err(e) => {
            log::error!("Failed to compute hash hex: {}", e);
            // TODO: Throw Java exception
            std::ptr::null_mut()
        }
    }
}

/// Get the number of accounts in the StateDigest
#[no_mangle]
pub extern "system" fn Java_org_tron_core_execution_spi_StateDigestJni_getAccountCount(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }

    let digest = unsafe { &*(handle as *const StateDigest) };
    digest.account_count() as jlong
}

/// Clear all accounts from the StateDigest
#[no_mangle]
pub extern "system" fn Java_org_tron_core_execution_spi_StateDigestJni_clear(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }

    let digest = unsafe { &mut *(handle as *mut StateDigest) };
    digest.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_digest_creation() {
        let digest = StateDigest::new();
        assert_eq!(digest.account_count(), 0);
    }

    #[test]
    fn test_add_account() {
        let mut digest = StateDigest::new();
        let account = AccountState {
            address: vec![1, 2, 3],
            balance: vec![0, 0, 0, 100],
            nonce: 5,
            code_hash: vec![4, 5, 6],
            storage_slots: BTreeMap::new(),
        };

        digest.add_account(account);
        assert_eq!(digest.account_count(), 1);
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let mut digest1 = StateDigest::new();
        let mut digest2 = StateDigest::new();

        let account = AccountState {
            address: vec![1, 2, 3],
            balance: vec![0, 0, 0, 100],
            nonce: 5,
            code_hash: vec![4, 5, 6],
            storage_slots: BTreeMap::new(),
        };

        digest1.add_account(account.clone());
        digest2.add_account(account);

        let hash1 = digest1.compute_hash().unwrap();
        let hash2 = digest2.compute_hash().unwrap();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_json_serialization() {
        let mut digest = StateDigest::new();
        let account = AccountState {
            address: vec![1, 2, 3],
            balance: vec![0, 0, 0, 100],
            nonce: 5,
            code_hash: vec![4, 5, 6],
            storage_slots: BTreeMap::new(),
        };

        digest.add_account(account);

        let json = digest.to_json().unwrap();

        // Verify JSON contains hex-encoded address key
        assert!(json.contains("\"010203\""), "JSON should contain hex-encoded address key");

        let digest2 = StateDigest::from_json(&json).unwrap();

        assert_eq!(digest.account_count(), digest2.account_count());
        assert_eq!(digest.compute_hash().unwrap(), digest2.compute_hash().unwrap());
    }
}
