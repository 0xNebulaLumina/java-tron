use anyhow::Result;
use revm::primitives::{Address, AccountInfo, Bytecode, U256};
use tron_backend_storage::StorageEngine;
use crate::storage_adapter::traits::EvmStateStore;
use crate::storage_adapter::types::{WitnessInfo, FreezeRecord, VotesRecord, AccountAext};
use crate::storage_adapter::utils::{keccak256, to_tron_address};

/// Persistent implementation of EVM state store backed by the storage engine.
/// Routes data to appropriate RocksDB databases matching java-tron's organization
/// while providing a unified interface for EVM execution.
pub struct EngineBackedEvmStateStore {
    storage_engine: StorageEngine,
}

impl EngineBackedEvmStateStore {
    pub fn new(storage_engine: StorageEngine) -> Self {
        Self {
            storage_engine,
        }
    }

    // Database name methods

    fn account_database(&self) -> &str { "account" }
    fn code_database(&self) -> &str { "code" }
    fn contract_state_database(&self) -> &str { "contract-state" }
    fn contract_database(&self) -> &str { "contract" }
    fn dynamic_properties_database(&self) -> &str { "properties" }
    fn witness_database(&self) -> &str { "witness" }
    fn votes_database(&self) -> &str { "votes" }
    fn freeze_records_database(&self) -> &str { "freeze-records" }
    fn account_name_database(&self) -> &str { "account-name" }
    fn account_aext_database(&self) -> &str { "account-resource" }

    // Key formatting methods

