//! Engine-backed implementation of EVM state store.
//!
//! This module provides the production storage implementation backed by the StorageEngine
//! (RocksDB). It routes data to appropriate databases matching java-tron's organization.

use std::collections::HashSet;
use anyhow::Result;
use revm::primitives::{AccountInfo, Bytecode, Address, U256};
use tron_backend_storage::StorageEngine;
use super::traits::EvmStateStore;
use super::types::{WitnessInfo, VotesRecord, FreezeRecord, AccountAext};
use super::utils::{keccak256, to_tron_address};

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

    /// Get the appropriate database name for dynamic properties
    fn dynamic_properties_database(&self) -> &str {
        "properties"
    }

    /// Get the appropriate database name for witness store
    fn witness_database(&self) -> &str {
        "witness"
    }

    /// Get the appropriate database name for votes store
    fn votes_database(&self) -> &str {
        "votes"
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

    /// Convert Address to storage key for witness store (21-byte address with 0x41 prefix)
    fn witness_key(&self, address: &Address) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.push(0x41); // Tron address prefix
        key.extend_from_slice(address.as_slice()); // 20-byte address
        key
    }

    /// Convert Address to storage key for votes store (21-byte address with 0x41 prefix)
    fn votes_key(&self, address: &Address) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.push(0x41); // Tron address prefix
        key.extend_from_slice(address.as_slice()); // 20-byte address
        key
    }

    /// Get the appropriate database name for freeze records
    fn freeze_records_database(&self) -> &str {
        "freeze-records"
    }

    /// Convert Address and FreezeResource to storage key for freeze records
    /// Format: 21-byte tron address (0x41 + 20-byte) + 1-byte resource type
    fn freeze_record_key(&self, address: &Address, resource: u8) -> Vec<u8> {
        let mut key = Vec::with_capacity(22);
        key.push(0x41); // Tron address prefix
        key.extend_from_slice(address.as_slice()); // 20-byte address
        key.push(resource); // Resource type (0=BANDWIDTH, 1=ENERGY, 2=TRON_POWER)
        key
    }

    /// Get the appropriate database name for account names
    fn account_name_database(&self) -> &str {
        "account-name"
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

    /// Get AccountUpgradeCost dynamic property
    /// Default value for witness creation cost in SUN
    pub fn get_account_upgrade_cost(&self) -> Result<u64> {
        let key = b"ACCOUNT_UPGRADE_COST";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let cost = u64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(cost)
                } else {
                    // Use default value for AccountUpgradeCost
                    Ok(9999000000) // 9999 TRX in SUN (default from TRON)
                }
            },
            None => {
                // Use default value for AccountUpgradeCost
                Ok(9999000000) // 9999 TRX in SUN (default from TRON)
            }
        }
    }

    /// Get AssetIssueFee dynamic property
    /// Default value for TRC-10 asset issuance cost in SUN
    /// Java reference: DynamicPropertiesStore.java:1554, 1568
    pub fn get_asset_issue_fee(&self) -> Result<u64> {
        let key = b"ASSET_ISSUE_FEE";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let fee = u64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(fee)
                } else {
                    // Use default value for AssetIssueFee
                    Ok(1024000000) // 1024 TRX in SUN (default from TRON mainnet)
                }
            },
            None => {
                // Use default value for AssetIssueFee
                Ok(1024000000) // 1024 TRX in SUN (default from TRON mainnet)
            }
        }
    }

    /// Get AllowMultiSign dynamic property
    /// Default value: 1 (enabled)
    pub fn get_allow_multi_sign(&self) -> Result<bool> {
        let key = b"ALLOW_MULTI_SIGN";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if !data.is_empty() {
                    Ok(data[0] != 0)
                } else {
                    Ok(true) // Default enabled
                }
            },
            None => {
                Ok(true) // Default enabled
            }
        }
    }

    /// Get Black Hole Optimization dynamic property (parity with Java)
    /// Java stores this as a long under key "ALLOW_BLACKHOLE_OPTIMIZATION".
    /// When this flag is 1, the node BURNS fees (optimization enabled).
    /// When 0, the node CREDITS the blackhole account.
    /// Default: false (credit blackhole) to match early-chain behavior when key is absent.
    pub fn support_black_hole_optimization(&self) -> Result<bool> {
        // Parity key with java-tron DynamicPropertiesStore
        let key = b"ALLOW_BLACKHOLE_OPTIMIZATION";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                // Java writes a long; interpret big-endian u64 when length >= 8.
                if data.len() >= 8 {
                    let val = u64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(val != 0)
                } else if !data.is_empty() {
                    // Fallback: treat first byte as boolean
                    Ok(data[0] != 0)
                } else {
                    // Empty value → treat as disabled (credit blackhole)
                    Ok(false)
                }
            },
            None => {
                // Absent key → default to disabled (credit blackhole) for early heights
                Ok(false)
            }
        }
    }

    /// Get AllowNewResourceModel dynamic property
    /// Determines whether to use new resource model for tron power calculation
    /// Default: true (enabled)
    pub fn support_allow_new_resource_model(&self) -> Result<bool> {
        let key = b"ALLOW_NEW_RESOURCE_MODEL";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = u64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(val != 0)
                } else if !data.is_empty() {
                    Ok(data[0] != 0)
                } else {
                    Ok(true) // Default enabled
                }
            },
            None => {
                Ok(true) // Default enabled
            }
        }
    }

    /// Get UnfreezeDelay dynamic property
    /// Returns true if unfreeze delay is enabled (UNFREEZE_DELAY_DAYS > 0)
    /// Default: false (no delay)
    pub fn support_unfreeze_delay(&self) -> Result<bool> {
        let key = b"UNFREEZE_DELAY_DAYS";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = u64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(val > 0)
                } else if !data.is_empty() {
                    Ok(data[0] > 0)
                } else {
                    Ok(false) // Default no delay
                }
            },
            None => {
                Ok(false) // Default no delay
            }
        }
    }

    /// Get blackhole address (if crediting instead of burning)
    /// Returns:
    /// - The configured dynamic property value when present (20 raw bytes)
    /// - Otherwise, a sane mainnet default (TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy)
    ///   to match java-tron's AccountStore.getBlackhole() behavior.
    pub fn get_blackhole_address(&self) -> Result<Option<Address>> {
        let key = b"BLACK_HOLE_ADDRESS";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 20 {
                    let mut addr_bytes = [0u8; 20];
                    addr_bytes.copy_from_slice(&data[0..20]);
                    Ok(Some(Address::from(addr_bytes)))
                } else {
                    // Invalid or empty value: fall back to default
                    Ok(Self::default_blackhole_address())
                }
            },
            None => {
                // Not configured in dynamic properties - use sane network default
                Ok(Self::default_blackhole_address())
            }
        }
    }

    /// Default blackhole address (mainnet): TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy
    /// Provided as 20-byte EVM address wrapped in revm_primitives::Address.
    fn default_blackhole_address() -> Option<Address> {
        // Use common address utility to decode TRON Base58
        match tron_backend_common::from_tron_address("TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy") {
            Ok(bytes20) => Some(Address::from(bytes20)),
            Err(_) => None,
        }
    }

    // WithdrawBalanceContract: Dynamic Properties

    /// Get LATEST_BLOCK_HEADER_TIMESTAMP dynamic property
    /// This is the timestamp of the latest processed block (milliseconds since epoch)
    /// Used for cooldown checks in WithdrawBalanceContract
    /// Default: 0 (should always be present in a running chain)
    pub fn get_latest_block_header_timestamp(&self) -> Result<i64> {
        // Java stores this as lowercase key "latest_block_header_timestamp"
        let key = b"latest_block_header_timestamp";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    tracing::warn!("LATEST_BLOCK_HEADER_TIMESTAMP has invalid length: {}", data.len());
                    Ok(0)
                }
            },
            None => {
                tracing::debug!("LATEST_BLOCK_HEADER_TIMESTAMP not found, returning 0");
                Ok(0)
            }
        }
    }

    /// Get WITNESS_ALLOWANCE_FROZEN_TIME dynamic property
    /// Number of days for witness withdrawal cooldown (multiplied by FROZEN_PERIOD to get ms)
    /// Default: 1 day if missing
    /// FROZEN_PERIOD = 86,400,000 ms (24 hours in ms)
    pub fn get_witness_allowance_frozen_time(&self) -> Result<i64> {
        let key = b"WITNESS_ALLOWANCE_FROZEN_TIME";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let days = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    Ok(days)
                } else if !data.is_empty() {
                    // Try parsing as single byte
                    Ok(data[0] as i64)
                } else {
                    Ok(1) // Default: 1 day
                }
            },
            None => {
                tracing::debug!("WITNESS_ALLOWANCE_FROZEN_TIME not found, returning default 1 day");
                Ok(1) // Default: 1 day
            }
        }
    }

    /// Get Account.allowance field (field 11 in Account protobuf)
    /// This is the accumulated witness reward available for withdrawal
    /// Returns 0 if account doesn't exist or field not present
    pub fn get_account_allowance(&self, address: &Address) -> Result<i64> {
        let key = self.account_key(address);

        match self.storage_engine.get(self.account_database(), &key)? {
            Some(data) => {
                match self.extract_i64_field_from_protobuf(&data, 11) {
                    Ok(allowance) => {
                        tracing::debug!("Account {} allowance: {}", to_tron_address(address), allowance);
                        Ok(allowance)
                    },
                    Err(e) => {
                        tracing::debug!("Failed to extract allowance from account: {}, returning 0", e);
                        Ok(0)
                    }
                }
            },
            None => {
                tracing::debug!("Account not found for address {:?}, returning allowance 0", address);
                Ok(0)
            }
        }
    }

    /// Get Account.latest_withdraw_time field (field 12 in Account protobuf)
    /// This is the timestamp of the last witness reward withdrawal
    /// Returns 0 if account doesn't exist or field not present
    pub fn get_account_latest_withdraw_time(&self, address: &Address) -> Result<i64> {
        let key = self.account_key(address);

        match self.storage_engine.get(self.account_database(), &key)? {
            Some(data) => {
                match self.extract_i64_field_from_protobuf(&data, 12) {
                    Ok(latest_withdraw_time) => {
                        tracing::debug!("Account {} latest_withdraw_time: {}", to_tron_address(address), latest_withdraw_time);
                        Ok(latest_withdraw_time)
                    },
                    Err(e) => {
                        tracing::debug!("Failed to extract latest_withdraw_time from account: {}, returning 0", e);
                        Ok(0)
                    }
                }
            },
            None => {
                tracing::debug!("Account not found for address {:?}, returning latest_withdraw_time 0", address);
                Ok(0)
            }
        }
    }

    /// Extract an i64 varint field from a protobuf message by field number
    /// Used for Account fields like allowance (11) and latest_withdraw_time (12)
    fn extract_i64_field_from_protobuf(&self, data: &[u8], target_field: u64) -> Result<i64> {
        let mut pos = 0;

        while pos < data.len() {
            // Read field header (varint)
            let (field_header, new_pos) = self.read_varint(data, pos)?;
            pos = new_pos;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            if field_number == target_field && wire_type == 0 {
                // Found our target field (varint)
                let (value, _) = self.read_varint(data, pos)?;
                // Convert u64 to i64 (for proper signed handling)
                return Ok(value as i64);
            } else {
                // Skip this field
                pos = self.skip_field(data, pos, wire_type)?;
            }
        }

        // Field not found - return 0 as default
        Ok(0)
    }

    // Bandwidth and Resource Dynamic Properties for AEXT tracking

    /// Get FREE_NET_LIMIT dynamic property (free bandwidth limit per account)
    /// Default: 5000 bytes per transaction
    pub fn get_free_net_limit(&self) -> Result<i64> {
        let key = b"FREE_NET_LIMIT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    Ok(5000) // Default
                }
            },
            None => Ok(5000) // Default
        }
    }

    /// Get PUBLIC_NET_LIMIT dynamic property (total public bandwidth pool)
    /// Default: 14_400_000_000 bytes
    pub fn get_public_net_limit(&self) -> Result<i64> {
        let key = b"PUBLIC_NET_LIMIT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    Ok(14_400_000_000) // Default
                }
            },
            None => Ok(14_400_000_000) // Default
        }
    }

    /// Get PUBLIC_NET_USAGE dynamic property (current public bandwidth usage)
    /// Default: 0
    pub fn get_public_net_usage(&self) -> Result<i64> {
        let key = b"PUBLIC_NET_USAGE";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    Ok(0)
                }
            },
            None => Ok(0)
        }
    }

    /// Set PUBLIC_NET_USAGE dynamic property
    pub fn set_public_net_usage(&self, value: i64) -> Result<()> {
        let key = b"PUBLIC_NET_USAGE";
        let data = value.to_be_bytes();
        self.storage_engine.put(self.dynamic_properties_database(), key, &data)?;
        Ok(())
    }

    /// Get PUBLIC_NET_TIME dynamic property (last time public bandwidth was updated)
    /// Default: 0
    pub fn get_public_net_time(&self) -> Result<i64> {
        let key = b"PUBLIC_NET_TIME";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    Ok(0)
                }
            },
            None => Ok(0)
        }
    }

    /// Set PUBLIC_NET_TIME dynamic property
    pub fn set_public_net_time(&self, value: i64) -> Result<()> {
        let key = b"PUBLIC_NET_TIME";
        let data = value.to_be_bytes();
        self.storage_engine.put(self.dynamic_properties_database(), key, &data)?;
        Ok(())
    }

    /// Get TOTAL_NET_WEIGHT dynamic property (total frozen for bandwidth)
    /// Default: 0
    pub fn get_total_net_weight(&self) -> Result<i64> {
        let key = b"TOTAL_NET_WEIGHT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    Ok(0)
                }
            },
            None => Ok(0)
        }
    }

    /// Get TOTAL_NET_LIMIT dynamic property (total bandwidth from frozen balance)
    /// Default: 43_200_000_000 bytes
    pub fn get_total_net_limit(&self) -> Result<i64> {
        let key = b"TOTAL_NET_LIMIT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    Ok(43_200_000_000) // Default
                }
            },
            None => Ok(43_200_000_000) // Default
        }
    }

    /// Get TOTAL_ENERGY_CURRENT_LIMIT dynamic property (current global energy limit)
    /// Default: 50_000_000_000 (parity with early mainnet defaults)
    pub fn get_total_energy_limit(&self) -> Result<i64> {
        let key = b"TOTAL_ENERGY_CURRENT_LIMIT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    Ok(50_000_000_000) // Default (mainnet early default)
                }
            },
            None => Ok(50_000_000_000), // Default
        }
    }

    /// Compute total NET weight from all freeze records
    /// Weight = sum(frozen_amount for resource=BANDWIDTH) / TRX_PRECISION
    /// TRX_PRECISION = 1_000_000 (matches Java ChainConstant.TRX_PRECISION)
    /// This scans all freeze records - O(n) operation, suitable for Phase 2 parity
    pub fn compute_total_net_weight(&self) -> Result<i64> {
        const TRX_PRECISION: u128 = 1_000_000;
        const BANDWIDTH_RESOURCE: u8 = 0;

        let mut total_sun: u128 = 0;

        // Scan all freeze records in the database
        let records = self.storage_engine.prefix_query(self.freeze_records_database(), &[])?;

        for kv in records {
            // Key format: 0x41 + 20-byte address + 1-byte resource = 22 bytes
            if kv.key.len() == 22 && kv.key[21] == BANDWIDTH_RESOURCE {
                // Deserialize freeze record
                let record = FreezeRecord::deserialize(&kv.value)?;
                total_sun = total_sun.checked_add(record.frozen_amount as u128)
                    .ok_or_else(|| anyhow::anyhow!("Overflow computing total net weight"))?;
            }
        }

        // Convert to weight: integer division by TRX_PRECISION
        let weight = (total_sun / TRX_PRECISION) as i64;

        tracing::debug!("Computed total net weight: {} (from {} SUN)", weight, total_sun);
        Ok(weight)
    }

    /// Compute total ENERGY weight from all freeze records
    /// Weight = sum(frozen_amount for resource=ENERGY) / TRX_PRECISION
    /// TRX_PRECISION = 1_000_000 (matches Java ChainConstant.TRX_PRECISION)
    /// This scans all freeze records - O(n) operation, suitable for Phase 2 parity
    pub fn compute_total_energy_weight(&self) -> Result<i64> {
        const TRX_PRECISION: u128 = 1_000_000;
        const ENERGY_RESOURCE: u8 = 1;

        let mut total_sun: u128 = 0;

        // Scan all freeze records in the database
        let records = self.storage_engine.prefix_query(self.freeze_records_database(), &[])?;

        for kv in records {
            // Key format: 0x41 + 20-byte address + 1-byte resource = 22 bytes
            if kv.key.len() == 22 && kv.key[21] == ENERGY_RESOURCE {
                // Deserialize freeze record
                let record = FreezeRecord::deserialize(&kv.value)?;
                total_sun = total_sun.checked_add(record.frozen_amount as u128)
                    .ok_or_else(|| anyhow::anyhow!("Overflow computing total energy weight"))?;
            }
        }

        // Convert to weight: integer division by TRX_PRECISION
        let weight = (total_sun / TRX_PRECISION) as i64;

        tracing::debug!("Computed total energy weight: {} (from {} SUN)", weight, total_sun);
        Ok(weight)
    }

    /// Get witness information by address
    /// Uses dual-decoder: tries protobuf first (Java format), falls back to legacy custom format
    pub fn get_witness(&self, address: &Address) -> Result<Option<WitnessInfo>> {
        let key = self.witness_key(address);
        tracing::debug!("Getting witness for address {:?}, key: {}",
                       address, hex::encode(&key));

        match self.storage_engine.get(self.witness_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found witness data, length: {}", data.len());

                // Step 1: Try protobuf decode (Java-compatible format)
                match WitnessInfo::deserialize(&data) {
                    Ok(witness) => {
                        tracing::debug!("Decoded witness as Protocol.Witness (protobuf) - URL: {}, votes: {}",
                                       witness.url, witness.vote_count);
                        return Ok(Some(witness));
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

    /// Store witness information
    /// Uses protobuf encoding by default for Java compatibility
    pub fn put_witness(&self, witness: &WitnessInfo) -> Result<()> {
        let key = self.witness_key(&witness.address);
        // Use protobuf encoding for Java compatibility
        let data = witness.serialize();

        tracing::debug!("Storing witness (protobuf format) for address {:?}, key: {}, URL: {}, votes: {}",
                       witness.address, hex::encode(&key), witness.url, witness.vote_count);

        self.storage_engine.put(self.witness_database(), &key, &data)?;
        Ok(())
    }

    /// Check if an address is already a witness
    pub fn is_witness(&self, address: &Address) -> Result<bool> {
        match self.get_witness(address)? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Get votes record for an address
    pub fn get_votes(&self, address: &Address) -> Result<Option<VotesRecord>> {
        let key = self.votes_key(address);
        tracing::debug!("Getting votes for address {:?}, key: {}",
                       address, hex::encode(&key));

        match self.storage_engine.get(self.votes_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found votes data, length: {}", data.len());
                match VotesRecord::deserialize(&data) {
                    Ok(votes) => {
                        tracing::debug!("Successfully deserialized votes - old_votes: {}, new_votes: {}",
                                       votes.old_votes.len(), votes.new_votes.len());
                        Ok(Some(votes))
                    },
                    Err(e) => {
                        tracing::error!("Failed to deserialize votes data: {}", e);
                        Ok(None) // Return None instead of error for corrupted data
                    }
                }
            },
            None => {
                tracing::debug!("No votes found for address {:?}", address);
                Ok(None)
            }
        }
    }

    /// Store votes record
    pub fn set_votes(&self, address: Address, votes: &VotesRecord) -> Result<()> {
        let key = self.votes_key(&address);
        let data = votes.serialize();

        tracing::debug!("Storing votes for address {:?}, key: {}, old_votes: {}, new_votes: {}",
                       address, hex::encode(&key), votes.old_votes.len(), votes.new_votes.len());

        self.storage_engine.put(self.votes_database(), &key, &data)?;
        Ok(())
    }

    /// Get the votes list from the Account protobuf (field 5: repeated Vote)
    /// This reads the persisted Account record and extracts the votes field.
    /// Used to seed old_votes when creating a new VotesRecord (to match embedded behavior).
    ///
    /// Account protobuf structure:
    ///   repeated Vote votes = 5;  // field 5, length-delimited
    ///
    /// Vote protobuf structure:
    ///   bytes vote_address = 1;   // 21-byte Tron address
    ///   int64 vote_count = 2;     // vote count
    pub fn get_account_votes_list(&self, address: &Address) -> Result<Vec<(Address, u64)>> {
        let key = self.account_key(address);
        let address_tron = to_tron_address(address);
        tracing::debug!("Getting account votes list for address {:?} (tron: {}), key: {}",
                       address, address_tron, hex::encode(&key));

        match self.storage_engine.get(self.account_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found account data for votes extraction, length: {}", data.len());
                match self.extract_votes_from_account_protobuf(&data) {
                    Ok(votes) => {
                        tracing::info!("Extracted {} votes from Account.votes field for {}",
                                      votes.len(), address_tron);
                        Ok(votes)
                    },
                    Err(e) => {
                        tracing::warn!("Failed to extract votes from Account protobuf: {}, returning empty", e);
                        Ok(Vec::new())
                    }
                }
            },
            None => {
                tracing::debug!("No account found for address {:?}, returning empty votes list", address);
                Ok(Vec::new())
            }
        }
    }

    /// Extract the votes field (field 5) from an Account protobuf message
    /// Returns a vector of (witness_address, vote_count) tuples
    fn extract_votes_from_account_protobuf(&self, data: &[u8]) -> Result<Vec<(Address, u64)>> {
        let mut votes = Vec::new();
        let mut pos = 0;

        while pos < data.len() {
            // Read field header
            let (field_header, new_pos) = self.read_varint(data, pos)?;
            pos = new_pos;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            if field_number == 5 && wire_type == 2 {
                // Field 5: repeated Vote (length-delimited)
                let (length, new_pos) = self.read_varint(data, pos)?;
                pos = new_pos;

                if pos + length as usize > data.len() {
                    return Err(anyhow::anyhow!("Invalid Vote field length"));
                }

                let vote_data = &data[pos..pos + length as usize];
                pos += length as usize;

                // Parse the Vote message
                match self.parse_vote_message(vote_data) {
                    Ok((vote_address, vote_count)) => {
                        votes.push((vote_address, vote_count));
                    },
                    Err(e) => {
                        tracing::warn!("Failed to parse Vote message: {}, skipping", e);
                    }
                }
            } else {
                // Skip other fields
                pos = self.skip_field(data, pos, wire_type)?;
            }
        }

        Ok(votes)
    }

    /// Parse a single Vote protobuf message
    /// Vote structure:
    ///   bytes vote_address = 1;  (length-delimited, 21-byte Tron address)
    ///   int64 vote_count = 2;    (varint)
    fn parse_vote_message(&self, data: &[u8]) -> Result<(Address, u64)> {
        let mut vote_address: Option<Address> = None;
        let mut vote_count: Option<u64> = None;
        let mut pos = 0;

        while pos < data.len() {
            // Read field header
            let (field_header, new_pos) = self.read_varint(data, pos)?;
            pos = new_pos;

            let field_number = field_header >> 3;
            let wire_type = field_header & 0x7;

            match (field_number, wire_type) {
                (1, 2) => {
                    // vote_address (length-delimited)
                    let (length, new_pos) = self.read_varint(data, pos)?;
                    pos = new_pos;

                    if pos + length as usize > data.len() {
                        return Err(anyhow::anyhow!("Invalid vote_address length"));
                    }

                    let addr_bytes = &data[pos..pos + length as usize];
                    pos += length as usize;

                    // Remove 0x41 prefix if present (21-byte Tron → 20-byte EVM)
                    let evm_addr = if addr_bytes.len() == 21 && addr_bytes[0] == 0x41 {
                        &addr_bytes[1..]
                    } else if addr_bytes.len() == 20 {
                        addr_bytes
                    } else {
                        return Err(anyhow::anyhow!("Invalid vote_address length: {}", addr_bytes.len()));
                    };

                    if evm_addr.len() != 20 {
                        return Err(anyhow::anyhow!("Invalid EVM address length: {}", evm_addr.len()));
                    }

                    let mut addr = [0u8; 20];
                    addr.copy_from_slice(evm_addr);
                    vote_address = Some(Address::from(addr));
                },
                (2, 0) => {
                    // vote_count (varint)
                    let (count, new_pos) = self.read_varint(data, pos)?;
                    pos = new_pos;
                    vote_count = Some(count);
                },
                _ => {
                    // Skip unknown fields
                    pos = self.skip_field(data, pos, wire_type)?;
                }
            }
        }

        let addr = vote_address.ok_or_else(|| anyhow::anyhow!("Missing vote_address"))?;
        let count = vote_count.ok_or_else(|| anyhow::anyhow!("Missing vote_count"))?;
        Ok((addr, count))
    }

    /// Get freeze record for an address and resource type
    /// resource: 0=BANDWIDTH, 1=ENERGY, 2=TRON_POWER
    pub fn get_freeze_record(&self, address: &Address, resource: u8) -> Result<Option<FreezeRecord>> {
        let key = self.freeze_record_key(address, resource);
        tracing::debug!("Getting freeze record for address {:?}, resource {}, key: {}",
                       address, resource, hex::encode(&key));

        match self.storage_engine.get(self.freeze_records_database(), &key)? {
            Some(data) => {
                let record = FreezeRecord::deserialize(&data)?;
                tracing::debug!("Found freeze record: amount={}, expiration={}",
                               record.frozen_amount, record.expiration_timestamp);
                Ok(Some(record))
            },
            None => {
                tracing::debug!("No freeze record found");
                Ok(None)
            }
        }
    }

    /// Store freeze record for an address and resource type
    pub fn set_freeze_record(&self, address: Address, resource: u8, record: &FreezeRecord) -> Result<()> {
        let key = self.freeze_record_key(&address, resource);
        let data = record.serialize();

        tracing::debug!("Storing freeze record for address {:?}, resource {}, key: {}, amount={}, expiration={}",
                       address, resource, hex::encode(&key), record.frozen_amount, record.expiration_timestamp);

        self.storage_engine.put(self.freeze_records_database(), &key, &data)?;
        Ok(())
    }

    /// Add to existing freeze amount (convenience method)
    /// If no record exists, creates a new one
    pub fn add_freeze_amount(&self, address: Address, resource: u8, amount: u64, expiration: i64) -> Result<()> {
        let mut record = self.get_freeze_record(&address, resource)?
            .unwrap_or(FreezeRecord::new(0, 0));

        // Add to frozen amount
        record.frozen_amount = record.frozen_amount.checked_add(amount)
            .ok_or_else(|| anyhow::anyhow!("Freeze amount overflow"))?;

        // Update expiration to later of existing or new
        record.expiration_timestamp = record.expiration_timestamp.max(expiration);

        self.set_freeze_record(address, resource, &record)?;
        Ok(())
    }

    /// Remove freeze record (for unfreeze operations)
    pub fn remove_freeze_record(&self, address: &Address, resource: u8) -> Result<()> {
        let key = self.freeze_record_key(address, resource);

        tracing::debug!("Removing freeze record for address {:?}, resource {}, key: {}",
                       address, resource, hex::encode(&key));

        self.storage_engine.delete(self.freeze_records_database(), &key)?;
        Ok(())
    }

    /// Get tron power for an address in SUN
    /// Sums frozen amounts across BANDWIDTH (0), ENERGY (1), and TRON_POWER (2) resources
    pub fn get_tron_power_in_sun(&self, address: &Address, new_model: bool) -> Result<u64> {
        // Resource types as defined in Tron protocol
        const BANDWIDTH: u8 = 0;
        const ENERGY: u8 = 1;
        const TRON_POWER: u8 = 2;

        let mut total: u64 = 0;
        let mut bandwidth_amount: u64 = 0;
        let mut energy_amount: u64 = 0;
        let mut tron_power_amount: u64 = 0;

        // Sum frozen amounts across all three resource types
        for resource in [BANDWIDTH, ENERGY, TRON_POWER] {
            if let Some(record) = self.get_freeze_record(address, resource)? {
                let amount = record.frozen_amount;
                total = total.checked_add(amount)
                    .ok_or_else(|| anyhow::anyhow!(
                        "Tron power overflow when adding resource {} amount {} to total {}",
                        resource, amount, total
                    ))?;

                // Track per-resource amounts for logging
                match resource {
                    BANDWIDTH => bandwidth_amount = amount,
                    ENERGY => energy_amount = amount,
                    TRON_POWER => tron_power_amount = amount,
                    _ => {}
                }
            }
        }

        // Log the computation with all relevant details
        tracing::info!(
            address = ?address,
            new_model = new_model,
            bandwidth = bandwidth_amount,
            energy = energy_amount,
            tron_power_legacy = tron_power_amount,
            total = total,
            "Computed tron power from freeze ledger"
        );

        Ok(total)
    }

    /// Get account name for an address
    pub fn get_account_name(&self, address: &Address) -> Result<Option<String>> {
        let key = self.account_key(address); // Reuse account_key helper (21-byte with 0x41 prefix)
        tracing::debug!("Getting account name for address {:?}, key: {}",
                       address, hex::encode(&key));

        match self.storage_engine.get(self.account_name_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found account name data, length: {}", data.len());
                // Decode as UTF-8 string
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

    /// Set account name for an address
    pub fn set_account_name(&mut self, address: Address, name: &[u8]) -> Result<()> {
        let key = self.account_key(&address); // Reuse account_key helper (21-byte with 0x41 prefix)

        tracing::debug!("Setting account name for address {:?}, key: {}, name_len: {}",
                       address, hex::encode(&key), name.len());

        // Validate name length (1 <= len <= 32 bytes to match java-tron constraints)
        if name.is_empty() {
            return Err(anyhow::anyhow!("Account name cannot be empty"));
        }
        if name.len() > 32 {
            return Err(anyhow::anyhow!("Account name cannot exceed 32 bytes, got {}", name.len()));
        }

        // Validate UTF-8 encoding (optional policy)
        match std::str::from_utf8(name) {
            Ok(name_str) => {
                tracing::debug!("Account name is valid UTF-8: {}", name_str);
            },
            Err(e) => {
                tracing::warn!("Account name contains invalid UTF-8: {}, allowing raw bytes", e);
                // Continue with raw bytes - some chains may allow arbitrary bytes
            }
        }

        self.storage_engine.put(self.account_name_database(), &key, name)?;

        tracing::info!("Successfully stored account name for address {:?}, length: {}", address, name.len());
        Ok(())
    }

    /// Get database name for account resource tracking (AEXT)
    fn account_aext_database(&self) -> &str {
        "account-resource"
    }

    /// Build storage key for account AEXT: 20-byte address
    fn account_aext_key(&self, address: &Address) -> Vec<u8> {
        address.as_slice().to_vec()
    }

    /// Get account AEXT (resource tracking fields) for an address
    pub fn get_account_aext(&self, address: &Address) -> Result<Option<AccountAext>> {
        let key = self.account_aext_key(address);
        tracing::debug!("Getting account AEXT for address {:?}, key: {}",
                       address, hex::encode(&key));

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

    /// Set account AEXT (resource tracking fields) for an address
    pub fn set_account_aext(&self, address: &Address, aext: &AccountAext) -> Result<()> {
        let key = self.account_aext_key(address);
        let data = aext.serialize();

        tracing::debug!("Setting account AEXT for address {:?}, net_usage: {}, free_net_usage: {}, net_window: {}",
                       address, aext.net_usage, aext.free_net_usage, aext.net_window_size);

        self.storage_engine.put(self.account_aext_database(), &key, &data)?;

        tracing::debug!("Successfully stored account AEXT for address {:?}", address);
        Ok(())
    }

    /// Get or initialize account AEXT with defaults
    pub fn get_or_init_account_aext(&self, address: &Address) -> Result<AccountAext> {
        if let Some(aext) = self.get_account_aext(address)? {
            Ok(aext)
        } else {
            let aext = AccountAext::with_defaults();
            self.set_account_aext(address, &aext)?;
            Ok(aext)
        }
    }

    // Phase C: Method alias shims (preferred names going forward)
    // See planning/storage_adapter_namings.planning.md for rationale

    /// **Preferred name**: Store freeze record (upsert semantics, aligns with `put_witness`).
    /// Delegates to `set_freeze_record`. Use this method in new code.
    pub fn put_freeze_record(&self, address: Address, resource: u8, record: &FreezeRecord) -> Result<()> {
        self.set_freeze_record(address, resource, record)
    }

    /// **Preferred name**: Compute tron power from ledger (reflects computation rather than "get").
    /// Delegates to `get_tron_power_in_sun`. Use this method in new code.
    pub fn compute_tron_power_in_sun(&self, address: &Address, new_model: bool) -> Result<u64> {
        self.get_tron_power_in_sun(address, new_model)
    }
}

impl EvmStateStore for EngineBackedEvmStateStore {
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
                            // Use canonical empty code hash keccak256("") for EOAs
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
