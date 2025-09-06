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
    /// Window size for free bandwidth (in milliseconds) 
    pub free_net_window_size: u64,
    /// Price of bandwidth in SUN per byte
    pub bandwidth_price: U256,
    /// Total energy limit
    pub total_energy_limit: u64,
    // Other dynamic properties can be added as needed
}

impl Default for DynamicProperties {
    fn default() -> Self {
        Self {
            free_net_limit: 5000,           // 5KB free bandwidth per day
            free_net_window_size: 86400000, // 24 hours in milliseconds  
            bandwidth_price: U256::from(1000), // 1000 SUN per byte (example)
            total_energy_limit: 100_000_000, // 100M energy limit
        }
    }
}

/// Resource usage record for an account
#[derive(Debug, Clone)]
pub struct ResourceUsageRecord {
    /// Free bandwidth used within current window
    pub free_net_used: u64,
    /// Timestamp of latest operation (for window calculation)
    pub latest_op_time: u64,
    /// Net bandwidth from staking (for this calculation window)
    pub net_used: u64,
    /// Energy used within current window
    pub energy_used: u64,
}

impl Default for ResourceUsageRecord {
    fn default() -> Self {
        Self {
            free_net_used: 0,
            latest_op_time: 0,
            net_used: 0,
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

    /// Load dynamic properties from storage
    /// Maps to Java's dynamic properties DB
    pub fn load_dynamic_properties(&self) -> Result<DynamicProperties> {
        // Try to load from storage first
        if let Ok(props) = self.load_dynamic_properties_from_storage() {
            debug!("Loaded dynamic properties from storage");
            return Ok(props);
        }

        // Fallback to defaults if not found
        warn!("Dynamic properties not found in storage, using defaults");
        Ok(DynamicProperties::default())
    }

    fn load_dynamic_properties_from_storage(&self) -> Result<DynamicProperties> {
        // Key format matches Java's properties DB
        // These keys are examples - actual keys need to match Java implementation
        let free_net_limit = self.load_property_u64("FREE_NET_LIMIT")?
            .unwrap_or(DynamicProperties::default().free_net_limit);
        
        let bandwidth_price = self.load_property_u256("BANDWIDTH_PRICE")?
            .unwrap_or(DynamicProperties::default().bandwidth_price);

        let total_energy_limit = self.load_property_u64("TOTAL_ENERGY_LIMIT")?
            .unwrap_or(DynamicProperties::default().total_energy_limit);

        Ok(DynamicProperties {
            free_net_limit,
            free_net_window_size: 86400000, // 24h fixed
            bandwidth_price,
            total_energy_limit,
        })
    }

    fn load_property_u64(&self, key: &str) -> Result<Option<u64>> {
        // Use a special system address for properties
        let system_addr = Address::ZERO;
        let prop_key_hash = sha3::Keccak256::digest(format!("properties:{}", key));
        let prop_key = U256::from_be_slice(&prop_key_hash);
        
        let value = self.storage.get_storage(&system_addr, &prop_key)?;
        if value != U256::ZERO {
            // Extract u64 from the lower 64 bits
            Ok(Some(value.as_limbs()[0]))
        } else {
            Ok(None)
        }
    }

    fn load_property_u256(&self, key: &str) -> Result<Option<U256>> {
        // Use a special system address for properties
        let system_addr = Address::ZERO;
        let prop_key_hash = sha3::Keccak256::digest(format!("properties:{}", key));
        let prop_key = U256::from_be_slice(&prop_key_hash);
        
        let value = self.storage.get_storage(&system_addr, &prop_key)?;
        if value != U256::ZERO {
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    /// Load resource usage for an account
    /// Uses a dedicated auxiliary DB and avoids emitting EVM storage changes
    pub fn load_resource_usage(&self, address: &Address) -> Result<ResourceUsageRecord> {
        let db = "resource-usage";
        let key = Self::resource_usage_db_key(address);

        if let Some(bytes) = self.storage.get_aux_kv(db, &key)? {
            return self.deserialize_resource_usage(&bytes);
        }

        // Return default if not found
        Ok(ResourceUsageRecord::default())
    }

    /// Save resource usage for an account in a dedicated auxiliary DB
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

    fn deserialize_resource_usage(&self, bytes: &[u8]) -> Result<ResourceUsageRecord> {
        // Simple serialization format: [free_net_used(8)][latest_op_time(8)][net_used(8)][energy_used(8)]
        if bytes.len() < 32 {
            return Err(anyhow::anyhow!("Invalid resource usage data length"));
        }

        let free_net_used = u64::from_be_bytes(bytes[0..8].try_into()?);
        let latest_op_time = u64::from_be_bytes(bytes[8..16].try_into()?);
        let net_used = u64::from_be_bytes(bytes[16..24].try_into()?);
        let energy_used = u64::from_be_bytes(bytes[24..32].try_into()?);

        Ok(ResourceUsageRecord {
            free_net_used,
            latest_op_time,
            net_used,
            energy_used,
        })
    }

    fn serialize_resource_usage(&self, usage: &ResourceUsageRecord) -> Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(32);
        bytes.extend_from_slice(&usage.free_net_used.to_be_bytes());
        bytes.extend_from_slice(&usage.latest_op_time.to_be_bytes());
        bytes.extend_from_slice(&usage.net_used.to_be_bytes());
        bytes.extend_from_slice(&usage.energy_used.to_be_bytes());
        Ok(bytes)
    }

    fn deserialize_resource_usage_from_u256(&self, value: U256) -> Result<ResourceUsageRecord> {
        // Unpack 4 u64 values from U256
        let bytes = value.to_be_bytes::<32>();
        
        let free_net_used = u64::from_be_bytes(bytes[0..8].try_into()?);
        let latest_op_time = u64::from_be_bytes(bytes[8..16].try_into()?);
        let net_used = u64::from_be_bytes(bytes[16..24].try_into()?);
        let energy_used = u64::from_be_bytes(bytes[24..32].try_into()?);

        Ok(ResourceUsageRecord {
            free_net_used,
            latest_op_time,
            net_used,
            energy_used,
        })
    }

    fn serialize_resource_usage_as_u256(&self, usage: &ResourceUsageRecord) -> Result<U256> {
        let mut bytes = [0u8; 32];
        bytes[0..8].copy_from_slice(&usage.free_net_used.to_be_bytes());
        bytes[8..16].copy_from_slice(&usage.latest_op_time.to_be_bytes());
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
            latest_op_time: 1234567890,
            net_used: 2000,
            energy_used: 3000,
        };

        let serialized = store.serialize_resource_usage(&usage).unwrap();
        let deserialized = store.deserialize_resource_usage(&serialized).unwrap();

        assert_eq!(deserialized.free_net_used, usage.free_net_used);
        assert_eq!(deserialized.latest_op_time, usage.latest_op_time);
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
        assert_eq!(usage.latest_op_time, 0);
    }
}
