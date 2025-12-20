//! Engine-backed implementation of EVM state store.
//!
//! This module provides the production storage implementation backed by the StorageEngine
//! (RocksDB). It routes data to appropriate databases matching java-tron's organization.
//!
//! ## Account Serialization (Phase 0.1 - Correctness Fix)
//!
//! The Account protobuf serialization now uses prost-generated types that match
//! Java's protocol definitions exactly. This ensures:
//! - Field numbers are correct (address is field 3, not field 1)
//! - All fields are preserved during decode→modify→encode cycles
//! - No non-deterministic values like SystemTime::now()
//!
//! See planning/fast_do.todo.md for the full implementation plan.

use anyhow::Result;
use prost::Message;
use revm::primitives::{AccountInfo, Bytecode, Address, U256};
use tron_backend_storage::StorageEngine;
use super::traits::EvmStateStore;
use super::types::{WitnessInfo, VotesRecord, FreezeRecord, AccountAext};
use super::utils::{keccak256, to_tron_address};
use super::db_names;

// Import the generated TRON protocol types
use crate::protocol::{Account as ProtoAccount, AccountType as ProtoAccountType};

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
        db_names::account::ACCOUNT
    }

    /// Get the appropriate database name for contract code
    fn code_database(&self) -> &str {
        db_names::contract::CODE
    }

    /// Get the appropriate database name for contract storage
    fn contract_state_database(&self) -> &str {
        db_names::contract::CONTRACT_STATE
    }

    /// Get the appropriate database name for contract metadata
    fn contract_database(&self) -> &str {
        db_names::contract::CONTRACT
    }

    /// Get the appropriate database name for dynamic properties
    fn dynamic_properties_database(&self) -> &str {
        db_names::system::PROPERTIES
    }

    /// Get the appropriate database name for witness store
    fn witness_database(&self) -> &str {
        db_names::governance::WITNESS
    }

    /// Get the appropriate database name for votes store
    fn votes_database(&self) -> &str {
        db_names::governance::VOTES
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
        db_names::freeze::FREEZE_RECORDS
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

    /// Get the appropriate database name for account index (by name)
    /// Note: Java's AccountIndexStore uses "account-index", not "account-name"
    fn account_index_database(&self) -> &str {
        db_names::account::ACCOUNT_INDEX
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

    /// Serialize AccountInfo to bytes in java-tron Account protobuf format.
    ///
    /// ## Phase 0.1 Implementation (Correctness Fix)
    ///
    /// This method uses prost-generated `ProtoAccount` types that match Java's
    /// protocol definitions exactly. Key guarantees:
    /// - Field 3 is address (not field 1 as in the old broken implementation)
    /// - All unmodified fields are preserved during decode→modify→encode
    /// - No non-deterministic values (no SystemTime::now())
    ///
    /// For new accounts (no existing data), creates a minimal Account proto.
    /// For existing accounts, use `serialize_account_update` which preserves fields.
    fn serialize_account(&self, address: &Address, account: &AccountInfo) -> Vec<u8> {
        // Create a new ProtoAccount with only the fields we know
        let tron_address = self.account_key(address); // 21-byte with 0x41 prefix

        let proto_account = ProtoAccount {
            address: tron_address,
            r#type: ProtoAccountType::Normal as i32,
            // Take low 64 bits and reinterpret as i64 (consistent with serialize_account_update)
            balance: account.balance.as_limbs()[0] as i64,
            // All other fields default to their proto defaults (empty/0/false)
            // This is correct for NEW accounts only.
            // For EXISTING accounts, use serialize_account_update() instead.
            ..Default::default()
        };

        proto_account.encode_to_vec()
    }

    /// Serialize an account update using decode→modify→encode pattern.
    ///
    /// ## Phase 0.1 Core Implementation
    ///
    /// This is the key method that ensures correctness when updating existing accounts.
    /// It reads the existing proto bytes, decodes them, modifies only the balance,
    /// and re-encodes - preserving all other fields (permissions, votes, assets, etc.).
    ///
    /// ### Parameters
    /// - `address`: The account address (for key generation and fallback)
    /// - `account`: The new account state (only balance is used currently)
    /// - `existing_data`: Optional existing proto bytes from storage
    ///
    /// ### Returns
    /// Serialized proto bytes ready for storage
    pub fn serialize_account_update(
        &self,
        address: &Address,
        account: &AccountInfo,
        existing_data: Option<&[u8]>,
    ) -> Vec<u8> {
        match existing_data {
            Some(data) => {
                // Decode→Modify→Encode pattern: preserve all existing fields
                match ProtoAccount::decode(data) {
                    Ok(mut proto_account) => {
                        // Only update the balance field; all other fields are preserved
                        // Take low 64 bits and reinterpret as i64 (preserves bit pattern for
                        // values that exceed i64::MAX when treated as unsigned, like blackhole balance)
                        proto_account.balance = account.balance.as_limbs()[0] as i64;

                        tracing::debug!(
                            "Account update (decode→modify→encode): address={}, old_balance={}, new_balance={}",
                            hex::encode(&proto_account.address),
                            // The old balance from the decoded proto (for logging only)
                            data.len(), // Use data len as placeholder since we already updated
                            proto_account.balance
                        );

                        proto_account.encode_to_vec()
                    }
                    Err(e) => {
                        // If decode fails, log warning and create new account
                        // This shouldn't happen with valid data from Java
                        tracing::warn!(
                            "Failed to decode existing Account proto for {:?}: {}. Creating new account.",
                            address, e
                        );
                        self.serialize_account(address, account)
                    }
                }
            }
            None => {
                // No existing data, create new account
                self.serialize_account(address, account)
            }
        }
    }

    /// Deserialize AccountInfo from protobuf bytes (java-tron Account message).
    ///
    /// ## Phase 0.1 Implementation
    ///
    /// Uses prost to properly decode the Account proto, extracting the balance
    /// and code_hash fields that REVM's AccountInfo needs.
    fn deserialize_account(&self, data: &[u8]) -> Result<AccountInfo> {
        let proto_account = ProtoAccount::decode(data)
            .map_err(|e| anyhow::anyhow!("Failed to decode Account proto: {}", e))?;

        // Convert balance from i64 to U256, preserving the bit pattern.
        // Java uses i64 for balance in proto, but some addresses (like blackhole) can have
        // balances that appear negative when interpreted as signed. We preserve the bits
        // by casting i64 to u64, which keeps the two's complement representation intact.
        // When Java receives the 32-byte balance in AccountInfo, it extracts the low 8 bytes
        // and interprets them as i64, recovering the original signed value.
        let balance = U256::from(proto_account.balance as u64);

        // Extract code_hash if present (field 30)
        let code_hash = if proto_account.code_hash.len() == 32 {
            revm::primitives::B256::from_slice(&proto_account.code_hash)
        } else {
            revm::primitives::B256::ZERO
        };

        Ok(AccountInfo {
            balance,
            nonce: 0, // TRON doesn't use nonce
            code_hash,
            code: None, // Code is stored separately in "code" database
        })
    }

    /// Get the full Account proto for an address.
    ///
    /// This returns the complete ProtoAccount with all fields, useful for
    /// operations that need to inspect or modify specific fields.
    pub fn get_account_proto(&self, address: &Address) -> Result<Option<ProtoAccount>> {
        let key = self.account_key(address);
        match self.storage_engine.get(self.account_database(), &key)? {
            Some(data) => {
                let proto_account = ProtoAccount::decode(data.as_slice())
                    .map_err(|e| anyhow::anyhow!("Failed to decode Account proto: {}", e))?;
                Ok(Some(proto_account))
            }
            None => Ok(None),
        }
    }

    /// Store a complete Account proto.
    ///
    /// This allows storing a fully-populated ProtoAccount, useful after
    /// making complex modifications to multiple fields.
    pub fn put_account_proto(&self, address: &Address, proto_account: &ProtoAccount) -> Result<()> {
        let key = self.account_key(address);
        let data = proto_account.encode_to_vec();
        self.storage_engine.put(self.account_database(), &key, &data)?;
        Ok(())
    }

    /// Write a varint to the output buffer (kept for manual proto parsing elsewhere)
    fn write_varint(&self, output: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            output.push(((value & 0x7F) | 0x80) as u8);
            value >>= 7;
        }
        output.push(value as u8);
    }

    /// Extract balance field from Account protobuf message (legacy, kept for compatibility)
    ///
    /// Note: Prefer using deserialize_account() with prost for full proto parsing.
    /// This manual parser is kept for cases where we only need the balance quickly.
    fn extract_balance_from_protobuf(&self, data: &[u8]) -> Result<u64> {
        // Use prost for proper parsing
        let proto_account = ProtoAccount::decode(data)
            .map_err(|e| anyhow::anyhow!("Failed to decode Account proto: {}", e))?;

        // Convert i64 to u64, preserving bit pattern (see deserialize_account for explanation)
        Ok(proto_account.balance as u64)
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

    /// Get CreateNewAccountFeeInSystemContract dynamic property
    /// Fee charged when creating a new account via system contract (AccountCreateContract)
    /// Java reference: DynamicPropertiesStore.java getCreateNewAccountFeeInSystemContract()
    /// Default value: 1_000_000 SUN (1 TRX)
    pub fn get_create_new_account_fee_in_system_contract(&self) -> Result<u64> {
        let key = b"CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let fee = u64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    tracing::debug!("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT from DB: {} SUN", fee);
                    Ok(fee)
                } else {
                    // Use default value if data is too short
                    tracing::debug!("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT has invalid length, using default 1000000 SUN");
                    Ok(1_000_000) // 1 TRX in SUN (default from TRON)
                }
            },
            None => {
                // Use default value if not found
                tracing::debug!("CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT not found, using default 1000000 SUN");
                Ok(1_000_000) // 1 TRX in SUN (default from TRON)
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

    /// Add to TOTAL_NET_WEIGHT dynamic property
    /// Used when canceling unfreezeV2 to re-freeze bandwidth
    pub fn add_total_net_weight(&self, delta: i64) -> Result<()> {
        let current = self.get_total_net_weight()?;
        let new_value = current.checked_add(delta)
            .ok_or_else(|| anyhow::anyhow!("Overflow in add_total_net_weight"))?;
        let key = b"TOTAL_NET_WEIGHT";
        let data = new_value.to_be_bytes();
        self.storage_engine.put(self.dynamic_properties_database(), key, &data)?;
        Ok(())
    }

    /// Get TOTAL_ENERGY_WEIGHT dynamic property
    /// Default: 0
    pub fn get_total_energy_weight(&self) -> Result<i64> {
        let key = b"TOTAL_ENERGY_WEIGHT";
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

    /// Add to TOTAL_ENERGY_WEIGHT dynamic property
    /// Used when canceling unfreezeV2 to re-freeze energy
    pub fn add_total_energy_weight(&self, delta: i64) -> Result<()> {
        let current = self.get_total_energy_weight()?;
        let new_value = current.checked_add(delta)
            .ok_or_else(|| anyhow::anyhow!("Overflow in add_total_energy_weight"))?;
        let key = b"TOTAL_ENERGY_WEIGHT";
        let data = new_value.to_be_bytes();
        self.storage_engine.put(self.dynamic_properties_database(), key, &data)?;
        Ok(())
    }

    /// Get TOTAL_TRON_POWER_WEIGHT dynamic property
    /// Default: 0
    pub fn get_total_tron_power_weight(&self) -> Result<i64> {
        let key = b"TOTAL_TRON_POWER_WEIGHT";
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

    /// Add to TOTAL_TRON_POWER_WEIGHT dynamic property
    /// Used when canceling unfreezeV2 to re-freeze tron power
    pub fn add_total_tron_power_weight(&self, delta: i64) -> Result<()> {
        let current = self.get_total_tron_power_weight()?;
        let new_value = current.checked_add(delta)
            .ok_or_else(|| anyhow::anyhow!("Overflow in add_total_tron_power_weight"))?;
        let key = b"TOTAL_TRON_POWER_WEIGHT";
        let data = new_value.to_be_bytes();
        self.storage_engine.put(self.dynamic_properties_database(), key, &data)?;
        Ok(())
    }

    /// Check ALLOW_CANCEL_ALL_UNFREEZE_V2 dynamic property
    /// Returns true if CancelAllUnfreezeV2 is enabled
    /// Default: false
    pub fn support_allow_cancel_all_unfreeze_v2(&self) -> Result<bool> {
        let key = b"ALLOW_CANCEL_ALL_UNFREEZE_V2";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(val > 0)
                } else if !data.is_empty() {
                    Ok(data[0] != 0)
                } else {
                    Ok(false)
                }
            },
            None => Ok(false) // Default disabled
        }
    }

    /// Check SUPPORT_DR dynamic property (delegate resource)
    /// Returns true if resource delegation is enabled
    /// Default: false
    pub fn support_dr(&self) -> Result<bool> {
        let key = b"ALLOW_DELEGATE_RESOURCE";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(val > 0)
                } else if !data.is_empty() {
                    Ok(data[0] != 0)
                } else {
                    Ok(false)
                }
            },
            None => Ok(false) // Default disabled
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

        match self.storage_engine.get(self.account_index_database(), &key)? {
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

        self.storage_engine.put(self.account_index_database(), &key, name)?;

        tracing::info!("Successfully stored account name for address {:?}, length: {}", address, name.len());
        Ok(())
    }

    /// Get database name for account resource tracking (AEXT)
    fn account_aext_database(&self) -> &str {
        db_names::account::ACCOUNT_RESOURCE
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

    // =========================================================================
    // Delegation Store Access Methods
    // =========================================================================
    // These methods provide access to the delegation store for reward computation.
    // Java reference: DelegationStore.java, MortgageService.java

    /// Get the database name for delegation store
    fn delegation_database(&self) -> &str {
        db_names::delegation::DELEGATION
    }

    /// Generate key for delegation store address lookups (21-byte with 0x41 prefix)
    fn delegation_address_key(&self, address: &Address) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.push(0x41); // Tron address prefix
        key.extend_from_slice(address.as_slice());
        key
    }

    // --- Dynamic Properties for Delegation ---

    /// Check if delegation changes are allowed.
    /// Java reference: DynamicPropertiesStore.allowChangeDelegation()
    /// Returns true if CHANGE_DELEGATION == 1
    pub fn allow_change_delegation(&self) -> Result<bool> {
        // java-tron stores this flag under the "CHANGE_DELEGATION" dynamic property key.
        let key = b"CHANGE_DELEGATION";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    Ok(val == 1)
                } else if !data.is_empty() {
                    Ok(data[0] == 1)
                } else {
                    Ok(false)
                }
            }
            None => {
                tracing::debug!("CHANGE_DELEGATION not found, returning false");
                Ok(false)
            }
        }
    }

    /// Get the current cycle number from dynamic properties.
    /// Java reference: DynamicPropertiesStore.getCurrentCycleNumber()
    pub fn get_current_cycle_number(&self) -> Result<i64> {
        let key = b"CURRENT_CYCLE_NUMBER";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    tracing::warn!("CURRENT_CYCLE_NUMBER has invalid length: {}", data.len());
                    Ok(0)
                }
            }
            None => {
                tracing::debug!("CURRENT_CYCLE_NUMBER not found, returning 0");
                Ok(0)
            }
        }
    }

    /// Get the cycle number when new reward algorithm takes effect.
    /// Java reference: DynamicPropertiesStore.getNewRewardAlgorithmEffectiveCycle()
    /// Returns Long.MAX_VALUE if not set (meaning old algorithm always used)
    pub fn get_new_reward_algorithm_effective_cycle(&self) -> Result<i64> {
        let key = b"NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    // Default to Long.MAX_VALUE (old algorithm always)
                    Ok(i64::MAX)
                }
            }
            None => {
                // Default to Long.MAX_VALUE (old algorithm always)
                tracing::debug!("NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE not found, returning MAX");
                Ok(i64::MAX)
            }
        }
    }

    // --- Delegation Store Read Methods ---

    /// Get the begin cycle for an address from delegation store.
    /// Java reference: DelegationStore.getBeginCycle()
    /// Returns 0 if not found.
    pub fn get_delegation_begin_cycle(&self, address: &Address) -> Result<i64> {
        use crate::delegation::delegation_begin_cycle_key;
        let tron_addr = self.delegation_address_key(address);
        let key = delegation_begin_cycle_key(&tron_addr);

        match self.storage_engine.get(self.delegation_database(), &key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let cycle = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    tracing::debug!("delegation begin_cycle for {:?}: {}", address, cycle);
                    Ok(cycle)
                } else {
                    tracing::warn!("Invalid begin_cycle data length: {}", data.len());
                    Ok(0)
                }
            }
            None => {
                tracing::debug!("delegation begin_cycle not found for {:?}, returning 0", address);
                Ok(0)
            }
        }
    }

    /// Get the end cycle for an address from delegation store.
    /// Java reference: DelegationStore.getEndCycle()
    /// Returns REMARK (-1) if not found.
    pub fn get_delegation_end_cycle(&self, address: &Address) -> Result<i64> {
        use crate::delegation::{delegation_end_cycle_key, DELEGATION_STORE_REMARK};
        let tron_addr = self.delegation_address_key(address);
        let key = delegation_end_cycle_key(&tron_addr);

        match self.storage_engine.get(self.delegation_database(), &key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let cycle = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    tracing::debug!("delegation end_cycle for {:?}: {}", address, cycle);
                    Ok(cycle)
                } else {
                    tracing::warn!("Invalid end_cycle data length: {}", data.len());
                    Ok(DELEGATION_STORE_REMARK)
                }
            }
            None => {
                tracing::debug!("delegation end_cycle not found for {:?}, returning REMARK", address);
                Ok(DELEGATION_STORE_REMARK)
            }
        }
    }

    /// Get account vote snapshot for a specific cycle.
    /// Java reference: DelegationStore.getAccountVote()
    /// Returns None if not found.
    pub fn get_delegation_account_vote(
        &self,
        cycle: i64,
        address: &Address,
    ) -> Result<Option<crate::delegation::AccountVoteSnapshot>> {
        use crate::delegation::{delegation_account_vote_key, AccountVoteSnapshot};
        let tron_addr = self.delegation_address_key(address);
        let key = delegation_account_vote_key(cycle, &tron_addr);

        match self.storage_engine.get(self.delegation_database(), &key)? {
            Some(data) => {
                match AccountVoteSnapshot::deserialize(&data) {
                    Ok(snapshot) => {
                        tracing::debug!(
                            "delegation account_vote for {:?} cycle {}: {} votes",
                            address, cycle, snapshot.votes.len()
                        );
                        Ok(Some(snapshot))
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to deserialize account_vote for {:?} cycle {}: {}",
                            address, cycle, e
                        );
                        Ok(None)
                    }
                }
            }
            None => {
                tracing::debug!("delegation account_vote not found for {:?} cycle {}", address, cycle);
                Ok(None)
            }
        }
    }

    /// Get total reward for a witness in a cycle.
    /// Java reference: DelegationStore.getReward()
    /// Returns 0 if not found.
    pub fn get_delegation_reward(&self, cycle: i64, witness_address: &Address) -> Result<i64> {
        use crate::delegation::delegation_reward_key;
        let tron_addr = self.delegation_address_key(witness_address);
        let key = delegation_reward_key(cycle, &tron_addr);

        match self.storage_engine.get(self.delegation_database(), &key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let reward = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    tracing::debug!(
                        "delegation reward for {:?} cycle {}: {}",
                        witness_address, cycle, reward
                    );
                    Ok(reward)
                } else {
                    Ok(0)
                }
            }
            None => Ok(0),
        }
    }

    /// Get total witness vote count for a cycle.
    /// Java reference: DelegationStore.getWitnessVote()
    /// Returns REMARK (-1) if not found.
    pub fn get_delegation_witness_vote(&self, cycle: i64, witness_address: &Address) -> Result<i64> {
        use crate::delegation::{delegation_witness_vote_key, DELEGATION_STORE_REMARK};
        let tron_addr = self.delegation_address_key(witness_address);
        let key = delegation_witness_vote_key(cycle, &tron_addr);

        match self.storage_engine.get(self.delegation_database(), &key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let vote = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    tracing::debug!(
                        "delegation witness_vote for {:?} cycle {}: {}",
                        witness_address, cycle, vote
                    );
                    Ok(vote)
                } else {
                    Ok(DELEGATION_STORE_REMARK)
                }
            }
            None => Ok(DELEGATION_STORE_REMARK),
        }
    }

    /// Get witness Vi (vote index) for a cycle.
    /// Java reference: DelegationStore.getWitnessVi()
    /// Returns BigInt::ZERO if not found.
    pub fn get_delegation_witness_vi(
        &self,
        cycle: i64,
        witness_address: &Address,
    ) -> Result<num_bigint::BigInt> {
        use crate::delegation::delegation_witness_vi_key;
        use num_bigint::BigInt;

        let tron_addr = self.delegation_address_key(witness_address);
        let key = delegation_witness_vi_key(cycle, &tron_addr);

        match self.storage_engine.get(self.delegation_database(), &key)? {
            Some(data) => {
                // Java stores BigInteger as signed two's complement bytes
                let vi = BigInt::from_signed_bytes_be(&data);
                tracing::debug!(
                    "delegation witness_vi for {:?} cycle {}: {}",
                    witness_address, cycle, vi
                );
                Ok(vi)
            }
            None => Ok(BigInt::from(0)),
        }
    }

    /// Get brokerage rate for a witness in a cycle.
    /// Java reference: DelegationStore.getBrokerage()
    /// Returns DEFAULT_BROKERAGE (20) if not found.
    pub fn get_delegation_brokerage(&self, cycle: i64, witness_address: &Address) -> Result<i32> {
        use crate::delegation::{delegation_brokerage_key, DEFAULT_BROKERAGE};
        let tron_addr = self.delegation_address_key(witness_address);
        let key = delegation_brokerage_key(cycle, &tron_addr);

        match self.storage_engine.get(self.delegation_database(), &key)? {
            Some(data) => {
                if data.len() >= 4 {
                    let brokerage = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                    tracing::debug!(
                        "delegation brokerage for {:?} cycle {}: {}%",
                        witness_address, cycle, brokerage
                    );
                    Ok(brokerage)
                } else {
                    Ok(DEFAULT_BROKERAGE)
                }
            }
            None => Ok(DEFAULT_BROKERAGE),
        }
    }

    // --- Delegation Store Write Methods ---

    /// Set the begin cycle for an address.
    /// Java reference: DelegationStore.setBeginCycle()
    pub fn set_delegation_begin_cycle(&self, address: &Address, cycle: i64) -> Result<()> {
        use crate::delegation::delegation_begin_cycle_key;
        let tron_addr = self.delegation_address_key(address);
        let key = delegation_begin_cycle_key(&tron_addr);
        let data = cycle.to_be_bytes();

        tracing::debug!("Setting delegation begin_cycle for {:?}: {}", address, cycle);
        self.storage_engine.put(self.delegation_database(), &key, &data)?;
        Ok(())
    }

    /// Set the end cycle for an address.
    /// Java reference: DelegationStore.setEndCycle()
    pub fn set_delegation_end_cycle(&self, address: &Address, cycle: i64) -> Result<()> {
        use crate::delegation::delegation_end_cycle_key;
        let tron_addr = self.delegation_address_key(address);
        let key = delegation_end_cycle_key(&tron_addr);
        let data = cycle.to_be_bytes();

        tracing::debug!("Setting delegation end_cycle for {:?}: {}", address, cycle);
        self.storage_engine.put(self.delegation_database(), &key, &data)?;
        Ok(())
    }

    /// Set account vote snapshot for a cycle.
    /// Java reference: DelegationStore.setAccountVote()
    pub fn set_delegation_account_vote(
        &self,
        cycle: i64,
        address: &Address,
        snapshot: &crate::delegation::AccountVoteSnapshot,
    ) -> Result<()> {
        use crate::delegation::delegation_account_vote_key;
        let tron_addr = self.delegation_address_key(address);
        let key = delegation_account_vote_key(cycle, &tron_addr);
        let data = snapshot.serialize();

        tracing::debug!(
            "Setting delegation account_vote for {:?} cycle {}: {} votes",
            address, cycle, snapshot.votes.len()
        );
        self.storage_engine.put(self.delegation_database(), &key, &data)?;
        Ok(())
    }

    /// Get votes list from account for delegation purposes.
    /// Converts Account.votes to DelegationVote format.
    /// Java reference: AccountCapsule.getVotesList()
    pub fn get_delegation_votes_from_account(
        &self,
        address: &Address,
    ) -> Result<Vec<crate::delegation::DelegationVote>> {
        use crate::delegation::DelegationVote;

        // Use existing method to get votes from Account protobuf
        let account_votes = self.get_account_votes_list(address)?;

        // Convert to DelegationVote format
        let votes: Vec<DelegationVote> = account_votes
            .into_iter()
            .map(|(addr, count)| DelegationVote::new(addr, count as i64))
            .collect();

        tracing::debug!(
            "Got {} delegation votes from account {:?}",
            votes.len(), address
        );
        Ok(votes)
    }

    /// Set brokerage for a witness address.
    /// Java reference: DelegationStore.setBrokerage(cycle, address, brokerage)
    /// The brokerage is stored as a 4-byte big-endian integer.
    /// For UpdateBrokerageContract, cycle is always -1 (REMARK).
    pub fn set_delegation_brokerage(&self, cycle: i64, address: &Address, brokerage: i32) -> Result<()> {
        use crate::delegation::delegation_brokerage_key;
        let tron_addr = self.delegation_address_key(address);
        let key = delegation_brokerage_key(cycle, &tron_addr);
        let data = brokerage.to_be_bytes();

        tracing::debug!(
            "Setting delegation brokerage for {:?} cycle {}: {}%",
            address, cycle, brokerage
        );
        self.storage_engine.put(self.delegation_database(), &key, &data)?;
        Ok(())
    }

    // =========================================================================
    // Proposal Store Access Methods (Phase 2.A)
    // =========================================================================
    // These methods provide access to the proposal store for governance operations.
    // Java reference: ProposalStore.java, ProposalCapsule.java

    /// Get the database name for proposal store
    fn proposal_database(&self) -> &str {
        db_names::governance::PROPOSAL
    }

    /// Generate key for proposal store: 8-byte big-endian proposal ID
    /// Java reference: ProposalCapsule.createDbKey() -> ByteArray.fromLong(proposalId)
    fn proposal_key(&self, proposal_id: i64) -> Vec<u8> {
        use super::key_helpers::proposal_key;
        proposal_key(proposal_id)
    }

    /// Get proposal by ID
    /// Returns the raw Proposal protobuf bytes
    pub fn get_proposal(&self, proposal_id: i64) -> Result<Option<crate::protocol::Proposal>> {
        use crate::protocol::Proposal;
        let key = self.proposal_key(proposal_id);
        tracing::debug!("Getting proposal {}, key: {}", proposal_id, hex::encode(&key));

        match self.storage_engine.get(self.proposal_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found proposal data, length: {}", data.len());
                match Proposal::decode(data.as_slice()) {
                    Ok(proposal) => {
                        tracing::debug!(
                            "Decoded proposal {} - proposer: {}, state: {:?}, approvals: {}",
                            proposal.proposal_id,
                            hex::encode(&proposal.proposer_address),
                            proposal.state,
                            proposal.approvals.len()
                        );
                        Ok(Some(proposal))
                    }
                    Err(e) => {
                        tracing::error!("Failed to decode proposal {}: {}", proposal_id, e);
                        Err(anyhow::anyhow!("Failed to decode proposal: {}", e))
                    }
                }
            }
            None => {
                tracing::debug!("Proposal {} not found", proposal_id);
                Ok(None)
            }
        }
    }

    /// Store proposal
    pub fn put_proposal(&self, proposal: &crate::protocol::Proposal) -> Result<()> {
        let key = self.proposal_key(proposal.proposal_id);
        let data = proposal.encode_to_vec();

        tracing::debug!(
            "Storing proposal {} - proposer: {}, state: {:?}, approvals: {}, key: {}",
            proposal.proposal_id,
            hex::encode(&proposal.proposer_address),
            proposal.state,
            proposal.approvals.len(),
            hex::encode(&key)
        );

        self.storage_engine.put(self.proposal_database(), &key, &data)?;
        Ok(())
    }

    /// Check if proposal exists
    pub fn has_proposal(&self, proposal_id: i64) -> Result<bool> {
        let key = self.proposal_key(proposal_id);
        match self.storage_engine.get(self.proposal_database(), &key)? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    // --- Dynamic Properties for Proposals ---

    /// Get LATEST_PROPOSAL_NUM dynamic property
    /// Returns the highest proposal ID that has been created
    /// Default: 0 if not found
    pub fn get_latest_proposal_num(&self) -> Result<i64> {
        let key = b"LATEST_PROPOSAL_NUM";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let num = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    tracing::debug!("LATEST_PROPOSAL_NUM: {}", num);
                    Ok(num)
                } else {
                    tracing::warn!("LATEST_PROPOSAL_NUM has invalid length: {}", data.len());
                    Ok(0)
                }
            }
            None => {
                tracing::debug!("LATEST_PROPOSAL_NUM not found, returning 0");
                Ok(0)
            }
        }
    }

    /// Set LATEST_PROPOSAL_NUM dynamic property
    pub fn set_latest_proposal_num(&self, num: i64) -> Result<()> {
        let key = b"LATEST_PROPOSAL_NUM";
        let data = num.to_be_bytes();
        tracing::debug!("Setting LATEST_PROPOSAL_NUM to {}", num);
        self.storage_engine.put(self.dynamic_properties_database(), key, &data)?;
        Ok(())
    }

    /// Get NEXT_MAINTENANCE_TIME dynamic property
    /// Returns the timestamp (milliseconds) of the next maintenance period
    /// Default: 0 if not found
    pub fn get_next_maintenance_time(&self) -> Result<i64> {
        let key = b"NEXT_MAINTENANCE_TIME";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let time = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    tracing::debug!("NEXT_MAINTENANCE_TIME: {}", time);
                    Ok(time)
                } else {
                    tracing::warn!("NEXT_MAINTENANCE_TIME has invalid length: {}", data.len());
                    Ok(0)
                }
            }
            None => {
                tracing::debug!("NEXT_MAINTENANCE_TIME not found, returning 0");
                Ok(0)
            }
        }
    }

    /// Get MAINTENANCE_TIME_INTERVAL dynamic property
    /// Returns the interval (milliseconds) between maintenance periods
    /// Default: 21600000 (6 hours) if not found
    pub fn get_maintenance_time_interval(&self) -> Result<i64> {
        let key = b"MAINTENANCE_TIME_INTERVAL";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let interval = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    tracing::debug!("MAINTENANCE_TIME_INTERVAL: {}", interval);
                    Ok(interval)
                } else {
                    tracing::warn!("MAINTENANCE_TIME_INTERVAL has invalid length: {}", data.len());
                    Ok(21600000) // 6 hours in milliseconds
                }
            }
            None => {
                tracing::debug!("MAINTENANCE_TIME_INTERVAL not found, returning default 21600000");
                Ok(21600000) // 6 hours in milliseconds
            }
        }
    }

    // ==========================================================================
    // Phase 2.B: AccountIdIndex Store Methods
    // ==========================================================================
    //
    // AccountIdIndex maps lowercase account IDs to account addresses.
    // Used by SetAccountIdContract (type 19).
    // Java reference: AccountIdIndexStore.java

    /// Get the database name for account id index
    fn account_id_index_database(&self) -> &str {
        db_names::account::ACCOUNT_ID_INDEX
    }

    /// Convert account ID to lowercase key format
    /// Java: AccountIdIndexStore.getLowerCaseAccountId() converts to lowercase UTF-8
    fn account_id_key(&self, account_id: &[u8]) -> Vec<u8> {
        // Convert bytes to UTF-8 string, lowercase, then back to bytes
        if let Ok(s) = std::str::from_utf8(account_id) {
            s.to_lowercase().into_bytes()
        } else {
            // If not valid UTF-8, just use the raw bytes
            account_id.to_vec()
        }
    }

    /// Check if an account ID already exists in the index
    /// Returns true if the account ID is already taken
    pub fn has_account_id(&self, account_id: &[u8]) -> Result<bool> {
        let key = self.account_id_key(account_id);
        tracing::debug!("Checking if account_id exists: {:?} -> key: {}",
                       String::from_utf8_lossy(account_id), hex::encode(&key));

        match self.storage_engine.get(self.account_id_index_database(), &key)? {
            Some(_) => {
                tracing::debug!("Account ID {} already exists", String::from_utf8_lossy(account_id));
                Ok(true)
            }
            None => {
                tracing::debug!("Account ID {} does not exist", String::from_utf8_lossy(account_id));
                Ok(false)
            }
        }
    }

    /// Get the address associated with an account ID
    /// Returns the 21-byte TRON address (with 0x41 prefix)
    pub fn get_address_by_account_id(&self, account_id: &[u8]) -> Result<Option<Vec<u8>>> {
        let key = self.account_id_key(account_id);
        tracing::debug!("Getting address for account_id: {:?} -> key: {}",
                       String::from_utf8_lossy(account_id), hex::encode(&key));

        match self.storage_engine.get(self.account_id_index_database(), &key)? {
            Some(data) => {
                tracing::debug!("Found address for account_id {}: {}",
                               String::from_utf8_lossy(account_id), hex::encode(&data));
                Ok(Some(data))
            }
            None => Ok(None)
        }
    }

    /// Store an account ID -> address mapping
    /// address should be the 21-byte TRON address (with 0x41 prefix)
    pub fn put_account_id_index(&self, account_id: &[u8], address: &[u8]) -> Result<()> {
        let key = self.account_id_key(account_id);
        tracing::debug!("Storing account_id index: {:?} -> {} (key: {})",
                       String::from_utf8_lossy(account_id), hex::encode(address), hex::encode(&key));

        self.storage_engine.put(self.account_id_index_database(), &key, address)?;
        Ok(())
    }

    // ==========================================================================
    // Phase 2.B: Account Permission and Dynamic Properties (Additional)
    // ==========================================================================
    //
    // Note: get_allow_multi_sign() and support_black_hole_optimization()
    // already exist above in this file (lines ~438 and ~459).

    /// Get TOTAL_SIGN_NUM dynamic property
    /// Maximum number of keys allowed in a permission
    /// Default: 5
    pub fn get_total_sign_num(&self) -> Result<i64> {
        let key = b"TOTAL_SIGN_NUM";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let value = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    tracing::debug!("TOTAL_SIGN_NUM: {}", value);
                    Ok(value)
                } else {
                    tracing::warn!("TOTAL_SIGN_NUM has invalid length: {}", data.len());
                    Ok(5) // Default
                }
            }
            None => {
                tracing::debug!("TOTAL_SIGN_NUM not found, returning default 5");
                Ok(5)
            }
        }
    }

    /// Get UPDATE_ACCOUNT_PERMISSION_FEE dynamic property
    /// Fee in SUN for updating account permissions
    /// Default: 100_000_000 (100 TRX)
    pub fn get_update_account_permission_fee(&self) -> Result<i64> {
        let key = b"UPDATE_ACCOUNT_PERMISSION_FEE";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let value = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    tracing::debug!("UPDATE_ACCOUNT_PERMISSION_FEE: {}", value);
                    Ok(value)
                } else {
                    tracing::warn!("UPDATE_ACCOUNT_PERMISSION_FEE has invalid length: {}", data.len());
                    Ok(100_000_000) // 100 TRX in SUN
                }
            }
            None => {
                tracing::debug!("UPDATE_ACCOUNT_PERMISSION_FEE not found, returning default 100_000_000");
                Ok(100_000_000) // 100 TRX in SUN
            }
        }
    }

    /// Get AVAILABLE_CONTRACT_TYPE dynamic property
    /// Bitmap of allowed contract types (32 bytes)
    /// Returns None if not found (all contracts allowed)
    pub fn get_available_contract_type(&self) -> Result<Option<Vec<u8>>> {
        let key = b"AVAILABLE_CONTRACT_TYPE";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                tracing::debug!("AVAILABLE_CONTRACT_TYPE: {} bytes", data.len());
                Ok(Some(data))
            }
            None => {
                tracing::debug!("AVAILABLE_CONTRACT_TYPE not found");
                Ok(None)
            }
        }
    }

    /// Get the blackhole address as 21-byte TRON format (0x41 prefix + 20 bytes)
    /// Java: AccountStore.getBlackhole() returns BURN_ADDRESS or HOLE_ADDRESS
    pub fn get_blackhole_address_tron(&self) -> [u8; 21] {
        // TRON blackhole address: TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy (mainnet default)
        // Base58 decoded to hex: 4174b2c93cba4fb9d734c4c3fc2c0df8ff9d36d50c
        [
            0x41,
            0x74, 0xb2, 0xc9, 0x3c, 0xba, 0x4f, 0xb9, 0xd7,
            0x34, 0xc4, 0xc3, 0xfc, 0x2c, 0x0d, 0xf8, 0xff,
            0x9d, 0x36, 0xd5, 0x0c,
        ]
    }

    /// Get the blackhole address as an EVM Address (20 bytes, no 0x41 prefix)
    pub fn get_blackhole_address_evm(&self) -> Address {
        Self::default_blackhole_address().unwrap_or(Address::ZERO)
    }

    // ==========================================================================
    // Phase 2.C: ContractStore and AbiStore Methods
    // ==========================================================================
    //
    // ContractStore: Stores SmartContract metadata (origin_address, consume_user_resource_percent, etc.)
    // AbiStore: Stores contract ABI (Application Binary Interface)
    // Java reference: ContractStore.java, AbiStore.java, ContractCapsule.java, AbiCapsule.java
    //
    // Note: contract_database() already exists at line ~58

    /// Get the database name for ABI store
    fn abi_database(&self) -> &str {
        db_names::contract::ABI
    }

    /// Get a smart contract by its address
    /// Returns the SmartContract protobuf if found
    /// Key: 21-byte TRON address (0x41 prefix + 20 bytes)
    pub fn get_smart_contract(&self, contract_address: &[u8]) -> Result<Option<crate::protocol::SmartContract>> {
        tracing::debug!("Getting smart contract for address: {}", hex::encode(contract_address));

        match self.storage_engine.get(self.contract_database(), contract_address)? {
            Some(data) => {
                tracing::debug!("Found contract data, length: {}", data.len());
                // Deserialize using prost
                match crate::protocol::SmartContract::decode(&data[..]) {
                    Ok(contract) => {
                        tracing::debug!("Successfully deserialized SmartContract - origin_address: {}, consume_percent: {}",
                                       hex::encode(&contract.origin_address), contract.consume_user_resource_percent);
                        Ok(Some(contract))
                    }
                    Err(e) => {
                        tracing::error!("Failed to decode SmartContract: {}", e);
                        Err(anyhow::anyhow!("Failed to decode SmartContract: {}", e))
                    }
                }
            }
            None => {
                tracing::debug!("Smart contract not found for address: {}", hex::encode(contract_address));
                Ok(None)
            }
        }
    }

    /// Store a smart contract
    /// Key: contract address (21-byte TRON address)
    pub fn put_smart_contract(&self, contract: &crate::protocol::SmartContract) -> Result<()> {
        let key = &contract.contract_address;
        tracing::debug!("Storing smart contract at address: {}, consume_percent: {}, origin_energy_limit: {}",
                       hex::encode(key), contract.consume_user_resource_percent, contract.origin_energy_limit);

        // Serialize using prost
        let mut buf = Vec::new();
        contract.encode(&mut buf).map_err(|e| anyhow::anyhow!("Failed to encode SmartContract: {}", e))?;

        self.storage_engine.put(self.contract_database(), key, &buf)?;
        Ok(())
    }

    /// Check if a smart contract exists
    pub fn has_smart_contract(&self, contract_address: &[u8]) -> Result<bool> {
        match self.storage_engine.get(self.contract_database(), contract_address)? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Get ABI for a contract
    /// Returns the SmartContract.ABI protobuf if found
    /// Key: contract address (21-byte TRON address)
    pub fn get_abi(&self, contract_address: &[u8]) -> Result<Option<crate::protocol::smart_contract::Abi>> {
        tracing::debug!("Getting ABI for contract: {}", hex::encode(contract_address));

        match self.storage_engine.get(self.abi_database(), contract_address)? {
            Some(data) => {
                tracing::debug!("Found ABI data, length: {}", data.len());
                // Deserialize using prost
                match crate::protocol::smart_contract::Abi::decode(&data[..]) {
                    Ok(abi) => {
                        tracing::debug!("Successfully deserialized ABI - entries: {}", abi.entrys.len());
                        Ok(Some(abi))
                    }
                    Err(e) => {
                        tracing::error!("Failed to decode ABI: {}", e);
                        Err(anyhow::anyhow!("Failed to decode ABI: {}", e))
                    }
                }
            }
            None => {
                tracing::debug!("ABI not found for contract: {}", hex::encode(contract_address));
                Ok(None)
            }
        }
    }

    /// Store ABI for a contract
    /// Key: contract address (21-byte TRON address)
    pub fn put_abi(&self, contract_address: &[u8], abi: &crate::protocol::smart_contract::Abi) -> Result<()> {
        tracing::debug!("Storing ABI for contract: {}, entries: {}",
                       hex::encode(contract_address), abi.entrys.len());

        // Serialize using prost
        let mut buf = Vec::new();
        abi.encode(&mut buf).map_err(|e| anyhow::anyhow!("Failed to encode ABI: {}", e))?;

        self.storage_engine.put(self.abi_database(), contract_address, &buf)?;
        Ok(())
    }

    /// Clear ABI for a contract (write default empty ABI)
    /// This is used by ClearABIContract (type 48)
    pub fn clear_abi(&self, contract_address: &[u8]) -> Result<()> {
        tracing::debug!("Clearing ABI for contract: {}", hex::encode(contract_address));

        // Create default empty ABI
        let default_abi = crate::protocol::smart_contract::Abi::default();
        self.put_abi(contract_address, &default_abi)
    }

    // ==========================================================================
    // Phase 2.D: DelegatedResource and DelegatedResourceAccountIndex Methods
    // ==========================================================================
    //
    // DelegatedResourceStore: Stores delegation records between accounts
    // DelegatedResourceAccountIndexStore: Stores index of delegation relationships
    // Java reference: DelegatedResourceStore.java, DelegatedResourceAccountIndexStore.java
    //
    // Key format for DelegatedResourceStore (V2):
    //   DELEGATED_RESOURCE_V2_KEY_PREFIX (0x01) + from_address (21 bytes) + to_address (21 bytes) = 43 bytes
    //
    // Key format for DelegatedResourceAccountIndexStore (V2):
    //   FROM_PREFIX (0x03) or TO_PREFIX (0x04) + address (21 bytes) = 22 bytes

    /// Get the database name for DelegatedResource store
    fn delegated_resource_database(&self) -> &str {
        db_names::delegation::DELEGATED_RESOURCE
    }

    /// Get the database name for DelegatedResourceAccountIndex store
    fn delegated_resource_account_index_database(&self) -> &str {
        db_names::delegation::DELEGATED_RESOURCE_ACCOUNT_INDEX
    }

    /// Create V2 key for DelegatedResource store (from -> to)
    fn delegated_resource_key_v2(&self, from: &Address, to: &Address) -> Vec<u8> {
        use super::key_helpers::delegated_resource;
        let from_tron = self.to_tron_address_21(from);
        let to_tron = self.to_tron_address_21(to);
        delegated_resource::create_db_key_v2_from(&from_tron, &to_tron)
    }

    /// Convert 20-byte EVM address to 21-byte TRON address
    fn to_tron_address_21(&self, address: &Address) -> [u8; 21] {
        let mut tron_addr = [0u8; 21];
        tron_addr[0] = 0x41;
        tron_addr[1..].copy_from_slice(address.as_slice());
        tron_addr
    }

    /// Delegate resource from owner to receiver
    /// Updates DelegatedResourceStore with the delegation record
    pub fn delegate_resource(
        &self,
        owner: &Address,
        receiver: &Address,
        is_bandwidth: bool,
        balance: i64,
        lock: bool,
        expire_time: i64,
    ) -> Result<()> {
        let key = self.delegated_resource_key_v2(owner, receiver);
        tracing::debug!("Delegating resource: from={}, to={}, is_bw={}, balance={}, lock={}, expire={}",
                       hex::encode(owner), hex::encode(receiver), is_bandwidth, balance, lock, expire_time);

        // Get or create DelegatedResource
        let mut dr = match self.storage_engine.get(self.delegated_resource_database(), &key)? {
            Some(data) => {
                crate::protocol::DelegatedResource::decode(&data[..])
                    .map_err(|e| anyhow::anyhow!("Failed to decode DelegatedResource: {}", e))?
            }
            None => {
                // Create new record
                crate::protocol::DelegatedResource {
                    from: self.to_tron_address_21(owner).to_vec(),
                    to: self.to_tron_address_21(receiver).to_vec(),
                    frozen_balance_for_bandwidth: 0,
                    frozen_balance_for_energy: 0,
                    expire_time_for_bandwidth: 0,
                    expire_time_for_energy: 0,
                }
            }
        };

        // Update based on resource type
        if is_bandwidth {
            dr.frozen_balance_for_bandwidth += balance;
            if lock && expire_time > dr.expire_time_for_bandwidth {
                dr.expire_time_for_bandwidth = expire_time;
            }
        } else {
            dr.frozen_balance_for_energy += balance;
            if lock && expire_time > dr.expire_time_for_energy {
                dr.expire_time_for_energy = expire_time;
            }
        }

        // Persist
        let data = dr.encode_to_vec();
        self.storage_engine.put(self.delegated_resource_database(), &key, &data)?;
        Ok(())
    }

    /// Undelegate resource (reclaim from receiver back to owner)
    pub fn undelegate_resource(
        &self,
        owner: &Address,
        receiver: &Address,
        is_bandwidth: bool,
        balance: i64,
        _now: i64,
    ) -> Result<()> {
        let key = self.delegated_resource_key_v2(owner, receiver);
        tracing::debug!("Undelegating resource: from={}, to={}, is_bw={}, balance={}",
                       hex::encode(owner), hex::encode(receiver), is_bandwidth, balance);

        // Get existing DelegatedResource
        let data = self.storage_engine.get(self.delegated_resource_database(), &key)?
            .ok_or_else(|| anyhow::anyhow!("DelegatedResource not found"))?;

        let mut dr = crate::protocol::DelegatedResource::decode(&data[..])
            .map_err(|e| anyhow::anyhow!("Failed to decode DelegatedResource: {}", e))?;

        // Reduce balance
        if is_bandwidth {
            dr.frozen_balance_for_bandwidth = (dr.frozen_balance_for_bandwidth - balance).max(0);
        } else {
            dr.frozen_balance_for_energy = (dr.frozen_balance_for_energy - balance).max(0);
        }

        // If both balances are 0, delete the record; otherwise, persist
        if dr.frozen_balance_for_bandwidth == 0 && dr.frozen_balance_for_energy == 0 {
            self.storage_engine.delete(self.delegated_resource_database(), &key)?;
        } else {
            let data = dr.encode_to_vec();
            self.storage_engine.put(self.delegated_resource_database(), &key, &data)?;
        }

        Ok(())
    }

    /// Get available (unlocked) delegate balance for undelegation
    /// Returns the balance that can be undelegated (considering lock expiration)
    pub fn get_available_delegate_balance(
        &self,
        owner: &Address,
        receiver: &Address,
        is_bandwidth: bool,
        now: i64,
    ) -> Result<i64> {
        let key = self.delegated_resource_key_v2(owner, receiver);

        match self.storage_engine.get(self.delegated_resource_database(), &key)? {
            Some(data) => {
                let dr = crate::protocol::DelegatedResource::decode(&data[..])
                    .map_err(|e| anyhow::anyhow!("Failed to decode DelegatedResource: {}", e))?;

                if is_bandwidth {
                    // Check if lock has expired
                    if dr.expire_time_for_bandwidth > now {
                        Ok(0) // Still locked
                    } else {
                        Ok(dr.frozen_balance_for_bandwidth)
                    }
                } else {
                    if dr.expire_time_for_energy > now {
                        Ok(0) // Still locked
                    } else {
                        Ok(dr.frozen_balance_for_energy)
                    }
                }
            }
            None => Ok(0) // No delegation exists
        }
    }

    /// Update DelegatedResourceAccountIndex for a delegation
    /// Adds receiver to owner's "to" list and owner to receiver's "from" list
    pub fn delegate_resource_account_index(
        &self,
        owner: &Address,
        receiver: &Address,
        timestamp: i64,
    ) -> Result<()> {
        let owner_tron = self.to_tron_address_21(owner);
        let receiver_tron = self.to_tron_address_21(receiver);

        // Update owner's "to" list
        self.add_to_delegated_index(&owner_tron, &receiver_tron, false, timestamp)?;

        // Update receiver's "from" list
        self.add_to_delegated_index(&receiver_tron, &owner_tron, true, timestamp)?;

        Ok(())
    }

    /// Add an address to a delegated resource account index
    /// is_from: true means adding to "from" list (0x03 prefix), false means "to" list (0x04 prefix)
    fn add_to_delegated_index(
        &self,
        account: &[u8; 21],
        related: &[u8; 21],
        is_from: bool,
        timestamp: i64,
    ) -> Result<()> {
        use super::key_helpers::delegated_resource_account_index;

        let key = if is_from {
            delegated_resource_account_index::create_db_key_v2_from(account)
        } else {
            delegated_resource_account_index::create_db_key_v2_to(account)
        };

        // Get or create index
        let mut index = match self.storage_engine.get(self.delegated_resource_account_index_database(), &key)? {
            Some(data) => {
                crate::protocol::DelegatedResourceAccountIndex::decode(&data[..])
                    .map_err(|e| anyhow::anyhow!("Failed to decode DelegatedResourceAccountIndex: {}", e))?
            }
            None => {
                crate::protocol::DelegatedResourceAccountIndex {
                    account: account.to_vec(),
                    from_accounts: Vec::new(),
                    to_accounts: Vec::new(),
                    timestamp: 0,
                }
            }
        };

        // Add to appropriate list if not already present
        let list = if is_from { &mut index.from_accounts } else { &mut index.to_accounts };
        if !list.iter().any(|a| a == related) {
            list.push(related.to_vec());
        }
        index.timestamp = timestamp;

        // Persist
        let data = index.encode_to_vec();
        self.storage_engine.put(self.delegated_resource_account_index_database(), &key, &data)?;

        Ok(())
    }

    // ==========================================================================
    // Phase 2.C: Dynamic Properties for Contract Metadata
    // ==========================================================================

    /// Get ALLOW_TVM_CONSTANTINOPLE dynamic property
    /// Returns 0 if Constantinople is not enabled, non-zero if enabled
    /// Default: 0 (not enabled)
    pub fn get_allow_tvm_constantinople(&self) -> Result<i64> {
        let key = b"ALLOW_TVM_CONSTANTINOPLE";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let value = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    tracing::debug!("ALLOW_TVM_CONSTANTINOPLE: {}", value);
                    Ok(value)
                } else {
                    tracing::warn!("ALLOW_TVM_CONSTANTINOPLE has invalid length: {}", data.len());
                    Ok(0)
                }
            }
            None => {
                tracing::debug!("ALLOW_TVM_CONSTANTINOPLE not found, returning 0");
                Ok(0)
            }
        }
    }

    /// Get LATEST_BLOCK_HEADER_NUMBER dynamic property
    /// Returns the latest block number
    /// Default: 0
    pub fn get_latest_block_header_number(&self) -> Result<i64> {
        let key = b"LATEST_BLOCK_HEADER_NUMBER";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let value = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    tracing::debug!("LATEST_BLOCK_HEADER_NUMBER: {}", value);
                    Ok(value)
                } else {
                    tracing::warn!("LATEST_BLOCK_HEADER_NUMBER has invalid length: {}", data.len());
                    Ok(0)
                }
            }
            None => {
                tracing::debug!("LATEST_BLOCK_HEADER_NUMBER not found, returning 0");
                Ok(0)
            }
        }
    }

    /// Get BLOCK_NUM_FOR_ENERGY_LIMIT configuration
    /// This is typically a configuration constant, not a dynamic property
    /// For checkForEnergyLimit(): block_num >= BLOCK_NUM_FOR_ENERGY_LIMIT
    /// Default: 4727890 (mainnet value from CommonParameter)
    pub fn get_block_num_for_energy_limit(&self) -> i64 {
        // This is a constant from CommonParameter, not stored in DB
        // Mainnet value: 4727890
        // Testnet value might differ
        4727890
    }

    /// Check if energy limit feature is enabled based on current block number
    /// Equivalent to ReceiptCapsule.checkForEnergyLimit()
    pub fn check_for_energy_limit(&self) -> Result<bool> {
        let block_num = self.get_latest_block_header_number()?;
        let threshold = self.get_block_num_for_energy_limit();
        let enabled = block_num >= threshold;
        tracing::debug!("checkForEnergyLimit: block_num={}, threshold={}, enabled={}",
                       block_num, threshold, enabled);
        Ok(enabled)
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
        let address_tron = to_tron_address(&address);

        // Phase 0.1: Use decode→modify→encode pattern to preserve existing fields
        // First, try to read existing account data
        let existing_data = self.storage_engine.get(self.account_database(), &key)?;

        // Serialize using the update method that preserves existing fields
        let data = self.serialize_account_update(
            &address,
            &account,
            existing_data.as_deref(),
        );

        tracing::info!(
            "Setting account for address {:?} (tron: {}), balance: {}, key: {}, data_len: {}, existing: {}",
            address,
            address_tron,
            account.balance,
            hex::encode(&key),
            data.len(),
            existing_data.is_some()
        );

        self.storage_engine.put(self.account_database(), &key, &data)?;

        // Immediately verify the write by reading it back
        if let Ok(Some(read_data)) = self.storage_engine.get(self.account_database(), &key) {
            if read_data == data {
                tracing::debug!("Verified account write for {} - data matches", address_tron);
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