    fn account_key(&self, address: &Address) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.push(0x41); // Tron address prefix
        key.extend_from_slice(address.as_slice());
        key
    }

    fn code_key(&self, address: &Address) -> Vec<u8> {
        address.as_slice().to_vec()
    }

    fn witness_key(&self, address: &Address) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.push(0x41);
        key.extend_from_slice(address.as_slice());
        key
    }

    fn votes_key(&self, address: &Address) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.push(0x41);
        key.extend_from_slice(address.as_slice());
        key
    }

    fn freeze_record_key(&self, address: &Address, resource: u8) -> Vec<u8> {
        let mut key = Vec::with_capacity(22);
        key.push(0x41);
        key.extend_from_slice(address.as_slice());
        key.push(resource);
        key
    }

    fn account_aext_key(&self, address: &Address) -> Vec<u8> {
        address.as_slice().to_vec()
    }

    fn contract_storage_key(&self, address: &Address, storage_key: &U256) -> Vec<u8> {
        let addr_hash = keccak256(address.as_slice());
        let storage_key_bytes = storage_key.to_be_bytes::<32>();
        let mut composed_key = Vec::with_capacity(32);
        composed_key.extend_from_slice(&addr_hash.as_slice()[0..16]);
        composed_key.extend_from_slice(&storage_key_bytes[16..32]);
        composed_key
    }

    // Serialization methods

    fn serialize_account(&self, address: &Address, account: &AccountInfo) -> Vec<u8> {
        let mut data = Vec::new();
        let tron_address = self.account_key(address);
        data.push(0x0a);
        self.write_varint(&mut data, tron_address.len() as u64);
        data.extend_from_slice(&tron_address);
        data.push(0x10);
        data.push(0x00);
        let balance_u64 = account.balance.to::<u64>();
        data.push(0x20);
        self.write_varint(&mut data, balance_u64);
        use std::time::{SystemTime, UNIX_EPOCH};
        let create_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if create_time > 0 {
            data.push(0x48);
            self.write_varint(&mut data, create_time);
        }
        data
    }

    fn write_varint(&self, output: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            output.push(((value & 0x7F) | 0x80) as u8);
            value >>= 7;
        }
        output.push(value as u8);
    }

    fn deserialize_account(&self, data: &[u8]) -> Result<AccountInfo> {
        let balance = self.extract_balance_from_protobuf(data)?;
        Ok(AccountInfo {
            balance: U256::from(balance),
            nonce: 0,
            code_hash: revm::primitives::B256::ZERO,
            code: None,
        })
    }

    fn extract_balance_from_protobuf(&self, data: &[u8]) -> Result<u64> {
        let mut pos = 0;
        while pos < data.len() {
            if pos >= data.len() { break; }
            let (field_header, new_pos) = self.read_varint(data, pos)?;
            pos = new_pos;
            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;
            if field_number == 4 && wire_type == 0 {
                let (balance, _) = self.read_varint(data, pos)?;
                return Ok(balance);
            } else {
                pos = self.skip_field(data, pos, wire_type)?;
            }
        }
        Ok(0)
    }

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

    fn skip_field(&self, data: &[u8], pos: usize, wire_type: u64) -> Result<usize> {
        match wire_type {
            0 => { let (_, new_pos) = self.read_varint(data, pos)?; Ok(new_pos) },
            1 => Ok(pos + 8),
            2 => { let (length, new_pos) = self.read_varint(data, pos)?; Ok(new_pos + length as usize) },
            5 => Ok(pos + 4),
            _ => Err(anyhow::anyhow!("Unknown wire type: {}", wire_type))
        }
    }

    // Dynamic properties methods

    pub fn get_account_upgrade_cost(&self) -> Result<u64> {
        let key = b"ACCOUNT_UPGRADE_COST";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) if data.len() >= 8 => {
                Ok(u64::from_be_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]))
            },
            _ => Ok(9999000000),
        }
    }

    pub fn get_allow_multi_sign(&self) -> Result<bool> {
        let key = b"ALLOW_MULTI_SIGN";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) if !data.is_empty() => Ok(data[0] != 0),
            _ => Ok(true),
        }
    }

    pub fn support_black_hole_optimization(&self) -> Result<bool> {
        let key = b"ALLOW_BLACKHOLE_OPTIMIZATION";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = u64::from_be_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]);
                    Ok(val != 0)
                } else if !data.is_empty() {
                    Ok(data[0] != 0)
                } else {
                    Ok(false)
                }
            },
            None => Ok(false)
        }
    }

    pub fn support_allow_new_resource_model(&self) -> Result<bool> {
        let key = b"ALLOW_NEW_RESOURCE_MODEL";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = u64::from_be_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]);
                    Ok(val != 0)
                } else if !data.is_empty() {
                    Ok(data[0] != 0)
                } else {
                    Ok(true)
                }
            },
            None => Ok(true)
        }
    }

    pub fn support_unfreeze_delay(&self) -> Result<bool> {
        let key = b"UNFREEZE_DELAY_DAYS";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = u64::from_be_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]);
                    Ok(val > 0)
                } else if !data.is_empty() {
                    Ok(data[0] > 0)
                } else {
                    Ok(false)
                }
            },
            None => Ok(false)
        }
    }

    pub fn get_blackhole_address(&self) -> Result<Option<Address>> {
        let key = b"BLACK_HOLE_ADDRESS";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) if data.len() >= 20 => {
                let mut addr_bytes = [0u8; 20];
                addr_bytes.copy_from_slice(&data[0..20]);
                Ok(Some(Address::from(addr_bytes)))
            },
            _ => Ok(Self::default_blackhole_address())
        }
    }

    fn default_blackhole_address() -> Option<Address> {
        match tron_backend_common::from_tron_address("TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy") {
            Ok(bytes20) => Some(Address::from(bytes20)),
            Err(_) => None,
        }
    }

    pub fn get_free_net_limit(&self) -> Result<i64> {
        let key = b"FREE_NET_LIMIT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) if data.len() >= 8 => {
                Ok(i64::from_be_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]))
            },
            _ => Ok(5000)
        }
    }

    pub fn get_public_net_limit(&self) -> Result<i64> {
        let key = b"PUBLIC_NET_LIMIT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) if data.len() >= 8 => {
                Ok(i64::from_be_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]))
            },
            _ => Ok(14_400_000_000)
        }
    }

    pub fn get_public_net_usage(&self) -> Result<i64> {
        let key = b"PUBLIC_NET_USAGE";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) if data.len() >= 8 => {
                Ok(i64::from_be_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]))
            },
            _ => Ok(0)
        }
    }

    pub fn set_public_net_usage(&self, value: i64) -> Result<()> {
        let key = b"PUBLIC_NET_USAGE";
        let data = value.to_be_bytes();
        self.storage_engine.put(self.dynamic_properties_database(), key, &data)?;
        Ok(())
    }

    pub fn get_public_net_time(&self) -> Result<i64> {
        let key = b"PUBLIC_NET_TIME";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) if data.len() >= 8 => {
                Ok(i64::from_be_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]))
            },
            _ => Ok(0)
        }
    }

    pub fn set_public_net_time(&self, value: i64) -> Result<()> {
        let key = b"PUBLIC_NET_TIME";
        let data = value.to_be_bytes();
        self.storage_engine.put(self.dynamic_properties_database(), key, &data)?;
        Ok(())
    }

    pub fn get_total_net_weight(&self) -> Result<i64> {
        let key = b"TOTAL_NET_WEIGHT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) if data.len() >= 8 => {
                Ok(i64::from_be_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]))
            },
            _ => Ok(0)
        }
    }

    pub fn get_total_net_limit(&self) -> Result<i64> {
        let key = b"TOTAL_NET_LIMIT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) if data.len() >= 8 => {
                Ok(i64::from_be_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]))
            },
            _ => Ok(43_200_000_000)
        }
    }

    pub fn compute_total_net_weight(&self) -> Result<i64> {
        const TRX_PRECISION: u128 = 1_000_000;
        const BANDWIDTH_RESOURCE: u8 = 0;
        let mut total_sun: u128 = 0;
        let records = self.storage_engine.prefix_query(self.freeze_records_database(), &[])?;
        for kv in records {
            if kv.key.len() == 22 && kv.key[21] == BANDWIDTH_RESOURCE {
                let record = FreezeRecord::deserialize(&kv.value)?;
                total_sun = total_sun.checked_add(record.frozen_amount as u128)
                    .ok_or_else(|| anyhow::anyhow!("Overflow computing total net weight"))?;
            }
        }
        let weight = (total_sun / TRX_PRECISION) as i64;
        tracing::debug!("Computed total net weight: {} (from {} SUN)", weight, total_sun);
        Ok(weight)
    }

    pub fn compute_total_energy_weight(&self) -> Result<i64> {
        const TRX_PRECISION: u128 = 1_000_000;
        const ENERGY_RESOURCE: u8 = 1;
        let mut total_sun: u128 = 0;
        let records = self.storage_engine.prefix_query(self.freeze_records_database(), &[])?;
        for kv in records {
            if kv.key.len() == 22 && kv.key[21] == ENERGY_RESOURCE {
                let record = FreezeRecord::deserialize(&kv.value)?;
                total_sun = total_sun.checked_add(record.frozen_amount as u128)
                    .ok_or_else(|| anyhow::anyhow!("Overflow computing total energy weight"))?;
            }
        }
        let weight = (total_sun / TRX_PRECISION) as i64;
        tracing::debug!("Computed total energy weight: {} (from {} SUN)", weight, total_sun);
        Ok(weight)
    }

    // Witness operations

    pub fn get_witness(&self, address: &Address) -> Result<Option<WitnessInfo>> {
        let key = self.witness_key(address);
        tracing::debug!("Getting witness for address {:?}, key: {}", address, hex::encode(&key));
        match self.storage_engine.get(self.witness_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found witness data, length: {}", data.len());
                match WitnessInfo::deserialize(&data) {
                    Ok(witness) => {
                        tracing::debug!("Decoded witness as Protocol.Witness (protobuf) - URL: {}, votes: {}", witness.url, witness.vote_count);
                        Ok(Some(witness))
                    },
                    Err(e) => {
                        tracing::debug!("Protobuf decode failed ({}), trying legacy format", e);
                        Ok(None)
                    }
                }
            },
            None => {
                tracing::debug!("No witness found for address {:?}", address);
                Ok(None)
            }
        }
    }

    pub fn put_witness(&self, witness: &WitnessInfo) -> Result<()> {
        let key = self.witness_key(&witness.address);
        let data = witness.serialize();
        tracing::debug!("Storing witness (protobuf format) for address {:?}, key: {}, URL: {}, votes: {}",
                       witness.address, hex::encode(&key), witness.url, witness.vote_count);
        self.storage_engine.put(self.witness_database(), &key, &data)?;
        Ok(())
    }

    pub fn is_witness(&self, address: &Address) -> Result<bool> {
        Ok(self.get_witness(address)?.is_some())
    }

    // Vote operations

    pub fn get_votes(&self, address: &Address) -> Result<Option<VotesRecord>> {
        let key = self.votes_key(address);
        tracing::debug!("Getting votes for address {:?}, key: {}", address, hex::encode(&key));
        match self.storage_engine.get(self.votes_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found votes data, length: {}", data.len());
                match VotesRecord::deserialize(&data) {
                    Ok(votes) => {
                        tracing::debug!("Successfully deserialized votes - old_votes: {}, new_votes: {}", votes.old_votes.len(), votes.new_votes.len());
                        Ok(Some(votes))
                    },
                    Err(e) => {
                        tracing::error!("Failed to deserialize votes data: {}", e);
                        Ok(None)
                    }
                }
            },
            None => {
                tracing::debug!("No votes found for address {:?}", address);
                Ok(None)
            }
        }
    }

    pub fn set_votes(&self, address: Address, votes: &VotesRecord) -> Result<()> {
        let key = self.votes_key(&address);
        let data = votes.serialize();
        tracing::debug!("Storing votes for address {:?}, key: {}, old_votes: {}, new_votes: {}",
                       address, hex::encode(&key), votes.old_votes.len(), votes.new_votes.len());
        self.storage_engine.put(self.votes_database(), &key, &data)?;
        Ok(())
    }

    // Freeze record operations

    pub fn get_freeze_record(&self, address: &Address, resource: u8) -> Result<Option<FreezeRecord>> {
        let key = self.freeze_record_key(address, resource);
        tracing::debug!("Getting freeze record for address {:?}, resource {}, key: {}",
                       address, resource, hex::encode(&key));
        match self.storage_engine.get(self.freeze_records_database(), &key)? {
            Some(data) => {
                let record = FreezeRecord::deserialize(&data)?;
                tracing::debug!("Found freeze record: amount={}, expiration={}", record.frozen_amount, record.expiration_timestamp);
                Ok(Some(record))
            },
            None => {
                tracing::debug!("No freeze record found");
                Ok(None)
            }
        }
    }

    pub fn set_freeze_record(&self, address: Address, resource: u8, record: &FreezeRecord) -> Result<()> {
        let key = self.freeze_record_key(&address, resource);
        let data = record.serialize();
        tracing::debug!("Storing freeze record for address {:?}, resource {}, key: {}, amount={}, expiration={}",
                       address, resource, hex::encode(&key), record.frozen_amount, record.expiration_timestamp);
        self.storage_engine.put(self.freeze_records_database(), &key, &data)?;
        Ok(())
    }

    pub fn add_freeze_amount(&self, address: Address, resource: u8, amount: u64, expiration: i64) -> Result<()> {
        let mut record = self.get_freeze_record(&address, resource)?
            .unwrap_or(FreezeRecord::new(0, 0));
        record.frozen_amount = record.frozen_amount.checked_add(amount)
            .ok_or_else(|| anyhow::anyhow!("Freeze amount overflow"))?;
        record.expiration_timestamp = record.expiration_timestamp.max(expiration);
        self.set_freeze_record(address, resource, &record)?;
        Ok(())
    }

    pub fn remove_freeze_record(&self, address: &Address, resource: u8) -> Result<()> {
        let key = self.freeze_record_key(address, resource);
        tracing::debug!("Removing freeze record for address {:?}, resource {}, key: {}", address, resource, hex::encode(&key));
        self.storage_engine.delete(self.freeze_records_database(), &key)?;
        Ok(())
    }

    pub fn get_tron_power_in_sun(&self, address: &Address, new_model: bool) -> Result<u64> {
        const BANDWIDTH: u8 = 0;
        const ENERGY: u8 = 1;
        const TRON_POWER: u8 = 2;
        let mut total: u64 = 0;
        let mut bandwidth_amount: u64 = 0;
        let mut energy_amount: u64 = 0;
        let mut tron_power_amount: u64 = 0;
        for resource in [BANDWIDTH, ENERGY, TRON_POWER] {
            if let Some(record) = self.get_freeze_record(address, resource)? {
                let amount = record.frozen_amount;
                total = total.checked_add(amount)
                    .ok_or_else(|| anyhow::anyhow!(
                        "Tron power overflow when adding resource {} amount {} to total {}",
                        resource, amount, total
                    ))?;
                match resource {
                    BANDWIDTH => bandwidth_amount = amount,
                    ENERGY => energy_amount = amount,
                    TRON_POWER => tron_power_amount = amount,
                    _ => {}
                }
            }
        }
        tracing::info!(
            address = ?address, new_model = new_model, bandwidth = bandwidth_amount,
            energy = energy_amount, tron_power_legacy = tron_power_amount, total = total,
            "Computed tron power from freeze ledger"
        );
        Ok(total)
    }

    // Account name operations

    pub fn get_account_name(&self, address: &Address) -> Result<Option<String>> {
        let key = self.account_key(address);
        tracing::debug!("Getting account name for address {:?}, key: {}", address, hex::encode(&key));
        match self.storage_engine.get(self.account_name_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found account name data, length: {}", data.len());
                match String::from_utf8(data) {
                    Ok(name) => {
                        tracing::debug!("Successfully decoded account name: {}", name);
                        Ok(Some(name))
                    },
                    Err(e) => {
                        tracing::error!("Failed to decode account name as UTF-8: {}", e);
                        Err(anyhow::anyhow!("Invalid UTF-8 in account name: {}", e))
                    }
                }
            },
            None => {
                tracing::debug!("No account name found for address {:?}", address);
                Ok(None)
            }
        }
    }

    pub fn set_account_name(&mut self, address: Address, name: &[u8]) -> Result<()> {
        let key = self.account_key(&address);
        tracing::debug!("Setting account name for address {:?}, key: {}, name_len: {}",
                       address, hex::encode(&key), name.len());
        if name.is_empty() {
            return Err(anyhow::anyhow!("Account name cannot be empty"));
        }
        if name.len() > 32 {
            return Err(anyhow::anyhow!("Account name cannot exceed 32 bytes, got {}", name.len()));
        }
        match std::str::from_utf8(name) {
            Ok(name_str) => {
                tracing::debug!("Account name is valid UTF-8: {}", name_str);
            },
            Err(e) => {
                tracing::warn!("Account name contains invalid UTF-8: {}, allowing raw bytes", e);
            }
        }
        self.storage_engine.put(self.account_name_database(), &key, name)?;
        tracing::info!("Successfully stored account name for address {:?}, length: {}", address, name.len());
        Ok(())
    }

    // Account AEXT operations

    pub fn get_account_aext(&self, address: &Address) -> Result<Option<AccountAext>> {
        let key = self.account_aext_key(address);
        tracing::debug!("Getting account AEXT for address {:?}, key: {}", address, hex::encode(&key));
        match self.storage_engine.get(self.account_aext_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found account AEXT data, length: {}", data.len());
                match AccountAext::deserialize(&data) {
                    Ok(aext) => {
                        tracing::debug!("Successfully deserialized account AEXT - net_usage: {}, free_net_usage: {}, net_window: {}",
                                       aext.net_usage, aext.free_net_usage, aext.net_window_size);
                        Ok(Some(aext))
                    },
                    Err(e) => {
                        tracing::warn!("Failed to deserialize account AEXT data: {}, returning None", e);
                        Ok(None)
                    }
                }
            },
            None => {
                tracing::debug!("No account AEXT found for address {:?}", address);
                Ok(None)
            }
        }
    }

    pub fn set_account_aext(&self, address: &Address, aext: &AccountAext) -> Result<()> {
        let key = self.account_aext_key(address);
        let data = aext.serialize();
        tracing::debug!("Setting account AEXT for address {:?}, net_usage: {}, free_net_usage: {}, net_window: {}",
                       address, aext.net_usage, aext.free_net_usage, aext.net_window_size);
        self.storage_engine.put(self.account_aext_database(), &key, &data)?;
        tracing::debug!("Successfully stored account AEXT for address {:?}", address);
        Ok(())
    }

    pub fn get_or_init_account_aext(&self, address: &Address) -> Result<AccountAext> {
        if let Some(aext) = self.get_account_aext(address)? {
            Ok(aext)
        } else {
            let aext = AccountAext::with_defaults();
            self.set_account_aext(address, &aext)?;
            Ok(aext)
        }
    }

    // Method alias shims

    pub fn put_freeze_record(&self, address: Address, resource: u8, record: &FreezeRecord) -> Result<()> {
        self.set_freeze_record(address, resource, record)
    }

    pub fn compute_tron_power_in_sun(&self, address: &Address, new_model: bool) -> Result<u64> {
        self.get_tron_power_in_sun(address, new_model)
    }
}

