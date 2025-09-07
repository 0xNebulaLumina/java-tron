//! Resource state storage for TRON bandwidth and fee management
//! 
//! Handles reading/writing resource-related data to/from storage mirroring Java's DB schema

use anyhow::Result;
use revm_primitives::{Address, U256};
use sha3::Digest;
use std::collections::HashMap;
use tracing::{debug, warn};

/// Dynamic properties from TRON network configuration
#[derive(Debug, Clone)]
pub struct DynamicProperties {
    /// Free bandwidth limit per account per day (in bytes)
    pub free_net_limit: u64,
    /// Window size for bandwidth accounting (in milliseconds)
    /// Used for both free and staked (net) windows in this implementation.
    pub free_net_window_size: u64,
    /// Price of bandwidth in SUN per byte (maps to Java TRANSACTION_FEE)
    pub bandwidth_price: U256,
    /// Total energy limit
    pub total_energy_limit: u64,
    /// Public net limit (global free bandwidth cap)
    pub public_net_limit: u64,
    /// Public net usage in the current window
    pub public_net_usage: u64,
    /// Public net time (timestamp of last update)
    pub public_net_time: u64,
    /// Whether to burn fees instead of crediting blackhole (ALLOW_BLACKHOLE_OPTIMIZATION)
    pub allow_blackhole_optimization: bool,
    /// Total net limit and weight for calculating per-account net limits (optional, Java-compatible)
    pub total_net_limit: u64,
    pub total_net_weight: u64,
    /// Create-new-account parameters
    pub create_new_account_bandwidth_rate: u64,
    pub create_new_account_fee_in_system_contract: u64,
}

impl Default for DynamicProperties {
    fn default() -> Self {
        Self {
            free_net_limit: 5000,           // 5KB free bandwidth per day
            free_net_window_size: 86400000, // 24 hours in milliseconds  
            bandwidth_price: U256::from(1000), // 1000 SUN per byte (example)
            total_energy_limit: 100_000_000, // 100M energy limit
            public_net_limit: 0,
            public_net_usage: 0,
            public_net_time: 0,
            allow_blackhole_optimization: true,
            total_net_limit: 0,
            total_net_weight: 0,
            create_new_account_bandwidth_rate: 1,
            create_new_account_fee_in_system_contract: 1_000_000, // 1 TRX default
        }
    }
}

/// Resource usage record for an account
#[derive(Debug, Clone)]
pub struct ResourceUsageRecord {
    /// Free bandwidth used within current window
    pub free_net_used: u64,
    /// Timestamp of latest free bandwidth operation
    pub latest_consume_free_time: u64,
    /// Net bandwidth (staked) used within current window
    pub net_used: u64,
    /// Timestamp of latest net (staked) bandwidth operation
    pub latest_consume_time: u64,
    /// Energy used within current window
    pub energy_used: u64,
}

impl Default for ResourceUsageRecord {
    fn default() -> Self {
        Self {
            free_net_used: 0,
            latest_consume_free_time: 0,
            net_used: 0,
            latest_consume_time: 0,
            energy_used: 0,
        }
    }
}

/// Delegated resource information
#[derive(Debug, Clone)]
pub struct DelegatedResource {
    /// Amount of TRX staked for NET bandwidth
    pub frozen_balance_for_bandwidth: U256,
    /// Amount of TRX staked for ENERGY
    pub frozen_balance_for_energy: U256,
    /// Expiration timestamp
    pub expire_time_for_bandwidth: u64,
    /// Expiration timestamp for energy  
    pub expire_time_for_energy: u64,
}

impl Default for DelegatedResource {
    fn default() -> Self {
        Self {
            frozen_balance_for_bandwidth: U256::ZERO,
            frozen_balance_for_energy: U256::ZERO,
            expire_time_for_bandwidth: 0,
            expire_time_for_energy: 0,
        }
    }
}

/// Storage adapter for resource-related data
pub struct ResourceStateStore<S> {
    storage: S,
}

impl<S> ResourceStateStore<S> 
where
    S: crate::storage_adapter::StorageAdapter + 'static,
{
    pub fn new(storage: S) -> Result<Self> {
        Ok(Self { storage })
    }

    /// Load dynamic properties from storage (Java-compatible)
    /// Reads from the java-tron "properties" RocksDB using raw key names
    pub fn load_dynamic_properties(&self) -> Result<DynamicProperties> {
        match self.load_dynamic_properties_from_storage() {
            Ok(props) => {
                debug!("Loaded dynamic properties from 'properties' DB");
                Ok(props)
            }
            Err(e) => {
                warn!("Dynamic properties not found or error ({}), using defaults", e);
                Ok(DynamicProperties::default())
            }
        }
    }

    fn load_dynamic_properties_from_storage(&self) -> Result<DynamicProperties> {
        // Java DB name for dynamic properties
        let db = "properties";

        // Helper to read a Java ByteArray.fromLong encoded value (big-endian i64)
        let read_long = |key: &str| -> Result<Option<u64>> {
            if let Some(bytes) = self.storage.get_aux_kv(db, key.as_bytes())? {
                if bytes.len() == 8 {
                    // BytesCapsule(ByteArray.fromLong) stores big-endian
                    let v = u64::from_be_bytes(bytes[0..8].try_into().unwrap());
                    Ok(Some(v))
                } else if bytes.is_empty() {
                    Ok(Some(0))
                } else {
                    // Try to parse variable-length provided value (defensive)
                    let mut padded = [0u8; 8];
                    let n = bytes.len().min(8);
                    padded[8 - n..8].copy_from_slice(&bytes[bytes.len() - n..]);
                    Ok(Some(u64::from_be_bytes(padded)))
                }
            } else {
                Ok(None)
            }
        };

        // FREE_NET_LIMIT
        let free_net_limit = read_long("FREE_NET_LIMIT")?
            .unwrap_or(DynamicProperties::default().free_net_limit);

        // TRANSACTION_FEE (price per byte in SUN)
        let transaction_fee = read_long("TRANSACTION_FEE")?.unwrap_or(1000);

        // TOTAL_ENERGY_LIMIT (optional; keep default if missing)
        let total_energy_limit = read_long("TOTAL_ENERGY_LIMIT")?
            .unwrap_or(DynamicProperties::default().total_energy_limit);

        // TOTAL_NET_LIMIT / TOTAL_NET_WEIGHT (optional; used for net limit calculations)
        let total_net_limit = read_long("TOTAL_NET_LIMIT")?.unwrap_or(0);
        let total_net_weight = read_long("TOTAL_NET_WEIGHT")?.unwrap_or(0);

        // PUBLIC_NET_* (global free bandwidth caps)
        let public_net_limit = read_long("PUBLIC_NET_LIMIT")?.unwrap_or(0);
        let public_net_usage = read_long("PUBLIC_NET_USAGE")?.unwrap_or(0);
        let public_net_time = read_long("PUBLIC_NET_TIME")?.unwrap_or(0);

        // ALLOW_BLACKHOLE_OPTIMIZATION (1 = burn, 0 = credit)
        let allow_blackhole_optimization = read_long("ALLOW_BLACKHOLE_OPTIMIZATION")?.unwrap_or(1) == 1;

        // CREATE_NEW_ACCOUNT_* parameters
        let create_new_account_bandwidth_rate = read_long("CREATE_NEW_ACCOUNT_BANDWIDTH_RATE")?.unwrap_or(1);
        let create_new_account_fee_in_system_contract = read_long("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT")?.unwrap_or(1_000_000);

        Ok(DynamicProperties {
            free_net_limit,
            free_net_window_size: 86400000, // 24h fixed
            bandwidth_price: U256::from(transaction_fee),
            total_energy_limit,
            total_net_limit,
            total_net_weight,
            public_net_limit,
            public_net_usage,
            public_net_time,
            allow_blackhole_optimization,
            create_new_account_bandwidth_rate,
            create_new_account_fee_in_system_contract,
        })
    }

    /// Load resource usage for an account.
    /// Prefer overlay (aux DB) if present; otherwise read from Java Account protobuf in "account" DB.
    pub fn load_resource_usage(&self, address: &Address) -> Result<ResourceUsageRecord> {
        // Check overlay first
        let overlay_db = "resource-usage";
        let overlay_key = Self::resource_usage_db_key(address);
        if let Some(bytes) = self.storage.get_aux_kv(overlay_db, &overlay_key)? {
            return self.deserialize_resource_usage(&bytes);
        }

        // Fall back to reading Java Account protobuf from "account" DB
        let account_db = "account";
        let acc_key = Self::account_db_key(address);
        if let Some(account_bytes) = self.storage.get_aux_kv(account_db, &acc_key)? {
            let usage = Self::extract_resource_usage_from_account_proto(&account_bytes)?;
            return Ok(usage);
        }

        // Default if account not found
        Ok(ResourceUsageRecord::default())
    }

    /// Save resource usage overlay for an account in a dedicated auxiliary DB.
    /// We intentionally do not mutate Java's Account protobuf here; Java will update its own counters.
    pub fn save_resource_usage(&mut self, address: &Address, usage: &ResourceUsageRecord) -> Result<()> {
        let db = "resource-usage";
        let key = Self::resource_usage_db_key(address);
        let bytes = self.serialize_resource_usage(usage)?;
        self.storage.put_aux_kv(db, &key, &bytes)?;
        Ok(())
    }

    fn resource_usage_db_key(address: &Address) -> Vec<u8> {
        format!("account_net_usage:{}", hex::encode(address.as_slice())).into_bytes()
    }

    /// Compose java-tron account DB key: 0x41 prefix + 20-byte address
    fn account_db_key(address: &Address) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.push(0x41);
        key.extend_from_slice(address.as_slice());
        key
    }

    /// Save public net usage and time back to the Java properties DB
    pub fn save_public_net_usage_time(&mut self, usage: u64, time: u64) -> Result<()> {
        let db = "properties";
        self.storage.put_aux_kv(db, b"PUBLIC_NET_USAGE", &usage.to_be_bytes())?;
        self.storage.put_aux_kv(db, b"PUBLIC_NET_TIME", &time.to_be_bytes())?;
        Ok(())
    }

    /// Check whether an account exists in storage (Java-compatible account DB)
    pub fn account_exists(&self, address: &Address) -> Result<bool> {
        Ok(self.storage.get_account(address)?.is_some())
    }

    /// Extract resource usage counters from a java-tron Account protobuf
    /// Fields of interest:
    /// - net_usage (field 8, varint)
    /// - free_net_usage (field 19, varint)
    /// - latest_consume_time (field 21, varint)
    /// - latest_consume_free_time (field 22, varint)
    fn extract_resource_usage_from_account_proto(bytes: &[u8]) -> Result<ResourceUsageRecord> {
        let mut pos = 0usize;
        let mut net_usage: u64 = 0;
        let mut free_net_usage: u64 = 0;
        let mut latest_consume_time: u64 = 0;
        let mut latest_consume_free_time: u64 = 0;

        while pos < bytes.len() {
            let (field_key, new_pos) = Self::read_varint(bytes, pos)?;
            pos = new_pos;
            let field_number = field_key >> 3;
            let wire_type = field_key & 0x7;

            match (field_number, wire_type) {
                (8, 0) => {
                    let (v, np) = Self::read_varint(bytes, pos)?;
                    net_usage = v; pos = np;
                }
                (19, 0) => {
                    let (v, np) = Self::read_varint(bytes, pos)?;
                    free_net_usage = v; pos = np;
                }
                (21, 0) => {
                    let (v, np) = Self::read_varint(bytes, pos)?;
                    latest_consume_time = v; pos = np;
                }
                (22, 0) => {
                    let (v, np) = Self::read_varint(bytes, pos)?;
                    latest_consume_free_time = v; pos = np;
                }
                (_, 0) => {
                    // Other varint fields: skip
                    let (_v, np) = Self::read_varint(bytes, pos)?;
                    pos = np;
                }
                (_, 1) => { pos += 8; }          // 64-bit
                (_, 5) => { pos += 4; }          // 32-bit
                (_, 2) => {                       // length-delimited
                    let (len, np) = Self::read_varint(bytes, pos)?;
                    pos = np + len as usize;
                }
                _ => break,
            }
        }

        Ok(ResourceUsageRecord {
            free_net_used: free_net_usage,
            latest_consume_free_time,
            net_used: net_usage,
            latest_consume_time,
            energy_used: 0,
        })
    }

    /// Decode varint at position, return (value, next_pos)
    fn read_varint(data: &[u8], mut pos: usize) -> Result<(u64, usize)> {
        let mut result: u64 = 0;
        let mut shift = 0u32;
        while pos < data.len() {
            let byte = data[pos];
            pos += 1;
            result |= ((byte & 0x7F) as u64) << shift;
            if (byte & 0x80) == 0 { return Ok((result, pos)); }
            shift += 7;
            if shift >= 64 { return Err(anyhow::anyhow!("Varint too long")); }
        }
        Err(anyhow::anyhow!("Unexpected EOF while reading varint"))
    }

    fn deserialize_resource_usage(&self, bytes: &[u8]) -> Result<ResourceUsageRecord> {
        // New serialization format: [free_net_used(8)][latest_consume_free_time(8)][net_used(8)][latest_consume_time(8)][energy_used(8)]
        // Backward-compatible with old 32-byte format.
        match bytes.len() {
            40..=usize::MAX => {
                let free_net_used = u64::from_be_bytes(bytes[0..8].try_into()?);
                let latest_consume_free_time = u64::from_be_bytes(bytes[8..16].try_into()?);
                let net_used = u64::from_be_bytes(bytes[16..24].try_into()?);
                let latest_consume_time = u64::from_be_bytes(bytes[24..32].try_into()?);
                let energy_used = u64::from_be_bytes(bytes[32..40].try_into()?);
                Ok(ResourceUsageRecord {
                    free_net_used,
                    latest_consume_free_time,
                    net_used,
                    latest_consume_time,
                    energy_used,
                })
            }
            32 => {
                // Old format: [free_net_used][latest_op_time][net_used][energy_used]
                let free_net_used = u64::from_be_bytes(bytes[0..8].try_into()?);
                let latest_op_time = u64::from_be_bytes(bytes[8..16].try_into()?);
                let net_used = u64::from_be_bytes(bytes[16..24].try_into()?);
                let energy_used = u64::from_be_bytes(bytes[24..32].try_into()?);
                Ok(ResourceUsageRecord {
                    free_net_used,
                    latest_consume_free_time: latest_op_time,
                    net_used,
                    latest_consume_time: latest_op_time,
                    energy_used,
                })
            }
            _ => Err(anyhow::anyhow!("Invalid resource usage data length")),
        }
    }

    fn serialize_resource_usage(&self, usage: &ResourceUsageRecord) -> Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(40);
        bytes.extend_from_slice(&usage.free_net_used.to_be_bytes());
        bytes.extend_from_slice(&usage.latest_consume_free_time.to_be_bytes());
        bytes.extend_from_slice(&usage.net_used.to_be_bytes());
        bytes.extend_from_slice(&usage.latest_consume_time.to_be_bytes());
        bytes.extend_from_slice(&usage.energy_used.to_be_bytes());
        Ok(bytes)
    }

    fn deserialize_resource_usage_from_u256(&self, value: U256) -> Result<ResourceUsageRecord> {
        // Unpack 4 u64 values from U256 (legacy compatibility): treat single timestamp as both
        let bytes = value.to_be_bytes::<32>();
        let free_net_used = u64::from_be_bytes(bytes[0..8].try_into()?);
        let latest_op_time = u64::from_be_bytes(bytes[8..16].try_into()?);
        let net_used = u64::from_be_bytes(bytes[16..24].try_into()?);
        let energy_used = u64::from_be_bytes(bytes[24..32].try_into()?);
        Ok(ResourceUsageRecord {
            free_net_used,
            latest_consume_free_time: latest_op_time,
            net_used,
            latest_consume_time: latest_op_time,
            energy_used,
        })
    }

    fn serialize_resource_usage_as_u256(&self, usage: &ResourceUsageRecord) -> Result<U256> {
        // Legacy-compatible packing: drop one timestamp (use net timestamp)
        let mut bytes = [0u8; 32];
        bytes[0..8].copy_from_slice(&usage.free_net_used.to_be_bytes());
        bytes[8..16].copy_from_slice(&usage.latest_consume_time.to_be_bytes());
        bytes[16..24].copy_from_slice(&usage.net_used.to_be_bytes());
        bytes[24..32].copy_from_slice(&usage.energy_used.to_be_bytes());
        Ok(U256::from_be_slice(&bytes))
    }

    /// Load account balance
    pub fn load_account_balance(&self, address: &Address) -> Result<U256> {
        if let Some(account_info) = self.storage.get_account(address)? {
            Ok(account_info.balance)
        } else {
            // Return zero balance if account doesn't exist
            Ok(U256::ZERO)
        }
    }

    /// Load delegated resources for an account
    pub fn load_delegated_resources(&self, address: &Address) -> Result<DelegatedResource> {
        let delegated_key_hash = sha3::Keccak256::digest(format!("delegated_resource:{}", hex::encode(address.as_slice())));
        let delegated_key = U256::from_be_slice(&delegated_key_hash);
        
        let delegated_value = self.storage.get_storage(address, &delegated_key)?;
        if delegated_value != U256::ZERO {
            // For simplicity, we'll store minimal delegated resource data in a single U256
            // In a full implementation, this would span multiple storage slots
            return Ok(DelegatedResource {
                frozen_balance_for_bandwidth: delegated_value,
                frozen_balance_for_energy: U256::ZERO,
                expire_time_for_bandwidth: 0,
                expire_time_for_energy: 0,
            });
        }

        Ok(DelegatedResource::default())
    }

    fn deserialize_delegated_resource(&self, bytes: &[u8]) -> Result<DelegatedResource> {
        // Format: [frozen_bandwidth(32)][frozen_energy(32)][expire_bandwidth(8)][expire_energy(8)]
        if bytes.len() < 80 {
            return Err(anyhow::anyhow!("Invalid delegated resource data length"));
        }

        let frozen_balance_for_bandwidth = U256::from_be_slice(&bytes[0..32]);
        let frozen_balance_for_energy = U256::from_be_slice(&bytes[32..64]);
        let expire_time_for_bandwidth = u64::from_be_bytes(bytes[64..72].try_into()?);
        let expire_time_for_energy = u64::from_be_bytes(bytes[72..80].try_into()?);

        Ok(DelegatedResource {
            frozen_balance_for_bandwidth,
            frozen_balance_for_energy,
            expire_time_for_bandwidth,
            expire_time_for_energy,
        })
    }

    /// Load incoming delegations (from DelegatedResourceAccountIndex)
    pub fn load_incoming_delegations(&self, address: &Address) -> Result<HashMap<Address, DelegatedResource>> {
        // This would load delegations made TO this address
        // Key format: delegated_resource_account_index:{to_address}:{from_address}
        let mut delegations = HashMap::new();
        
        // For now, return empty - this would need to iterate through the index
        // In a full implementation, we'd scan the delegated resource index
        debug!("Loading incoming delegations for address {:?} (placeholder)", address);
        
        Ok(delegations)
    }

    /// Load outgoing delegations (from DelegatedResourceAccountIndex) 
    pub fn load_outgoing_delegations(&self, address: &Address) -> Result<HashMap<Address, DelegatedResource>> {
        // This would load delegations made BY this address
        let mut delegations = HashMap::new();
        
        debug!("Loading outgoing delegations for address {:?} (placeholder)", address);
        
        Ok(delegations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_adapter::InMemoryStorageAdapter;

    fn create_test_store() -> ResourceStateStore<InMemoryStorageAdapter> {
        let storage = InMemoryStorageAdapter::new();
        ResourceStateStore::new(storage).unwrap()
    }

    #[test]
    fn test_resource_usage_serialization() {
        let store = create_test_store();
        let usage = ResourceUsageRecord {
            free_net_used: 1000,
            latest_consume_free_time: 1234567890,
            net_used: 2000,
            latest_consume_time: 1234567891,
            energy_used: 3000,
        };

        let serialized = store.serialize_resource_usage(&usage).unwrap();
        let deserialized = store.deserialize_resource_usage(&serialized).unwrap();

        assert_eq!(deserialized.free_net_used, usage.free_net_used);
        assert_eq!(deserialized.latest_consume_free_time, usage.latest_consume_free_time);
        assert_eq!(deserialized.latest_consume_time, usage.latest_consume_time);
        assert_eq!(deserialized.net_used, usage.net_used);
        assert_eq!(deserialized.energy_used, usage.energy_used);
    }

    #[test]
    fn test_default_dynamic_properties() {
        let props = DynamicProperties::default();
        assert_eq!(props.free_net_limit, 5000);
        assert_eq!(props.free_net_window_size, 86400000);
        assert!(props.bandwidth_price > U256::ZERO);
    }

    #[test]
    fn test_load_nonexistent_account() {
        let store = create_test_store();
        let address = Address::from_slice(&[0x42; 20]);
        
        let balance = store.load_account_balance(&address).unwrap();
        assert_eq!(balance, U256::ZERO);

        let usage = store.load_resource_usage(&address).unwrap();
        assert_eq!(usage.free_net_used, 0);
        assert_eq!(usage.latest_consume_free_time, 0);
        assert_eq!(usage.latest_consume_time, 0);
    }
}