impl EvmStateStore for EngineBackedEvmStateStore {
    fn get_account(&self, address: &Address) -> Result<Option<AccountInfo>> {
        let key = self.account_key(address);
        let address_tron = to_tron_address(address);
        tracing::info!("Getting account for address {:?} (tron: {}), key: {}", address, address_tron, hex::encode(&key));
        match self.storage_engine.get(self.account_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found account data, length: {}, first 32 bytes: {}",
                               data.len(), hex::encode(&data[..std::cmp::min(32, data.len())]));
                match self.deserialize_account(&data) {
                    Ok(account) => {
                        tracing::info!("Successfully deserialized account - balance: {}, nonce: {}", account.balance, account.nonce);
                        Ok(Some(account))
                    },
                    Err(e) => {
                        tracing::error!("Failed to deserialize account data: {}", e);
                        let default_balance = revm::primitives::U256::from(0u64);
                        let default_account = AccountInfo {
                            balance: default_balance,
                            nonce: 0,
                            code_hash: keccak256(&[]),
                            code: None,
                        };
                        tracing::warn!("Providing default account due to deserialization error, balance: {}", default_balance);
                        Ok(Some(default_account))
                    }
                }
            },
            None => {
                tracing::info!("No account data found for address {:?} with key {} - account does not exist", address, hex::encode(&key));
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
                       address, address_tron, account.balance, hex::encode(&key), data.len(), hex::encode(&data));
        self.storage_engine.put(self.account_database(), &key, &data)?;
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
        let account_key = self.account_key(address);
        self.storage_engine.delete(self.account_database(), &account_key)?;
        let code_key = self.code_key(address);
        self.storage_engine.delete(self.code_database(), &code_key)?;
        Ok(())
    }
}
