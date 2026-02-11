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
use super::write_buffer::{ExecutionWriteBuffer, WriteOp};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

// Import the generated TRON protocol types
use crate::protocol::{Account as ProtoAccount, AccountType as ProtoAccountType};

/// Persistent implementation of EVM state store backed by the storage engine.
/// Routes data to appropriate RocksDB databases matching java-tron's organization
/// while providing a unified interface for EVM execution.
///
/// ## Buffered Writes (Phase B Conformance)
///
/// When `write_buffer` is set, all write operations are buffered instead of
/// being written directly to storage. This enables atomic commit/rollback:
/// - On success: call `commit_buffer()` to persist all changes
/// - On failure: drop the buffer (no writes occur)
///
/// This is essential for Phase B conformance where `validate_fail` fixtures
/// require zero writes to post_db.
pub struct EngineBackedEvmStateStore {
    storage_engine: StorageEngine,
    address_prefix: u8,
    /// Optional write buffer for atomic writes.
    /// When set, all writes go to the buffer instead of directly to storage.
    /// Use `commit_buffer()` to flush the buffer to storage on success.
    write_buffer: Option<Arc<Mutex<ExecutionWriteBuffer>>>,
}

impl EngineBackedEvmStateStore {
    /// TRON address prefix byte used by this database (0x41 mainnet, 0xa0 testnets).
    pub fn address_prefix(&self) -> u8 {
        self.address_prefix
    }

    pub fn new(storage_engine: StorageEngine) -> Self {
        let address_prefix = Self::detect_address_prefix(&storage_engine);
        Self {
            storage_engine,
            address_prefix,
            write_buffer: None,
        }
    }

    /// Create a new store with a write buffer for atomic writes.
    ///
    /// When the buffer is set, all write operations go to the buffer instead
    /// of directly to storage. Call `commit_buffer()` after successful execution
    /// to persist all changes atomically.
    pub fn new_with_buffer(storage_engine: StorageEngine) -> (Self, Arc<Mutex<ExecutionWriteBuffer>>) {
        let address_prefix = Self::detect_address_prefix(&storage_engine);
        let buffer = Arc::new(Mutex::new(ExecutionWriteBuffer::new()));
        let store = Self {
            storage_engine,
            address_prefix,
            write_buffer: Some(buffer.clone()),
        };
        (store, buffer)
    }

    /// Set the write buffer for this store.
    ///
    /// When set, all write operations go to the buffer instead of directly
    /// to storage. This is useful for sharing a buffer across multiple stores
    /// or for setting the buffer after construction.
    pub fn set_write_buffer(&mut self, buffer: Arc<Mutex<ExecutionWriteBuffer>>) {
        self.write_buffer = Some(buffer);
    }

    /// Clear the write buffer, returning to direct writes.
    pub fn clear_write_buffer(&mut self) {
        self.write_buffer = None;
    }

    /// Check if a write buffer is attached.
    pub fn has_write_buffer(&self) -> bool {
        self.write_buffer.is_some()
    }

    /// Get a reference to the write buffer if one is attached.
    pub fn get_write_buffer(&self) -> Option<Arc<Mutex<ExecutionWriteBuffer>>> {
        self.write_buffer.clone()
    }

    /// Commit the write buffer to storage.
    ///
    /// This persists all buffered writes to the storage engine atomically
    /// (per database). Should be called after successful execution.
    ///
    /// Returns an error if no buffer is attached or if the commit fails.
    pub fn commit_buffer(&mut self) -> Result<()> {
        if let Some(buffer) = &self.write_buffer {
            buffer.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?.commit(&self.storage_engine)?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("No write buffer attached"))
        }
    }

    /// Helper method to write to storage or buffer.
    ///
    /// If a write buffer is attached, the write goes to the buffer.
    /// Otherwise, it goes directly to the storage engine.
    fn buffered_put(&self, db: &str, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        if let Some(buffer) = &self.write_buffer {
            buffer.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?.put(db, key, value);
            Ok(())
        } else {
            self.storage_engine.put(db, &key, &value)
        }
    }

    /// Helper method to read from storage or buffer.
    ///
    /// When a write buffer is attached, reads consult the buffer first to
    /// provide read-your-writes semantics within a transaction.
    fn buffered_get(&self, db: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        if let Some(buffer) = &self.write_buffer {
            let guard = buffer.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
            if let Some(ops) = guard.get_operations(db) {
                if let Some(op) = ops.get(key) {
                    return match op {
                        WriteOp::Put(value) => Ok(Some(value.clone())),
                        WriteOp::Delete => Ok(None),
                    };
                }
            }
        }
        self.storage_engine.get(db, key)
    }

    /// Helper method to prefix-query from storage with buffer overlay.
    ///
    /// Some system/market handlers iterate keys after writing new ones within
    /// the same transaction; this makes those writes visible without commit.
    fn buffered_prefix_query(
        &self,
        db: &str,
        prefix: &[u8],
    ) -> Result<Vec<tron_backend_storage::KeyValue>> {
        let mut merged: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        for kv in self.storage_engine.prefix_query(db, prefix)? {
            merged.insert(kv.key, kv.value);
        }

        if let Some(buffer) = &self.write_buffer {
            let guard = buffer.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
            if let Some(ops) = guard.get_operations(db) {
                for (key, op) in ops {
                    if !key.starts_with(prefix) {
                        continue;
                    }
                    match op {
                        WriteOp::Put(value) => {
                            merged.insert(key.clone(), value.clone());
                        }
                        WriteOp::Delete => {
                            merged.remove(key);
                        }
                    }
                }
            }
        }

        Ok(merged
            .into_iter()
            .map(|(key, value)| tron_backend_storage::KeyValue {
                key,
                value,
                found: true,
            })
            .collect())
    }

    /// Helper method to delete from storage or buffer.
    ///
    /// If a write buffer is attached, the delete goes to the buffer.
    /// Otherwise, it goes directly to the storage engine.
    fn buffered_delete(&self, db: &str, key: Vec<u8>) -> Result<()> {
        if let Some(buffer) = &self.write_buffer {
            buffer.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?.delete(db, key);
            Ok(())
        } else {
            self.storage_engine.delete(db, &key)
        }
    }

    /// Detect the TRON address prefix byte used by the underlying database.
    ///
    /// Mainnet uses `0x41`, testnets commonly use `0xa0`.
    fn detect_address_prefix(storage_engine: &StorageEngine) -> u8 {
        let candidate_dbs = [
            db_names::account::ACCOUNT,
            db_names::governance::WITNESS,
            db_names::governance::VOTES,
        ];

        for db_name in candidate_dbs {
            // Scan more than one entry because some fixtures include non-address keys at the
            // beginning of the Account DB (e.g. intentionally malformed addresses for validation
            // tests). The first valid address prefix found determines the network prefix.
            let entries = match storage_engine.get_next(db_name, &Vec::new(), 256) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries {
                if entry.key.len() != 21 {
                    continue;
                }
                let prefix = entry.key[0];
                if prefix == 0x41 || prefix == 0xa0 {
                    return prefix;
                }
            }
        }

        0x41
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
    fn storage_row_database(&self) -> &str {
        db_names::storage::STORAGE_ROW
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
        key.push(self.address_prefix);
        key.extend_from_slice(address.as_slice()); // 20-byte address
        key
    }

    /// Convert Address to storage key for code (matching java-tron format).
    ///
    /// Java-tron stores contract code using the 21-byte TRON address key
    /// (prefix byte + 20-byte address), consistent with AccountStore and ContractStore.
    fn code_key(&self, address: &Address) -> Vec<u8> {
        self.account_key(address)
    }

    /// Convert Address to storage key for witness store (21-byte address with 0x41 prefix)
    fn witness_key(&self, address: &Address) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.push(self.address_prefix);
        key.extend_from_slice(address.as_slice()); // 20-byte address
        key
    }

    /// Convert Address to storage key for votes store (21-byte address with 0x41 prefix)
    fn votes_key(&self, address: &Address) -> Vec<u8> {
        let mut key = Vec::with_capacity(21);
        key.push(self.address_prefix);
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
        key.push(self.address_prefix);
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
        // java-tron hashes the 21-byte TRON address (prefix + 20 bytes), not the raw 20-byte EVM
        // address. Fixture DBs commonly use 0xa0 prefixes, mainnet uses 0x41.
        let tron_address = self.account_key(address);
        let addr_hash = keccak256(&tron_address);
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
        match self.buffered_get(self.account_database(), &key)? {
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
        let prev = self.buffered_get(self.account_database(), &key)?;
        let data = self.encode_account_proto_java_compatible(proto_account, prev.as_deref())?;
        self.buffered_put(self.account_database(), key, data)?;
        Ok(())
    }

    fn encode_account_proto_java_compatible(
        &self,
        proto_account: &ProtoAccount,
        prev_bytes: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        let data = proto_account.encode_to_vec();

        // Conformance fixtures assert raw DB bytes produced by java-tron's protobuf encoder.
        // Two java-specific behaviors matter for Account.assetV2 (field 56):
        // 1) Map entry order preserves insertion/parse order (not sorted by key).
        // 2) Map entry `key` is serialized even when empty ("") as `0x0A 0x00`.
        // 3) Map entry `value` is serialized even when it is the default `0` as `0x10 0x00`.
        //
        // Prost uses `BTreeMap` for deterministic ordering (sorted by key) and skips encoding
        // default fields (empty key and zero value), so we need a small compatibility rewrite.
        let needs_asset_v2_rewrite = proto_account.asset_v2.len() >= 2
            || proto_account.asset_v2.contains_key("")
            || proto_account.asset_v2.values().any(|v| *v == 0);
        if !needs_asset_v2_rewrite {
            return Ok(data);
        }

        let prev_order = match prev_bytes {
            Some(bytes) => Some(self.extract_account_asset_v2_key_order(bytes)?),
            None => None,
        };

        self.rewrite_account_asset_v2(&data, prev_order.as_deref())
    }

    fn rewrite_account_asset_v2(&self, data: &[u8], prev_order: Option<&[String]>) -> Result<Vec<u8>> {
        // Account.assetV2 field number in protocol.tron.proto is 56.
        const ACCOUNT_ASSET_V2_FIELD_NUMBER: u64 = 56;

        let (entries_by_key, current_order) = self.collect_account_asset_v2_entries(data)?;
        if entries_by_key.is_empty() {
            return Ok(data.to_vec());
        }

        let desired_order = self.merge_asset_v2_order(prev_order, &current_order, &entries_by_key)?;

        let mut out = Vec::with_capacity(data.len() + 8);
        let mut pos = 0usize;
        let mut emitted_asset_v2 = false;

        while pos < data.len() {
            let (tag, new_pos) = self.read_varint(data, pos)?;
            pos = new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            match wire_type {
                0 => {
                    self.write_varint(&mut out, tag);
                    let (value, next_pos) = self.read_varint(data, pos)?;
                    pos = next_pos;
                    self.write_varint(&mut out, value);
                }
                1 => {
                    self.write_varint(&mut out, tag);
                    out.extend_from_slice(&data[pos..pos + 8]);
                    pos += 8;
                }
                2 => {
                    let (length, next_pos) = self.read_varint(data, pos)?;
                    pos = next_pos;
                    let length_usize = length as usize;

                    if pos + length_usize > data.len() {
                        return Err(anyhow::anyhow!(
                            "Length-delimited field exceeds buffer: pos={} len={} total={}",
                            pos,
                            length_usize,
                            data.len()
                        ));
                    }

                    let payload = &data[pos..pos + length_usize];
                    pos += length_usize;

                    if field_number == ACCOUNT_ASSET_V2_FIELD_NUMBER {
                        // Skip all existing assetV2 entries; emit the rewritten entries exactly once.
                        if !emitted_asset_v2 {
                            emitted_asset_v2 = true;
                            for key in &desired_order {
                                if let Some(entry_bytes) = entries_by_key.get(key) {
                                    // field 56, wire type 2
                                    self.write_varint(&mut out, tag);
                                    self.write_varint(&mut out, entry_bytes.len() as u64);
                                    out.extend_from_slice(entry_bytes);
                                }
                            }
                        }
                        continue;
                    }

                    self.write_varint(&mut out, tag);
                    self.write_varint(&mut out, length);
                    out.extend_from_slice(payload);
                }
                5 => {
                    self.write_varint(&mut out, tag);
                    out.extend_from_slice(&data[pos..pos + 4]);
                    pos += 4;
                }
                _ => return Err(anyhow::anyhow!("Unknown wire type: {}", wire_type)),
            }
        }

        Ok(out)
    }

    fn collect_account_asset_v2_entries(
        &self,
        data: &[u8],
    ) -> Result<(BTreeMap<String, Vec<u8>>, Vec<String>)> {
        const ACCOUNT_ASSET_V2_FIELD_NUMBER: u64 = 56;

        let mut entries_by_key: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut order = Vec::new();

        let mut pos = 0usize;
        while pos < data.len() {
            let (tag, new_pos) = self.read_varint(data, pos)?;
            pos = new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            if wire_type != 2 {
                pos = self.skip_field(data, pos, wire_type)?;
                continue;
            }

            let (length, next_pos) = self.read_varint(data, pos)?;
            pos = next_pos;
            let length_usize = length as usize;
            if pos + length_usize > data.len() {
                return Err(anyhow::anyhow!(
                    "Length-delimited field exceeds buffer: pos={} len={} total={}",
                    pos,
                    length_usize,
                    data.len()
                ));
            }

            let payload = &data[pos..pos + length_usize];
            pos += length_usize;

            if field_number != ACCOUNT_ASSET_V2_FIELD_NUMBER {
                continue;
            }

            let key = self.map_entry_string_key(payload)?;
            if !entries_by_key.contains_key(&key) {
                order.push(key.clone());
            }

            let entry_bytes = if self.map_entry_has_string_key(payload)? {
                payload.to_vec()
            } else {
                // Ensure empty-string keys serialize the `key` field: `0x0A 0x00`.
                let mut patched = Vec::with_capacity(payload.len() + 2);
                patched.extend_from_slice(&[0x0A, 0x00]);
                patched.extend_from_slice(payload);
                patched
            };
            let entry_bytes = if self.map_entry_has_int64_value(entry_bytes.as_slice())? {
                entry_bytes
            } else {
                // Ensure zero values serialize the `value` field: `0x10 0x00`.
                let mut patched = Vec::with_capacity(entry_bytes.len() + 2);
                patched.extend_from_slice(&entry_bytes);
                patched.extend_from_slice(&[0x10, 0x00]);
                patched
            };

            entries_by_key.insert(key, entry_bytes);
        }

        Ok((entries_by_key, order))
    }

    fn merge_asset_v2_order(
        &self,
        prev_order: Option<&[String]>,
        current_order: &[String],
        entries_by_key: &BTreeMap<String, Vec<u8>>,
    ) -> Result<Vec<String>> {
        let mut desired = Vec::with_capacity(entries_by_key.len());
        let mut seen = std::collections::BTreeSet::<String>::new();

        if let Some(prev) = prev_order {
            for key in prev {
                if entries_by_key.contains_key(key) && !seen.contains(key) {
                    desired.push(key.clone());
                    seen.insert(key.clone());
                }
            }
        }

        for key in current_order {
            if entries_by_key.contains_key(key) && !seen.contains(key) {
                desired.push(key.clone());
                seen.insert(key.clone());
            }
        }

        // Safety net: append any remaining keys deterministically.
        for key in entries_by_key.keys() {
            if !seen.contains(key) {
                desired.push(key.clone());
                seen.insert(key.clone());
            }
        }

        Ok(desired)
    }

    fn extract_account_asset_v2_key_order(&self, data: &[u8]) -> Result<Vec<String>> {
        const ACCOUNT_ASSET_V2_FIELD_NUMBER: u64 = 56;

        let mut order = Vec::new();
        let mut pos = 0usize;

        while pos < data.len() {
            let (tag, new_pos) = self.read_varint(data, pos)?;
            pos = new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            if wire_type != 2 {
                pos = self.skip_field(data, pos, wire_type)?;
                continue;
            }

            let (length, next_pos) = self.read_varint(data, pos)?;
            pos = next_pos;
            let length_usize = length as usize;
            if pos + length_usize > data.len() {
                return Err(anyhow::anyhow!(
                    "Length-delimited field exceeds buffer: pos={} len={} total={}",
                    pos,
                    length_usize,
                    data.len()
                ));
            }

            let payload = &data[pos..pos + length_usize];
            pos += length_usize;

            if field_number == ACCOUNT_ASSET_V2_FIELD_NUMBER {
                order.push(self.map_entry_string_key(payload)?);
            }
        }

        Ok(order)
    }

    fn map_entry_has_string_key(&self, entry: &[u8]) -> Result<bool> {
        let mut pos = 0usize;
        while pos < entry.len() {
            let (tag, new_pos) = self.read_varint(entry, pos)?;
            pos = new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            if field_number == 1 && wire_type == 2 {
                return Ok(true);
            }

            pos = self.skip_field(entry, pos, wire_type)?;
        }

        Ok(false)
    }

    fn map_entry_has_int64_value(&self, entry: &[u8]) -> Result<bool> {
        let mut pos = 0usize;
        while pos < entry.len() {
            let (tag, new_pos) = self.read_varint(entry, pos)?;
            pos = new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            if field_number == 2 && wire_type == 0 {
                return Ok(true);
            }

            pos = self.skip_field(entry, pos, wire_type)?;
        }

        Ok(false)
    }

    fn map_entry_string_key(&self, entry: &[u8]) -> Result<String> {
        let mut pos = 0usize;
        while pos < entry.len() {
            let (tag, new_pos) = self.read_varint(entry, pos)?;
            pos = new_pos;

            let field_number = tag >> 3;
            let wire_type = tag & 0x7;

            if field_number == 1 {
                if wire_type != 2 {
                    return Err(anyhow::anyhow!(
                        "Invalid wire type for map key: expected 2, got {}",
                        wire_type
                    ));
                }
                let (length, next_pos) = self.read_varint(entry, pos)?;
                pos = next_pos;
                let length_usize = length as usize;
                if pos + length_usize > entry.len() {
                    return Err(anyhow::anyhow!("Map key extends past entry bounds"));
                }
                let bytes = &entry[pos..pos + length_usize];
                return Ok(String::from_utf8_lossy(bytes).to_string());
            }

            pos = self.skip_field(entry, pos, wire_type)?;
        }

        Ok(String::new())
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

    /// Get CreateNewAccountBandwidthRate dynamic property
    /// This is the multiplier applied to bytes for create-account bandwidth cost.
    /// Java reference: DynamicPropertiesStore.getCreateNewAccountBandwidthRate()
    /// Default value: 1 (no multiplier)
    pub fn get_create_new_account_bandwidth_rate(&self) -> Result<i64> {
        let key = b"CREATE_NEW_ACCOUNT_BANDWIDTH_RATE";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) if data.len() >= 8 => {
                let rate = i64::from_be_bytes([
                    data[0], data[1], data[2], data[3],
                    data[4], data[5], data[6], data[7],
                ]);
                tracing::debug!("CREATE_NEW_ACCOUNT_BANDWIDTH_RATE from DB: {}", rate);
                Ok(rate)
            }
            _ => {
                tracing::debug!("CREATE_NEW_ACCOUNT_BANDWIDTH_RATE not found, using default 1");
                Ok(1) // Default: no multiplier
            }
        }
    }

    /// Get CreateAccountFee dynamic property
    /// Fee charged as fallback when bandwidth is insufficient for account creation.
    /// Java reference: DynamicPropertiesStore.getCreateAccountFee()
    /// Default value: 100_000 SUN (0.1 TRX)
    pub fn get_create_account_fee(&self) -> Result<u64> {
        let key = b"CREATE_ACCOUNT_FEE";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) if data.len() >= 8 => {
                let fee = u64::from_be_bytes([
                    data[0], data[1], data[2], data[3],
                    data[4], data[5], data[6], data[7],
                ]);
                tracing::debug!("CREATE_ACCOUNT_FEE from DB: {} SUN", fee);
                Ok(fee)
            }
            _ => {
                tracing::debug!("CREATE_ACCOUNT_FEE not found, using default 100000 SUN");
                Ok(100_000) // 0.1 TRX in SUN (default from TRON)
            }
        }
    }

    /// Get TOTAL_CREATE_ACCOUNT_COST dynamic property.
    /// Accumulated cost of all create-account transactions via fee path.
    /// Java reference: DynamicPropertiesStore.getTotalCreateAccountCost()
    /// Default: 0 if not present.
    pub fn get_total_create_account_cost(&self) -> Result<i64> {
        let key = b"TOTAL_CREATE_ACCOUNT_COST";
        match self.buffered_get(self.dynamic_properties_database(), key)? {
            Some(data) if data.len() >= 8 => Ok(i64::from_be_bytes([
                data[0], data[1], data[2], data[3],
                data[4], data[5], data[6], data[7],
            ])),
            _ => Ok(0),
        }
    }

    /// Add to TOTAL_CREATE_ACCOUNT_COST dynamic property (java: addTotalCreateAccountCost()).
    /// Called when bandwidth is insufficient and fee fallback is used for account creation.
    pub fn add_total_create_account_cost(&self, fee: u64) -> Result<()> {
        if fee == 0 {
            return Ok(());
        }

        let delta: i64 = fee
            .try_into()
            .map_err(|_| anyhow::anyhow!("fee exceeds i64::MAX"))?;
        let current = self.get_total_create_account_cost()?;
        let new_value = current
            .checked_add(delta)
            .ok_or_else(|| anyhow::anyhow!("Overflow in add_total_create_account_cost"))?;

        let key = b"TOTAL_CREATE_ACCOUNT_COST";
        self.buffered_put(
            self.dynamic_properties_database(),
            key.to_vec(),
            new_value.to_be_bytes().to_vec(),
        )?;
        tracing::debug!("TOTAL_CREATE_ACCOUNT_COST updated: {} -> {}", current, new_value);
        Ok(())
    }

    /// Get AllowMultiSign dynamic property
    /// Java-tron uses strict `== 1` check (not just `!= 0`) for parity.
    /// Java throws `IllegalArgumentException("not found ALLOW_MULTI_SIGN")` if missing.
    pub fn get_allow_multi_sign(&self) -> Result<bool> {
        let key = b"ALLOW_MULTI_SIGN";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                // Java stores dynamic properties as big-endian i64.
                // Java: getAllowMultiSign() != 1 is "not allowed", so we need strict == 1 check.
                if data.len() >= 8 {
                    let val = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    Ok(val == 1)
                } else if !data.is_empty() {
                    // Fallback for short data (edge case)
                    Ok(data[data.len() - 1] == 1)
                } else {
                    // Empty data treated as missing for strict parity
                    Err(anyhow::anyhow!("not found ALLOW_MULTI_SIGN"))
                }
            },
            None => {
                // Java throws IllegalArgumentException when key is missing
                Err(anyhow::anyhow!("not found ALLOW_MULTI_SIGN"))
            }
        }
    }

    /// Get ACTIVE_DEFAULT_OPERATIONS dynamic property.
    ///
    /// Java reference: `DynamicPropertiesStore.getActiveDefaultOperations()`, with default value
    /// "7fff1fc0033e0000000000000000000000000000000000000000000000000000" when missing.
    pub fn get_active_default_operations(&self) -> Result<Vec<u8>> {
        const DEFAULT_ACTIVE_DEFAULT_OPERATIONS: [u8; 32] = [
            0x7f, 0xff, 0x1f, 0xc0, 0x03, 0x3e, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        let key = b"ACTIVE_DEFAULT_OPERATIONS";
        match self.buffered_get(self.dynamic_properties_database(), key)? {
            Some(data) if !data.is_empty() => Ok(data),
            _ => Ok(DEFAULT_ACTIVE_DEFAULT_OPERATIONS.to_vec()),
        }
    }

    /// Get FORBID_TRANSFER_TO_CONTRACT dynamic property.
    ///
    /// Java reference: `DynamicPropertiesStore.getForbidTransferToContract()`.
    /// Default: 0 when missing/invalid.
    pub fn get_forbid_transfer_to_contract(&self) -> Result<u64> {
        let key = b"FORBID_TRANSFER_TO_CONTRACT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(u64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else if !data.is_empty() {
                    Ok(data[0] as u64)
                } else {
                    Ok(0)
                }
            }
            None => Ok(0),
        }
    }

    /// Get ALLOW_TVM_COMPATIBLE_EVM dynamic property.
    ///
    /// Java reference: `DynamicPropertiesStore.getAllowTvmCompatibleEvm()`.
    /// Default: 0 when missing/invalid.
    pub fn get_allow_tvm_compatible_evm(&self) -> Result<u64> {
        let key = b"ALLOW_TVM_COMPATIBLE_EVM";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(u64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else if !data.is_empty() {
                    Ok(data[0] as u64)
                } else {
                    Ok(0)
                }
            }
            None => Ok(0),
        }
    }

    /// Get Black Hole Optimization dynamic property (parity with Java)
    /// Java stores this as a long under key "ALLOW_BLACKHOLE_OPTIMIZATION".
    /// Java uses strict `== 1` check (not just `!= 0`) for parity.
    /// When this flag is 1, the node BURNS fees (optimization enabled).
    /// When 0 or any other value, the node CREDITS the blackhole account.
    /// Default: false (credit blackhole) to match early-chain behavior when key is absent.
    pub fn support_black_hole_optimization(&self) -> Result<bool> {
        // Parity key with java-tron DynamicPropertiesStore
        let key = b"ALLOW_BLACKHOLE_OPTIMIZATION";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                // Java writes a long; interpret big-endian i64 when length >= 8.
                // Java: supportBlackHoleOptimization() checks value == 1 (strict).
                if data.len() >= 8 {
                    let val = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(val == 1)
                } else if !data.is_empty() {
                    // Fallback: treat last byte as the value (edge case)
                    Ok(data[data.len() - 1] == 1)
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

    /// Get TOTAL_CREATE_WITNESS_FEE dynamic property.
    ///
    /// Java stores this under key "TOTAL_CREATE_WITNESS_FEE" (constant name: TOTAL_CREATE_WITNESS_COST).
    /// Default: 0 if not present.
    pub fn get_total_create_witness_cost(&self) -> Result<i64> {
        let key = b"TOTAL_CREATE_WITNESS_FEE";
        match self.buffered_get(self.dynamic_properties_database(), key)? {
            Some(data) if data.len() >= 8 => Ok(i64::from_be_bytes([
                data[0], data[1], data[2], data[3],
                data[4], data[5], data[6], data[7],
            ])),
            Some(_) => Ok(0),
            None => Ok(0),
        }
    }

    /// Add to TOTAL_CREATE_WITNESS_FEE dynamic property (java: addTotalCreateWitnessCost()).
    pub fn add_total_create_witness_cost(&self, fee: u64) -> Result<()> {
        if fee == 0 {
            return Ok(());
        }

        let delta: i64 = fee
            .try_into()
            .map_err(|_| anyhow::anyhow!("fee exceeds i64::MAX"))?;
        let current = self.get_total_create_witness_cost()?;
        let new_value = current
            .checked_add(delta)
            .ok_or_else(|| anyhow::anyhow!("Overflow in add_total_create_witness_cost"))?;

        let key = b"TOTAL_CREATE_WITNESS_FEE";
        self.buffered_put(
            self.dynamic_properties_database(),
            key.to_vec(),
            new_value.to_be_bytes().to_vec(),
        )?;
        Ok(())
    }

    /// Get BURN_TRX_AMOUNT dynamic property.
    /// Default: 0 if not present.
    pub fn get_burn_trx_amount(&self) -> Result<i64> {
        let key = b"BURN_TRX_AMOUNT";
        match self.buffered_get(self.dynamic_properties_database(), key)? {
            Some(data) if data.len() >= 8 => Ok(i64::from_be_bytes([
                data[0], data[1], data[2], data[3],
                data[4], data[5], data[6], data[7],
            ])),
            Some(_) => Ok(0),
            None => Ok(0),
        }
    }

    /// Burn TRX by incrementing BURN_TRX_AMOUNT (java: burnTrx()).
    pub fn burn_trx(&self, amount: u64) -> Result<()> {
        if amount == 0 {
            return Ok(());
        }

        let delta: i64 = amount
            .try_into()
            .map_err(|_| anyhow::anyhow!("burn amount exceeds i64::MAX"))?;
        let current = self.get_burn_trx_amount()?;
        let new_value = current
            .checked_add(delta)
            .ok_or_else(|| anyhow::anyhow!("Overflow in burn_trx"))?;

        let key = b"BURN_TRX_AMOUNT";
        self.buffered_put(
            self.dynamic_properties_database(),
            key.to_vec(),
            new_value.to_be_bytes().to_vec(),
        )?;
        Ok(())
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

    /// Get UNFREEZE_DELAY_DAYS dynamic property value.
    ///
    /// Returns the configured unfreeze delay in days, or 0 when missing/invalid.
    pub fn get_unfreeze_delay_days(&self) -> Result<i64> {
        let key = b"UNFREEZE_DELAY_DAYS";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]))
                } else if !data.is_empty() {
                    Ok(data[0] as i64)
                } else {
                    Ok(0)
                }
            }
            None => Ok(0),
        }
    }

    /// Get MAX_DELEGATE_LOCK_PERIOD dynamic property (in blocks).
    ///
    /// Java parity:
    /// - getMaxDelegateLockPeriod() defaults to DELEGATE_PERIOD / BLOCK_PRODUCED_INTERVAL (86400)
    ///   when missing
    /// - supportMaxDelegateLockPeriod() requires max > default and UNFREEZE_DELAY_DAYS > 0
    pub fn get_max_delegate_lock_period(&self) -> Result<i64> {
        const DEFAULT_MAX_DELEGATE_LOCK_PERIOD: i64 = 86400; // DELEGATE_PERIOD / BLOCK_PRODUCED_INTERVAL
        let key = b"MAX_DELEGATE_LOCK_PERIOD";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else if !data.is_empty() {
                    Ok(data[0] as i64)
                } else {
                    Ok(DEFAULT_MAX_DELEGATE_LOCK_PERIOD)
                }
            }
            None => Ok(DEFAULT_MAX_DELEGATE_LOCK_PERIOD),
        }
    }

    /// Check supportMaxDelegateLockPeriod() (DynamicPropertiesStore.supportMaxDelegateLockPeriod).
    pub fn support_max_delegate_lock_period(&self) -> Result<bool> {
        const DEFAULT_MAX_DELEGATE_LOCK_PERIOD: i64 = 86400; // DELEGATE_PERIOD / BLOCK_PRODUCED_INTERVAL
        let max_lock_period = self.get_max_delegate_lock_period()?;
        let unfreeze_delay_days = self.get_unfreeze_delay_days()?;
        Ok(max_lock_period > DEFAULT_MAX_DELEGATE_LOCK_PERIOD && unfreeze_delay_days > 0)
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
                    // Invalid or empty value: fall back to default for the detected network prefix
                    Ok(Some(self.get_blackhole_address_evm()))
                }
            },
            None => {
                // Not configured in dynamic properties - use sane network default for prefix
                Ok(Some(self.get_blackhole_address_evm()))
            }
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

    /// Get ENERGY_FEE dynamic property (SUN per energy unit).
    ///
    /// Java stores dynamic properties as big-endian i64/u64 under their string keys.
    /// When missing or invalid, return 0 and allow callers to fall back to context values.
    pub fn get_energy_fee(&self) -> Result<u64> {
        let key = b"ENERGY_FEE";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    if val > 0 {
                        Ok(val as u64)
                    } else {
                        Ok(0)
                    }
                } else if !data.is_empty() {
                    Ok(data[0] as u64)
                } else {
                    Ok(0)
                }
            }
            None => Ok(0),
        }
    }

    /// Apply TRON VM energy fee accounting and minimal resource timestamp updates.
    ///
    /// For VM transactions, java-tron charges `energy_used * energy_price` from the sender's
    /// Account.balance and, when blackhole optimization is disabled, credits the blackhole account
    /// by the same amount.
    ///
    /// Additionally, when `latest_block_header_timestamp` is available (>0), java-tron updates:
    /// - `Account.latest_opration_time = latest_block_header_timestamp`
    /// - `Account.account_resource.latest_consume_time_for_energy = latest_block_header_timestamp / 3000`
    ///
    /// This helper is intentionally minimal and only touches fields required by conformance
    /// fixtures.
    pub fn apply_vm_energy_fee(
        &self,
        owner: &Address,
        energy_used: u64,
        energy_price: u64,
    ) -> Result<()> {
        // Parity: java-tron derives energy price from the dynamic property ENERGY_FEE.
        // Fixtures may set ExecutionContext.energy_price differently, so treat it as a fallback.
        let effective_price = match self.get_energy_fee()? {
            0 => energy_price,
            v => v,
        };

        let fee_sun = energy_used.saturating_mul(effective_price);
        if fee_sun == 0 {
            return Ok(());
        }

        let fee_i64 = fee_sun as i64; // preserve low 64 bits (two's complement) like Java
        let now_ms = self.get_latest_block_header_timestamp()?;

        // Update owner account balance and timestamps.
        if let Some(mut owner_account) = self.get_account_proto(owner)? {
            owner_account.balance = owner_account.balance.wrapping_sub(fee_i64);

            if now_ms > 0 {
                owner_account.latest_opration_time = now_ms;
                let head_slot = now_ms / 3000;
                if owner_account.account_resource.is_none() {
                    owner_account.account_resource =
                        Some(crate::protocol::account::AccountResource::default());
                }
                if let Some(ar) = owner_account.account_resource.as_mut() {
                    ar.latest_consume_time_for_energy = head_slot;
                }
            }

            self.put_account_proto(owner, &owner_account)?;
        }

        // Credit blackhole account when optimization is disabled (fees are not burned).
        if !self.support_black_hole_optimization()? {
            if let Some(blackhole_address) = self.get_blackhole_address()? {
                if let Some(mut blackhole_account) = self.get_account_proto(&blackhole_address)? {
                    blackhole_account.balance = blackhole_account.balance.wrapping_add(fee_i64);
                    self.put_account_proto(&blackhole_address, &blackhole_account)?;
                }
            }
        }

        Ok(())
    }

    /// Get WITNESS_ALLOWANCE_FROZEN_TIME dynamic property
    /// Number of days for witness withdrawal cooldown (multiplied by FROZEN_PERIOD to get ms)
    /// Default: 1 day if missing
    /// FROZEN_PERIOD = 86,400,000 ms (24 hours in ms)
    pub fn get_witness_allowance_frozen_time(&self) -> Result<i64> {
        let key = b"WITNESS_ALLOWANCE_FROZEN_TIME";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                match data.len() {
                    len if len >= 8 => Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                    ])),
                    4 => Ok(i32::from_be_bytes([data[0], data[1], data[2], data[3]]) as i64),
                    1 => Ok(data[0] as i64),
                    0 => Ok(1), // Default: 1 day
                    other => {
                        tracing::warn!(
                            "WITNESS_ALLOWANCE_FROZEN_TIME has invalid length: {}",
                            other
                        );
                        Ok(1)
                    }
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

        match self.buffered_get(self.account_database(), &key)? {
            Some(data) => {
                let allowance = match ProtoAccount::decode(data.as_slice()) {
                    Ok(proto_account) => {
                        let allowance = proto_account.allowance;
                        match self.extract_i64_field_from_protobuf(&data, 11) {
                            Ok(scanned) if scanned != allowance => {
                                tracing::warn!(
                                    "Account {} allowance mismatch: proto={} scanned={} (using proto)",
                                    to_tron_address(address),
                                    allowance,
                                    scanned
                                );
                            }
                            Err(e) => {
                                tracing::debug!(
                                    "Account {} allowance: proto={} (scan failed: {})",
                                    to_tron_address(address),
                                    allowance,
                                    e
                                );
                            }
                            _ => {}
                        }
                        allowance
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to decode Account proto for allowance ({}), falling back to scan: {}",
                            to_tron_address(address),
                            e
                        );
                        match self.extract_i64_field_from_protobuf(&data, 11) {
                            Ok(allowance) => allowance,
                            Err(e) => {
                                tracing::debug!(
                                    "Failed to extract allowance from account ({}): {}, returning 0",
                                    to_tron_address(address),
                                    e
                                );
                                0
                            }
                        }
                    }
                };

                tracing::debug!(
                    "Account {} allowance: {}",
                    to_tron_address(address),
                    allowance
                );
                Ok(allowance)
            }
            None => {
                tracing::debug!(
                    "Account not found for address {:?}, returning allowance 0",
                    address
                );
                Ok(0)
            }
        }
    }

    /// Get Account.latest_withdraw_time field (field 12 in Account protobuf)
    /// This is the timestamp of the last witness reward withdrawal
    /// Returns 0 if account doesn't exist or field not present
    pub fn get_account_latest_withdraw_time(&self, address: &Address) -> Result<i64> {
        let key = self.account_key(address);

        match self.buffered_get(self.account_database(), &key)? {
            Some(data) => {
                let latest_withdraw_time = match ProtoAccount::decode(data.as_slice()) {
                    Ok(proto_account) => {
                        let latest_withdraw_time = proto_account.latest_withdraw_time;
                        match self.extract_i64_field_from_protobuf(&data, 12) {
                            Ok(scanned) if scanned != latest_withdraw_time => {
                                tracing::warn!(
                                    "Account {} latest_withdraw_time mismatch: proto={} scanned={} (using proto)",
                                    to_tron_address(address),
                                    latest_withdraw_time,
                                    scanned
                                );
                            }
                            Err(e) => {
                                tracing::debug!(
                                    "Account {} latest_withdraw_time: proto={} (scan failed: {})",
                                    to_tron_address(address),
                                    latest_withdraw_time,
                                    e
                                );
                            }
                            _ => {}
                        }
                        latest_withdraw_time
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to decode Account proto for latest_withdraw_time ({}), falling back to scan: {}",
                            to_tron_address(address),
                            e
                        );
                        match self.extract_i64_field_from_protobuf(&data, 12) {
                            Ok(latest_withdraw_time) => latest_withdraw_time,
                            Err(e) => {
                                tracing::debug!(
                                    "Failed to extract latest_withdraw_time from account ({}): {}, returning 0",
                                    to_tron_address(address),
                                    e
                                );
                                0
                            }
                        }
                    }
                };

                tracing::debug!(
                    "Account {} latest_withdraw_time: {}",
                    to_tron_address(address),
                    latest_withdraw_time
                );
                Ok(latest_withdraw_time)
            }
            None => {
                tracing::debug!(
                    "Account not found for address {:?}, returning latest_withdraw_time 0",
                    address
                );
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
        self.buffered_put(self.dynamic_properties_database(), key.to_vec(), data.to_vec())?;
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
        self.buffered_put(self.dynamic_properties_database(), key.to_vec(), data.to_vec())?;
        Ok(())
    }

    /// Get TOTAL_NET_WEIGHT dynamic property (total frozen for bandwidth)
    /// Default: 0
    pub fn get_total_net_weight(&self) -> Result<i64> {
        let key = b"TOTAL_NET_WEIGHT";
        match self.buffered_get(self.dynamic_properties_database(), key)? {
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
        match self.buffered_get(self.dynamic_properties_database(), key)? {
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
        self.buffered_put(self.dynamic_properties_database(), key.to_vec(), data.to_vec())?;
        Ok(())
    }

    /// Get TOTAL_ENERGY_WEIGHT dynamic property
    /// Default: 0
    pub fn get_total_energy_weight(&self) -> Result<i64> {
        let key = b"TOTAL_ENERGY_WEIGHT";
        match self.buffered_get(self.dynamic_properties_database(), key)? {
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
        self.buffered_put(self.dynamic_properties_database(), key.to_vec(), data.to_vec())?;
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
        self.buffered_put(self.dynamic_properties_database(), key.to_vec(), data.to_vec())?;
        Ok(())
    }

    /// Check ALLOW_CANCEL_ALL_UNFREEZE_V2 dynamic property
    /// Returns true if CancelAllUnfreezeV2 is enabled
    /// Default: false
    pub fn support_allow_cancel_all_unfreeze_v2(&self) -> Result<bool> {
        let key = b"ALLOW_CANCEL_ALL_UNFREEZE_V2";
        let allow_cancel = match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    val == 1
                } else if !data.is_empty() {
                    data[0] == 1
                } else {
                    false
                }
            },
            None => false, // Default disabled
        };

        // Java parity: enabled only when ALLOW_CANCEL_ALL_UNFREEZE_V2 == 1
        // and UNFREEZE_DELAY_DAYS > 0.
        let unfreeze_delay_days = self.get_unfreeze_delay_days()?;
        Ok(allow_cancel && unfreeze_delay_days > 0)
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
        match self.buffered_get(self.dynamic_properties_database(), key)? {
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
        let records = self.buffered_prefix_query(self.freeze_records_database(), &[])?;

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
        let records = self.buffered_prefix_query(self.freeze_records_database(), &[])?;

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
        // Use protobuf encoding for Java compatibility, with the detected network prefix.
        let data = witness.serialize_with_prefix(self.address_prefix);

        tracing::debug!("Storing witness (protobuf format) for address {:?}, key: {}, URL: {}, votes: {}",
                       witness.address, hex::encode(&key), witness.url, witness.vote_count);

        self.buffered_put(self.witness_database(), key, data)?;
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
        let data = votes.serialize_with_prefix(self.address_prefix);

        tracing::debug!("Storing votes for address {:?}, key: {}, old_votes: {}, new_votes: {}",
                       address, hex::encode(&key), votes.old_votes.len(), votes.new_votes.len());

        self.buffered_put(self.votes_database(), key, data)?;
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

                    // Remove TRON address prefix if present (21-byte Tron → 20-byte EVM)
                    let evm_addr = if addr_bytes.len() == 21 && (addr_bytes[0] == 0x41 || addr_bytes[0] == 0xa0) {
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

        match self.buffered_get(self.freeze_records_database(), &key)? {
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

        self.buffered_put(self.freeze_records_database(), key, data)?;
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

        self.buffered_delete(self.freeze_records_database(), key)?;
        Ok(())
    }

    /// Get tron power for an address in SUN
    /// Matches java-tron's `AccountCapsule.getTronPower()` / `getAllTronPower()` semantics:
    /// - `new_model=false` (ALLOW_NEW_RESOURCE_MODEL=0): `getTronPower()`
    /// - `new_model=true`  (ALLOW_NEW_RESOURCE_MODEL=1): `getAllTronPower()`
    pub fn get_tron_power_in_sun(&self, address: &Address, new_model: bool) -> Result<u64> {
        fn add_non_negative_i64(total: &mut i128, value: i64, label: &'static str) -> Result<()> {
            if value < 0 {
                return Err(anyhow::anyhow!("Negative {}: {}", label, value));
            }
            *total = total
                .checked_add(value as i128)
                .ok_or_else(|| anyhow::anyhow!("Overflow while adding {}", label))?;
            Ok(())
        }

        fn to_u64_checked(value: i128, label: &'static str) -> Result<u64> {
            if value < 0 {
                return Err(anyhow::anyhow!("Negative {} total: {}", label, value));
            }
            u64::try_from(value).map_err(|_| anyhow::anyhow!("{} exceeds u64::MAX: {}", label, value))
        }

        const TRON_POWER: i32 = crate::protocol::ResourceCode::TronPower as i32;

        // Prefer AccountStore representation (java-tron canonical) when present.
        let account_total = match self.get_account_proto(address)? {
            Some(account) => {
                let mut tron_power: i128 = 0;

                // bandwidth frozen balance (Account.frozen)
                for frozen in &account.frozen {
                    add_non_negative_i64(&mut tron_power, frozen.frozen_balance, "frozen_balance")?;
                }

                // energy frozen balance + delegated balances (Account.account_resource)
                if let Some(resource) = account.account_resource.as_ref() {
                    if let Some(frozen_energy) = resource.frozen_balance_for_energy.as_ref() {
                        add_non_negative_i64(
                            &mut tron_power,
                            frozen_energy.frozen_balance,
                            "frozen_balance_for_energy",
                        )?;
                    }
                    add_non_negative_i64(
                        &mut tron_power,
                        resource.delegated_frozen_balance_for_energy,
                        "delegated_frozen_balance_for_energy",
                    )?;
                    add_non_negative_i64(
                        &mut tron_power,
                        resource.delegated_frozen_v2_balance_for_energy,
                        "delegated_frozen_v2_balance_for_energy",
                    )?;
                }

                // bandwidth delegated balances (Account.delegated_frozen_balance_for_bandwidth)
                add_non_negative_i64(
                    &mut tron_power,
                    account.delegated_frozen_balance_for_bandwidth,
                    "delegated_frozen_balance_for_bandwidth",
                )?;
                add_non_negative_i64(
                    &mut tron_power,
                    account.delegated_frozen_v2_balance_for_bandwidth,
                    "delegated_frozen_v2_balance_for_bandwidth",
                )?;

                // FreezeV2 balances for BANDWIDTH/ENERGY (exclude TRON_POWER).
                for frozen_v2 in &account.frozen_v2 {
                    if frozen_v2.r#type != TRON_POWER {
                        add_non_negative_i64(&mut tron_power, frozen_v2.amount, "frozen_v2_amount")?;
                    }
                }

                if !new_model {
                    to_u64_checked(tron_power, "tron_power")?
                } else {
                    // getAllTronPower() = getTronPower() + tronPowerFrozenBalance + tronPowerFrozenV2Balance
                    // with old_tron_power gating.
                    let tron_power_frozen_balance = account
                        .tron_power
                        .as_ref()
                        .map(|f| f.frozen_balance)
                        .unwrap_or(0);

                    let mut tron_power_frozen_v2_balance: i128 = 0;
                    for frozen_v2 in &account.frozen_v2 {
                        if frozen_v2.r#type == TRON_POWER {
                            add_non_negative_i64(
                                &mut tron_power_frozen_v2_balance,
                                frozen_v2.amount,
                                "tron_power_frozen_v2_amount",
                            )?;
                        }
                    }

                    let tp_frozen_total: i128 = {
                        let mut tmp: i128 = 0;
                        add_non_negative_i64(
                            &mut tmp,
                            tron_power_frozen_balance,
                            "tron_power_frozen_balance",
                        )?;
                        tmp.checked_add(tron_power_frozen_v2_balance)
                            .ok_or_else(|| anyhow::anyhow!("Overflow while summing tron power frozen totals"))?
                    };

                    let old_tron_power = account.old_tron_power;
                    let all_tron_power = if old_tron_power == -1 {
                        tp_frozen_total
                    } else if old_tron_power == 0 {
                        tron_power
                            .checked_add(tp_frozen_total)
                            .ok_or_else(|| anyhow::anyhow!("Overflow while summing all_tron_power"))?
                    } else if old_tron_power > 0 {
                        (old_tron_power as i128)
                            .checked_add(tp_frozen_total)
                            .ok_or_else(|| anyhow::anyhow!("Overflow while summing all_tron_power"))?
                    } else {
                        return Err(anyhow::anyhow!("Invalid old_tron_power: {}", old_tron_power));
                    };

                    to_u64_checked(all_tron_power, "all_tron_power")?
                }
            }
            None => 0,
        };

        if account_total > 0 {
            tracing::info!(
                address = ?address,
                new_model = new_model,
                total = account_total,
                "Computed tron power from AccountStore"
            );
            return Ok(account_total);
        }

        // Fallback: compute from the freeze ledger DB (used in some unit tests / legacy paths).
        // Resource types as defined in TRON protocol
        const BANDWIDTH: u8 = 0;
        const ENERGY: u8 = 1;
        const TRON_POWER_LEDGER: u8 = 2;

        let mut total: u64 = 0;
        for resource in [BANDWIDTH, ENERGY, TRON_POWER_LEDGER] {
            if let Some(record) = self.get_freeze_record(address, resource)? {
                total = total.checked_add(record.frozen_amount).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Tron power overflow when adding resource {} amount {} to total {}",
                        resource,
                        record.frozen_amount,
                        total
                    )
                })?;
            }
        }

        tracing::info!(
            address = ?address,
            new_model = new_model,
            total = total,
            "Computed tron power from freeze ledger fallback"
        );

        Ok(total)
    }

    /// Get account name for an address
    pub fn get_account_name(&self, address: &Address) -> Result<Option<String>> {
        let proto_account = match self.get_account_proto(address)? {
            Some(account) => account,
            None => return Ok(None),
        };

        if proto_account.account_name.is_empty() {
            return Ok(None);
        }

        String::from_utf8(proto_account.account_name)
            .map(Some)
            .map_err(|e| anyhow::anyhow!("Invalid UTF-8 in account name: {}", e))
    }

    /// Set account name for an address
    pub fn set_account_name(&mut self, address: Address, name: &[u8]) -> Result<()> {
        const MAX_ACCOUNT_NAME_LEN: usize = 200;
        if name.len() > MAX_ACCOUNT_NAME_LEN {
            return Err(anyhow::anyhow!("Invalid accountName"));
        }

        let mut proto_account = self
            .get_account_proto(&address)?
            .ok_or_else(|| anyhow::anyhow!("Account does not exist"))?;
        proto_account.account_name = name.to_vec();
        self.put_account_proto(&address, &proto_account)?;

        // Java-tron AccountIndexStore is a reverse index: name -> address (21-byte TRON key).
        let tron_address = self.account_key(&address);
        self.buffered_put(self.account_index_database(), name.to_vec(), tron_address)?;

        tracing::info!(
            "Stored account name (len={}) and index entry for address {:?}",
            name.len(),
            address
        );
        Ok(())
    }

    /// Get ALLOW_UPDATE_ACCOUNT_NAME dynamic property.
    ///
    /// Java reference: DynamicPropertiesStore.getAllowUpdateAccountName()
    /// Default: 0 if missing.
    pub fn get_allow_update_account_name(&self) -> Result<i64> {
        let key = b"ALLOW_UPDATE_ACCOUNT_NAME";
        match self.buffered_get(self.dynamic_properties_database(), key)? {
            Some(data) if data.len() >= 8 => Ok(i64::from_be_bytes([
                data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
            ])),
            Some(_) => Ok(0),
            None => Ok(0),
        }
    }

    /// Returns true if `account-index` contains `name` as a key.
    ///
    /// Java reference: AccountIndexStore.has(name_bytes)
    pub fn account_index_has(&self, name: &[u8]) -> Result<bool> {
        Ok(self
            .buffered_get(self.account_index_database(), name)?
            .is_some())
    }

    /// Get database name for account resource tracking (AEXT)
    fn account_aext_database(&self) -> &str {
        db_names::account::ACCOUNT_RESOURCE
    }

    /// Build storage key for account AEXT: 20-byte address
    fn account_aext_key(&self, address: &Address) -> Vec<u8> {
        address.as_slice().to_vec()
    }

    /// Get account AEXT (resource tracking fields) for an address.
    ///
    /// **DEPRECATED (Phase 4)**: This method reads from the legacy `account-resource` DB.
    /// After AEXT migration, use `aext_view_from_account_proto()` instead.
    /// This method is retained only for:
    /// - The offline migrator (`tron-backend-migrate-aext`)
    /// - The lazy backfill mechanism (`lazy_aext_backfill()`)
    ///
    /// Production code should NOT call this directly.
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

    /// Set account AEXT (resource tracking fields) for an address.
    ///
    /// **DEPRECATED (Phase 4)**: This method writes to the legacy `account-resource` DB.
    /// After AEXT migration, use `apply_bandwidth_aext_to_account_proto()` instead.
    /// Production code should NOT call this directly - changes should go directly to
    /// `protocol::Account` via `apply_bandwidth_aext_to_account_proto()`.
    #[deprecated(note = "Use apply_bandwidth_aext_to_account_proto() instead. This writes to legacy account-resource DB.")]
    pub fn set_account_aext(&self, address: &Address, aext: &AccountAext) -> Result<()> {
        let key = self.account_aext_key(address);
        let data = aext.serialize();

        tracing::debug!("Setting account AEXT for address {:?}, net_usage: {}, free_net_usage: {}, net_window: {}",
                       address, aext.net_usage, aext.free_net_usage, aext.net_window_size);

        self.buffered_put(self.account_aext_database(), key, data)?;

        tracing::debug!("Successfully stored account AEXT for address {:?}", address);
        Ok(())
    }

    /// Get or initialize account AEXT with defaults.
    ///
    /// **DEPRECATED (Phase 4)**: This method reads/writes the legacy `account-resource` DB.
    /// After AEXT migration, use `aext_view_from_account_proto()` instead.
    /// Production code should NOT call this directly.
    #[deprecated(note = "Use aext_view_from_account_proto() instead. This uses legacy account-resource DB.")]
    #[allow(deprecated)]
    pub fn get_or_init_account_aext(&self, address: &Address) -> Result<AccountAext> {
        if let Some(aext) = self.get_account_aext(address)? {
            Ok(aext)
        } else {
            let aext = AccountAext::with_defaults();
            self.set_account_aext(address, &aext)?;
            Ok(aext)
        }
    }

    /// Apply bandwidth-related AEXT fields to the Account proto stored in the `account` DB.
    ///
    /// This keeps `protocol::Account` as a usable source-of-truth for:
    /// - `net_usage`, `free_net_usage`
    /// - `latest_consume_time`, `latest_consume_free_time`
    /// - `net_window_size`, `net_window_optimized`
    ///
    /// NOTE: This intentionally does **not** touch energy-related fields since the current
    /// ResourceTracker only updates bandwidth usage.
    ///
    /// Java reference:
    /// - `AccountCapsule.getWindowSize(ResourceCode)` / `getWindowSizeV2(ResourceCode)`
    /// - `WINDOW_SIZE_PRECISION = 1000`
    pub fn apply_bandwidth_aext_to_account_proto(
        &self,
        address: &Address,
        aext: &AccountAext,
    ) -> Result<()> {
        const WINDOW_SIZE_PRECISION: i64 = 1000;

        let Some(mut account) = self.get_account_proto(address)? else {
            return Ok(());
        };

        account.net_usage = aext.net_usage;
        account.free_net_usage = aext.free_net_usage;
        account.latest_consume_time = aext.latest_consume_time;
        account.latest_consume_free_time = aext.latest_consume_free_time;

        account.net_window_optimized = aext.net_window_optimized;
        account.net_window_size = if aext.net_window_size == 0 {
            0
        } else if aext.net_window_optimized {
            aext.net_window_size.saturating_mul(WINDOW_SIZE_PRECISION)
        } else {
            aext.net_window_size
        };

        self.put_account_proto(address, &account)?;
        Ok(())
    }

    // =========================================================================
    // AEXT Migration: Lazy Backfill (Phase 2)
    // =========================================================================
    // When upgrading without running the offline migrator, this provides a safety net.
    // If proto resource fields are default/empty but account-resource has non-default
    // values, we apply AEXT→proto and optionally delete the AEXT key.

    /// Perform lazy backfill from AEXT to Account proto if needed.
    ///
    /// Returns the AccountAext to use (either from backfill or freshly loaded).
    /// If backfill occurred, also deletes the AEXT key.
    ///
    /// This is called at the start of tracked bandwidth accounting to ensure
    /// the proto is up-to-date before we compute resource changes.
    ///
    /// Note: This method intentionally uses the deprecated `get_account_aext()` to
    /// read from the legacy store during the migration transition period.
    #[allow(deprecated)]
    pub fn lazy_aext_backfill(&self, address: &Address) -> Result<AccountAext> {
        // Load current AEXT (if any)
        let aext = match self.get_account_aext(address)? {
            Some(a) => a,
            None => return Ok(AccountAext::with_defaults()),
        };

        // Check if AEXT has non-default values that we should migrate
        let aext_has_data = aext.net_usage != 0
            || aext.free_net_usage != 0
            || aext.latest_consume_time != 0
            || aext.latest_consume_free_time != 0;

        if !aext_has_data {
            return Ok(aext);
        }

        // Load Account proto to check if it needs backfill
        let Some(mut account) = self.get_account_proto(address)? else {
            // No proto exists - just return AEXT, don't create phantom accounts
            return Ok(aext);
        };

        // Check if proto has default/empty resource fields
        let proto_is_default = account.net_usage == 0
            && account.free_net_usage == 0
            && account.latest_consume_time == 0
            && account.latest_consume_free_time == 0;

        if !proto_is_default {
            // Proto already has values, no backfill needed
            return Ok(aext);
        }

        // Perform backfill: apply AEXT to proto
        tracing::info!(
            "Lazy AEXT backfill for {:?}: net_usage={}, free_net_usage={}, latest_consume_time={}",
            address,
            aext.net_usage,
            aext.free_net_usage,
            aext.latest_consume_time
        );

        account.net_usage = aext.net_usage;
        account.free_net_usage = aext.free_net_usage;
        account.latest_consume_time = aext.latest_consume_time;
        account.latest_consume_free_time = aext.latest_consume_free_time;

        // Apply window fields only if proto is 0
        if account.net_window_size == 0 && aext.net_window_size != 0 {
            const WINDOW_SIZE_PRECISION: i64 = 1000;
            account.net_window_size = if aext.net_window_optimized {
                aext.net_window_size.saturating_mul(WINDOW_SIZE_PRECISION)
            } else {
                aext.net_window_size
            };
            account.net_window_optimized = aext.net_window_optimized;
        }

        // Write updated proto
        self.put_account_proto(address, &account)?;

        // Delete the AEXT key (migration complete for this account)
        let aext_key = self.account_aext_key(address);
        self.storage_engine.delete(self.account_aext_database(), &aext_key)?;

        tracing::debug!("Lazy backfill complete, AEXT key deleted for {:?}", address);

        Ok(aext)
    }

    /// Build an AccountAext view from a protocol::Account proto.
    ///
    /// This is used after Phase 3 when we no longer read from account-resource,
    /// and need to derive the AEXT "view" from the canonical Account proto.
    pub fn aext_view_from_account_proto(&self, address: &Address) -> Result<AccountAext> {
        let Some(account) = self.get_account_proto(address)? else {
            return Ok(AccountAext::with_defaults());
        };

        const WINDOW_SIZE_PRECISION: i64 = 1000;

        // Normalize window size from raw proto value to logical slots
        let net_window_size = if account.net_window_optimized && account.net_window_size > 0 {
            account.net_window_size / WINDOW_SIZE_PRECISION
        } else if account.net_window_size > 0 {
            account.net_window_size
        } else {
            28800 // Default window size
        };

        // Energy window (if AccountResource present)
        let (energy_usage, latest_consume_time_for_energy, energy_window_size, energy_window_optimized) =
            if let Some(ref ar) = account.account_resource {
                let ews = if ar.energy_window_optimized && ar.energy_window_size > 0 {
                    ar.energy_window_size / WINDOW_SIZE_PRECISION
                } else if ar.energy_window_size > 0 {
                    ar.energy_window_size
                } else {
                    28800
                };
                (ar.energy_usage, ar.latest_consume_time_for_energy, ews, ar.energy_window_optimized)
            } else {
                (0, 0, 28800, false)
            };

        Ok(AccountAext {
            net_usage: account.net_usage,
            free_net_usage: account.free_net_usage,
            energy_usage,
            latest_consume_time: account.latest_consume_time,
            latest_consume_free_time: account.latest_consume_free_time,
            latest_consume_time_for_energy,
            net_window_size,
            net_window_optimized: account.net_window_optimized,
            energy_window_size,
            energy_window_optimized,
        })
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
        key.push(self.address_prefix);
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
        self.buffered_put(self.delegation_database(), key, data.to_vec())?;
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
        self.buffered_put(self.delegation_database(), key, data.to_vec())?;
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
        self.buffered_put(self.delegation_database(), key, data)?;
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
        self.buffered_put(self.delegation_database(), key, data.to_vec())?;
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
        let data = self.encode_proposal_java_compatible(proposal);

        tracing::debug!(
            "Storing proposal {} - proposer: {}, state: {:?}, approvals: {}, key: {}",
            proposal.proposal_id,
            hex::encode(&proposal.proposer_address),
            proposal.state,
            proposal.approvals.len(),
            hex::encode(&key)
        );

        self.buffered_put(self.proposal_database(), key, data)?;
        Ok(())
    }

    /// Encode a `Proposal` exactly the way java-tron persists it.
    ///
    /// `prost`'s default map encoding omits map-entry keys when the key is `0`, but java-tron
    /// includes the key field (`08 00`) in that case. Conformance fixtures assert raw DB bytes,
    /// so proposal persistence must be byte-for-byte compatible.
    fn encode_proposal_java_compatible(&self, proposal: &crate::protocol::Proposal) -> Vec<u8> {
        let mut out = Vec::new();

        // Field 1: proposal_id (int64, varint)
        if proposal.proposal_id != 0 {
            self.write_varint(&mut out, (1 << 3) | 0);
            self.write_varint(&mut out, proposal.proposal_id as u64);
        }

        // Field 2: proposer_address (bytes)
        if !proposal.proposer_address.is_empty() {
            self.write_varint(&mut out, (2 << 3) | 2);
            self.write_varint(&mut out, proposal.proposer_address.len() as u64);
            out.extend_from_slice(&proposal.proposer_address);
        }

        // Field 3: parameters (map<int64,int64>) - entries are encoded in ascending key order.
        if !proposal.parameters.is_empty() {
            let mut entries: Vec<(i64, i64)> = proposal
                .parameters
                .iter()
                .map(|(k, v)| (*k, *v))
                .collect();
            entries.sort_by_key(|(k, _)| *k);

            for (key, value) in entries {
                let mut entry_buf = Vec::new();
                // Map entry field 1: key (int64, varint) - ALWAYS encoded (even if 0).
                self.write_varint(&mut entry_buf, (1 << 3) | 0);
                self.write_varint(&mut entry_buf, key as u64);
                // Map entry field 2: value (int64, varint) - encoded for parity with java-tron.
                self.write_varint(&mut entry_buf, (2 << 3) | 0);
                self.write_varint(&mut entry_buf, value as u64);

                self.write_varint(&mut out, (3 << 3) | 2);
                self.write_varint(&mut out, entry_buf.len() as u64);
                out.extend_from_slice(&entry_buf);
            }
        }

        // Field 4: expiration_time (int64, varint)
        if proposal.expiration_time != 0 {
            self.write_varint(&mut out, (4 << 3) | 0);
            self.write_varint(&mut out, proposal.expiration_time as u64);
        }

        // Field 5: create_time (int64, varint)
        if proposal.create_time != 0 {
            self.write_varint(&mut out, (5 << 3) | 0);
            self.write_varint(&mut out, proposal.create_time as u64);
        }

        // Field 6: approvals (repeated bytes)
        for approval in &proposal.approvals {
            if approval.is_empty() {
                continue;
            }
            self.write_varint(&mut out, (6 << 3) | 2);
            self.write_varint(&mut out, approval.len() as u64);
            out.extend_from_slice(approval);
        }

        // Field 7: state (enum, varint) - proto3 omits default 0.
        if proposal.state != 0 {
            self.write_varint(&mut out, (7 << 3) | 0);
            self.write_varint(&mut out, proposal.state as u64);
        }

        out
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
        self.buffered_put(self.dynamic_properties_database(), key.to_vec(), data.to_vec())?;
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

    /// Get REMOVE_THE_POWER_OF_THE_GR dynamic property.
    ///
    /// java-tron uses this as a tri-state flag:
    /// - 0: not yet executed
    /// - 1: enabled
    /// - -1: executed (one-time proposal)
    ///
    /// Default: 0 if not found.
    pub fn get_remove_the_power_of_the_gr(&self) -> Result<i64> {
        let key = b"REMOVE_THE_POWER_OF_THE_GR";
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
            }
            None => Ok(0),
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

        self.buffered_put(self.account_id_index_database(), key, address.to_vec())?;
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
    /// Java throws `IllegalArgumentException("not found TOTAL_SIGN_NUM")` if missing.
    ///
    /// Note: Java stores this as 4-byte int (ByteArray.fromInt), not 8-byte long.
    /// Java's ByteArray.toInt uses BigInteger(1, b).intValue() which:
    /// - Returns 0 for empty arrays
    /// - Interprets bytes as unsigned big-endian
    /// - Truncates to low 32 bits (equivalent to taking last 4 bytes for len >= 4)
    pub fn get_total_sign_num(&self) -> Result<i64> {
        let key = b"TOTAL_SIGN_NUM";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                // Match Java's ByteArray.toInt(byte[] b) exactly:
                // return ArrayUtils.isEmpty(b) ? 0 : new BigInteger(1, b).intValue();
                let value = if data.is_empty() {
                    // Java's toInt returns 0 for empty arrays
                    0i64
                } else if data.len() >= 4 {
                    // BigInteger(1, b).intValue() returns low 32 bits of unsigned big-endian.
                    // For len >= 4, this is equivalent to taking the last 4 bytes.
                    let start = data.len() - 4;
                    let last_4 = [data[start], data[start + 1], data[start + 2], data[start + 3]];
                    // Interpret as unsigned u32, cast to i32 for signed semantics, then to i64
                    let unsigned_val = u32::from_be_bytes(last_4);
                    (unsigned_val as i32) as i64
                } else {
                    // For len < 4, interpret as unsigned big-endian (fits in i32)
                    let mut val: i64 = 0;
                    for &byte in &data {
                        val = (val << 8) | (byte as i64);
                    }
                    val
                };
                tracing::debug!("TOTAL_SIGN_NUM: {} (from {} bytes)", value, data.len());
                Ok(value)
            }
            None => {
                // Java throws IllegalArgumentException when key is missing
                Err(anyhow::anyhow!("not found TOTAL_SIGN_NUM"))
            }
        }
    }

    /// Get UPDATE_ACCOUNT_PERMISSION_FEE dynamic property
    /// Fee in SUN for updating account permissions
    /// Java throws `IllegalArgumentException("not found UPDATE_ACCOUNT_PERMISSION_FEE")` if missing.
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
                    // Invalid length treated as missing for strict parity
                    tracing::warn!("UPDATE_ACCOUNT_PERMISSION_FEE has invalid length: {}", data.len());
                    Err(anyhow::anyhow!("not found UPDATE_ACCOUNT_PERMISSION_FEE"))
                }
            }
            None => {
                // Java throws IllegalArgumentException when key is missing
                Err(anyhow::anyhow!("not found UPDATE_ACCOUNT_PERMISSION_FEE"))
            }
        }
    }

    /// Get AVAILABLE_CONTRACT_TYPE dynamic property
    /// Bitmap of allowed contract types (32 bytes)
    /// Java throws `IllegalArgumentException("not found AVAILABLE_CONTRACT_TYPE")` if missing.
    pub fn get_available_contract_type(&self) -> Result<Vec<u8>> {
        let key = b"AVAILABLE_CONTRACT_TYPE";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                tracing::debug!("AVAILABLE_CONTRACT_TYPE: {} bytes", data.len());
                Ok(data)
            }
            None => {
                // Java throws IllegalArgumentException when key is missing
                Err(anyhow::anyhow!("not found AVAILABLE_CONTRACT_TYPE"))
            }
        }
    }

    /// Get the blackhole address as 21-byte TRON format (0x41 prefix + 20 bytes)
    /// Java: AccountStore.getBlackhole() returns BURN_ADDRESS or HOLE_ADDRESS
    pub fn get_blackhole_address_tron(&self) -> [u8; 21] {
        // Mainnet default: TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy
        // Testnet default (config-test.conf genesis): 27WtBq2KoSy5v8VnVZBZHHJcDuWNiSgjbE3
        match self.address_prefix {
            0xa0 => [
                0xa0,
                0x55, 0x9c, 0xcf, 0x55, 0xfa, 0xdf, 0xfd, 0xf8,
                0x14, 0xa4, 0x2a, 0xff, 0x33, 0x1d, 0xe9, 0x68,
                0x8c, 0x13, 0x26, 0x12,
            ],
            _ => [
                0x41,
                0x77, 0x94, 0x4d, 0x19, 0xc0, 0x52, 0xb7, 0x3e,
                0xe2, 0x28, 0x68, 0x23, 0xaa, 0x83, 0xf8, 0x13,
                0x8c, 0xb7, 0x03, 0x2f,
            ],
        }
    }

    /// Get the blackhole address as an EVM Address (20 bytes, no 0x41 prefix)
    pub fn get_blackhole_address_evm(&self) -> Address {
        let tron = self.get_blackhole_address_tron();
        Address::from_slice(&tron[1..])
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

        self.buffered_put(self.contract_database(), key.clone(), buf)?;
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

        self.buffered_put(self.abi_database(), contract_address.to_vec(), buf)?;
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
    //   UNLOCK_PREFIX (0x01) + from_address (21 bytes) + to_address (21 bytes) = 43 bytes
    //   LOCK_PREFIX   (0x02) + from_address (21 bytes) + to_address (21 bytes) = 43 bytes
    //
    // Key format for DelegatedResourceAccountIndexStore (V2):
    //   FROM_PREFIX (0x03) + from_address (21 bytes) + to_address (21 bytes) = 43 bytes
    //   TO_PREFIX   (0x04) + to_address (21 bytes) + from_address (21 bytes) = 43 bytes

    /// Get the database name for DelegatedResource store
    fn delegated_resource_database(&self) -> &str {
        db_names::delegation::DELEGATED_RESOURCE
    }

    /// Get the database name for DelegatedResourceAccountIndex store
    fn delegated_resource_account_index_database(&self) -> &str {
        db_names::delegation::DELEGATED_RESOURCE_ACCOUNT_INDEX
    }

    /// Create V1 key for DelegatedResource store (from -> to).
    /// Java reference: DelegatedResourceCapsule.createDbKey(from, to)
    fn delegated_resource_key_v1(&self, from: &Address, to: &Address) -> Vec<u8> {
        let from_tron = self.to_tron_address_21(from);
        let to_tron = self.to_tron_address_21(to);
        let mut key = Vec::with_capacity(42);
        key.extend_from_slice(&from_tron);
        key.extend_from_slice(&to_tron);
        key
    }

    /// Create V2 key for DelegatedResource store (from -> to).
    /// Lock semantics match Java: lock=false uses 0x01 prefix, lock=true uses 0x02 prefix.
    fn delegated_resource_key_v2(&self, from: &Address, to: &Address, lock: bool) -> Vec<u8> {
        use super::key_helpers::delegated_resource;
        let from_tron = self.to_tron_address_21(from);
        let to_tron = self.to_tron_address_21(to);
        delegated_resource::create_db_key_v2(&from_tron, &to_tron, lock)
    }

    /// Convert 20-byte EVM address to 21-byte TRON address
    pub fn to_tron_address_21(&self, address: &Address) -> [u8; 21] {
        let mut tron_addr = [0u8; 21];
        tron_addr[0] = self.address_prefix;
        tron_addr[1..].copy_from_slice(address.as_slice());
        tron_addr
    }

    /// Get a DelegatedResource record for V1 delegation (FreezeBalanceContract delegation).
    pub fn get_delegated_resource_v1(
        &self,
        owner: &Address,
        receiver: &Address,
    ) -> Result<Option<crate::protocol::DelegatedResource>> {
        let key = self.delegated_resource_key_v1(owner, receiver);
        match self.buffered_get(self.delegated_resource_database(), &key)? {
            Some(data) => {
                let dr = crate::protocol::DelegatedResource::decode(data.as_slice())
                    .map_err(|e| anyhow::anyhow!("Failed to decode DelegatedResource: {}", e))?;
                Ok(Some(dr))
            }
            None => Ok(None),
        }
    }

    /// Delegate resource (V1 semantics, used by FreezeBalanceContract delegation).
    ///
    /// Java oracle: FreezeBalanceActuator#delegateResource (DelegatedResourceCapsule.createDbKey)
    pub fn delegate_resource_v1(
        &self,
        owner: &Address,
        receiver: &Address,
        is_bandwidth: bool,
        balance: i64,
        expire_time: i64,
    ) -> Result<()> {
        let key = self.delegated_resource_key_v1(owner, receiver);
        let owner_tron = self.to_tron_address_21(owner);
        let receiver_tron = self.to_tron_address_21(receiver);

        let mut dr = match self.buffered_get(self.delegated_resource_database(), &key)? {
            Some(data) => crate::protocol::DelegatedResource::decode(&data[..])
                .map_err(|e| anyhow::anyhow!("Failed to decode DelegatedResource: {}", e))?,
            None => crate::protocol::DelegatedResource {
                from: owner_tron.to_vec(),
                to: receiver_tron.to_vec(),
                ..Default::default()
            },
        };

        if is_bandwidth {
            dr.frozen_balance_for_bandwidth = dr
                .frozen_balance_for_bandwidth
                .checked_add(balance)
                .ok_or_else(|| anyhow::anyhow!("Overflow updating frozen_balance_for_bandwidth"))?;
            dr.expire_time_for_bandwidth = expire_time;
        } else {
            dr.frozen_balance_for_energy = dr
                .frozen_balance_for_energy
                .checked_add(balance)
                .ok_or_else(|| anyhow::anyhow!("Overflow updating frozen_balance_for_energy"))?;
            dr.expire_time_for_energy = expire_time;
        }

        self.buffered_put(
            self.delegated_resource_database(),
            key,
            dr.encode_to_vec(),
        )?;

        Ok(())
    }

    /// Update DelegatedResourceAccountIndex for V1 delegation (FreezeBalanceContract delegation).
    ///
    /// Java oracle: FreezeBalanceActuator#delegateResource when supportAllowDelegateOptimization == false.
    pub fn delegate_resource_account_index_v1(&self, owner: &Address, receiver: &Address) -> Result<()> {
        let owner_tron = self.to_tron_address_21(owner).to_vec();
        let receiver_tron = self.to_tron_address_21(receiver).to_vec();

        // Owner index: add receiver to to_accounts.
        let owner_key = owner_tron.clone();
        let mut owner_index = match self
            .buffered_get(self.delegated_resource_account_index_database(), &owner_key)?
        {
            Some(data) => crate::protocol::DelegatedResourceAccountIndex::decode(&data[..])
                .map_err(|e| anyhow::anyhow!("Failed to decode DelegatedResourceAccountIndex: {}", e))?,
            None => crate::protocol::DelegatedResourceAccountIndex {
                account: owner_tron.clone(),
                from_accounts: Vec::new(),
                to_accounts: Vec::new(),
                ..Default::default()
            },
        };

        if !owner_index.to_accounts.iter().any(|a| a == &receiver_tron) {
            owner_index.to_accounts.push(receiver_tron.clone());
        }

        self.buffered_put(
            self.delegated_resource_account_index_database(),
            owner_key,
            owner_index.encode_to_vec(),
        )?;

        // Receiver index: add owner to from_accounts.
        let receiver_key = receiver_tron.clone();
        let mut receiver_index = match self
            .buffered_get(self.delegated_resource_account_index_database(), &receiver_key)?
        {
            Some(data) => crate::protocol::DelegatedResourceAccountIndex::decode(&data[..])
                .map_err(|e| anyhow::anyhow!("Failed to decode DelegatedResourceAccountIndex: {}", e))?,
            None => crate::protocol::DelegatedResourceAccountIndex {
                account: receiver_tron.clone(),
                from_accounts: Vec::new(),
                to_accounts: Vec::new(),
                ..Default::default()
            },
        };

        if !receiver_index.from_accounts.iter().any(|a| a == &owner_tron) {
            receiver_index.from_accounts.push(owner_tron);
        }

        self.buffered_put(
            self.delegated_resource_account_index_database(),
            receiver_key,
            receiver_index.encode_to_vec(),
        )?;

        Ok(())
    }

    /// Store a DelegatedResource record for V1 delegation.
    pub fn put_delegated_resource_v1(
        &self,
        owner: &Address,
        receiver: &Address,
        resource: &crate::protocol::DelegatedResource,
    ) -> Result<()> {
        let key = self.delegated_resource_key_v1(owner, receiver);
        self.buffered_put(
            self.delegated_resource_database(),
            key,
            resource.encode_to_vec(),
        )?;
        Ok(())
    }

    /// Delete a DelegatedResource record for V1 delegation.
    pub fn delete_delegated_resource_v1(&self, owner: &Address, receiver: &Address) -> Result<()> {
        let key = self.delegated_resource_key_v1(owner, receiver);
        self.buffered_delete(self.delegated_resource_database(), key)?;
        Ok(())
    }

    /// Remove DelegatedResourceAccountIndex entries for V1 delegation (UnfreezeBalanceContract).
    ///
    /// Java oracle: UnfreezeBalanceActuator#execute when supportAllowDelegateOptimization == false.
    pub fn undelegate_resource_account_index_v1(&self, owner: &Address, receiver: &Address) -> Result<()> {
        let owner_tron = self.to_tron_address_21(owner).to_vec();
        let receiver_tron = self.to_tron_address_21(receiver).to_vec();

        // Owner index: remove receiver from to_accounts.
        let owner_key = owner_tron.clone();
        if let Some(data) = self
            .buffered_get(self.delegated_resource_account_index_database(), &owner_key)?
        {
            let mut owner_index = crate::protocol::DelegatedResourceAccountIndex::decode(&data[..])
                .map_err(|e| anyhow::anyhow!("Failed to decode DelegatedResourceAccountIndex: {}", e))?;
            owner_index.to_accounts.retain(|a| a != &receiver_tron);
            self.buffered_put(
                self.delegated_resource_account_index_database(),
                owner_key,
                owner_index.encode_to_vec(),
            )?;
        }

        // Receiver index: remove owner from from_accounts.
        let receiver_key = receiver_tron.clone();
        if let Some(data) = self
            .buffered_get(self.delegated_resource_account_index_database(), &receiver_key)?
        {
            let mut receiver_index = crate::protocol::DelegatedResourceAccountIndex::decode(&data[..])
                .map_err(|e| anyhow::anyhow!("Failed to decode DelegatedResourceAccountIndex: {}", e))?;
            receiver_index.from_accounts.retain(|a| a != &owner_tron);
            self.buffered_put(
                self.delegated_resource_account_index_database(),
                receiver_key,
                receiver_index.encode_to_vec(),
            )?;
        }

        Ok(())
    }

    /// Get a DelegatedResource record (lock/unlock) without mutating state.
    pub fn get_delegated_resource(
        &self,
        owner: &Address,
        receiver: &Address,
        lock: bool,
    ) -> Result<Option<crate::protocol::DelegatedResource>> {
        let key = self.delegated_resource_key_v2(owner, receiver, lock);
        match self.buffered_get(self.delegated_resource_database(), &key)? {
            Some(data) => {
                let dr = crate::protocol::DelegatedResource::decode(data.as_slice())
                    .map_err(|e| anyhow::anyhow!("Failed to decode DelegatedResource: {}", e))?;
                Ok(Some(dr))
            }
            None => Ok(None),
        }
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
        // Java parity: unlock expired locked balances before mutating records.
        let now = self.get_latest_block_header_timestamp()?;
        self.unlock_expired_delegated_resource(owner, receiver, now)?;

        let key = self.delegated_resource_key_v2(owner, receiver, lock);
        tracing::debug!("Delegating resource: from={}, to={}, is_bw={}, balance={}, lock={}, expire={}",
                       hex::encode(owner), hex::encode(receiver), is_bandwidth, balance, lock, expire_time);

        // Get or create DelegatedResource
        let mut dr = match self.buffered_get(self.delegated_resource_database(), &key)? {
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
            dr.expire_time_for_bandwidth = expire_time;
        } else {
            dr.frozen_balance_for_energy += balance;
            dr.expire_time_for_energy = expire_time;
        }

        // Persist
        let data = dr.encode_to_vec();
        self.buffered_put(self.delegated_resource_database(), key, data)?;
        Ok(())
    }

    /// Java parity: `DelegatedResourceStore.unLockExpireResource(from, to, now)`
    /// Moves expired balances from the lock record (0x02) to the unlock record (0x01).
    pub fn unlock_expired_delegated_resource(&self, from: &Address, to: &Address, now: i64) -> Result<()> {
        let lock_key = self.delegated_resource_key_v2(from, to, true);
        let unlock_key = self.delegated_resource_key_v2(from, to, false);

        let Some(lock_data) = self.buffered_get(self.delegated_resource_database(), &lock_key)? else {
            return Ok(());
        };

        let mut lock_resource = crate::protocol::DelegatedResource::decode(&lock_data[..])
            .map_err(|e| anyhow::anyhow!("Failed to decode DelegatedResource lock record: {}", e))?;

        // If neither resource has expired, no-op.
        if lock_resource.expire_time_for_energy >= now && lock_resource.expire_time_for_bandwidth >= now {
            return Ok(());
        }

        let mut unlock_resource = match self.buffered_get(self.delegated_resource_database(), &unlock_key)? {
            Some(data) => crate::protocol::DelegatedResource::decode(&data[..])
                .map_err(|e| anyhow::anyhow!("Failed to decode DelegatedResource unlock record: {}", e))?,
            None => crate::protocol::DelegatedResource {
                from: self.to_tron_address_21(from).to_vec(),
                to: self.to_tron_address_21(to).to_vec(),
                frozen_balance_for_bandwidth: 0,
                frozen_balance_for_energy: 0,
                expire_time_for_bandwidth: 0,
                expire_time_for_energy: 0,
            },
        };

        if lock_resource.expire_time_for_energy < now {
            unlock_resource.frozen_balance_for_energy += lock_resource.frozen_balance_for_energy;
            unlock_resource.expire_time_for_energy = 0;
            lock_resource.frozen_balance_for_energy = 0;
            lock_resource.expire_time_for_energy = 0;
        }

        if lock_resource.expire_time_for_bandwidth < now {
            unlock_resource.frozen_balance_for_bandwidth += lock_resource.frozen_balance_for_bandwidth;
            unlock_resource.expire_time_for_bandwidth = 0;
            lock_resource.frozen_balance_for_bandwidth = 0;
            lock_resource.expire_time_for_bandwidth = 0;
        }

        if lock_resource.frozen_balance_for_bandwidth == 0 && lock_resource.frozen_balance_for_energy == 0 {
            self.buffered_delete(self.delegated_resource_database(), lock_key)?;
        } else {
            self.buffered_put(
                self.delegated_resource_database(),
                lock_key,
                lock_resource.encode_to_vec(),
            )?;
        }

        self.buffered_put(
            self.delegated_resource_database(),
            unlock_key,
            unlock_resource.encode_to_vec(),
        )?;

        Ok(())
    }

    /// Undelegate resource (reclaim from receiver back to owner)
    pub fn undelegate_resource(
        &self,
        owner: &Address,
        receiver: &Address,
        is_bandwidth: bool,
        balance: i64,
        now: i64,
    ) -> Result<()> {
        // Java parity: transfer expired locked balances to the unlock record before mutating.
        self.unlock_expired_delegated_resource(owner, receiver, now)?;

        let key = self.delegated_resource_key_v2(owner, receiver, false);
        tracing::debug!("Undelegating resource: from={}, to={}, is_bw={}, balance={}",
                       hex::encode(owner), hex::encode(receiver), is_bandwidth, balance);

        // Get existing DelegatedResource
        let data = self.buffered_get(self.delegated_resource_database(), &key)?
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
            self.buffered_delete(self.delegated_resource_database(), key)?;
        } else {
            let data = dr.encode_to_vec();
            self.buffered_put(self.delegated_resource_database(), key, data)?;
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
        // Matches Java validate() logic:
        // - Always include unlocked balance (prefix 0x01)
        // - Include locked balance (prefix 0x02) only if expired for the resource type
        let unlock_key = self.delegated_resource_key_v2(owner, receiver, false);
        let lock_key = self.delegated_resource_key_v2(owner, receiver, true);

        let mut balance = 0i64;

        if let Some(data) = self.buffered_get(self.delegated_resource_database(), &unlock_key)? {
            let dr = crate::protocol::DelegatedResource::decode(&data[..])
                .map_err(|e| anyhow::anyhow!("Failed to decode DelegatedResource: {}", e))?;
            if is_bandwidth {
                balance += dr.frozen_balance_for_bandwidth;
            } else {
                balance += dr.frozen_balance_for_energy;
            }
        }

        if let Some(data) = self.buffered_get(self.delegated_resource_database(), &lock_key)? {
            let dr = crate::protocol::DelegatedResource::decode(&data[..])
                .map_err(|e| anyhow::anyhow!("Failed to decode DelegatedResource: {}", e))?;
            if is_bandwidth {
                if dr.expire_time_for_bandwidth < now {
                    balance += dr.frozen_balance_for_bandwidth;
                }
            } else if dr.expire_time_for_energy < now {
                balance += dr.frozen_balance_for_energy;
            }
        }

        Ok(balance)
    }

    /// Update DelegatedResourceAccountIndex for a delegation.
    ///
    /// Matches Java `DelegatedResourceAccountIndexStore.delegateV2()` semantics:
    /// - 0x03 + from + to  -> { account=to, timestamp }
    /// - 0x04 + to + from  -> { account=from, timestamp }
    pub fn delegate_resource_account_index(
        &self,
        owner: &Address,
        receiver: &Address,
        timestamp: i64,
    ) -> Result<()> {
        use super::key_helpers::delegated_resource_account_index;

        let owner_tron = self.to_tron_address_21(owner);
        let receiver_tron = self.to_tron_address_21(receiver);

        let from_key = delegated_resource_account_index::create_db_key_v2_from(&owner_tron, &receiver_tron);
        let to_key = delegated_resource_account_index::create_db_key_v2_to(&owner_tron, &receiver_tron);

        let to_index = crate::protocol::DelegatedResourceAccountIndex {
            account: receiver_tron.to_vec(),
            from_accounts: Vec::new(),
            to_accounts: Vec::new(),
            timestamp,
        };

        let from_index = crate::protocol::DelegatedResourceAccountIndex {
            account: owner_tron.to_vec(),
            from_accounts: Vec::new(),
            to_accounts: Vec::new(),
            timestamp,
        };

        self.buffered_put(
            self.delegated_resource_account_index_database(),
            from_key,
            to_index.encode_to_vec(),
        )?;
        self.buffered_put(
            self.delegated_resource_account_index_database(),
            to_key,
            from_index.encode_to_vec(),
        )?;

        Ok(())
    }

    /// Remove DelegatedResourceAccountIndex entries for a delegation (unDelegateV2).
    pub fn undelegate_resource_account_index(&self, owner: &Address, receiver: &Address) -> Result<()> {
        use super::key_helpers::delegated_resource_account_index;

        let owner_tron = self.to_tron_address_21(owner);
        let receiver_tron = self.to_tron_address_21(receiver);

        let from_key = delegated_resource_account_index::create_db_key_v2_from(&owner_tron, &receiver_tron);
        let to_key = delegated_resource_account_index::create_db_key_v2_to(&owner_tron, &receiver_tron);

        self.buffered_delete(self.delegated_resource_account_index_database(), from_key)?;
        self.buffered_delete(self.delegated_resource_account_index_database(), to_key)?;

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

    /// Get ALLOW_TVM_SOLIDITY_059 dynamic property
    /// Returns 0 if Solidity 0.5.9 features are not enabled, non-zero if enabled.
    /// Default: 0 (not enabled)
    pub fn get_allow_tvm_solidity059(&self) -> Result<i64> {
        let key = b"ALLOW_TVM_SOLIDITY_059";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let value = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]);
                    tracing::debug!("ALLOW_TVM_SOLIDITY_059: {}", value);
                    Ok(value)
                } else {
                    tracing::warn!("ALLOW_TVM_SOLIDITY_059 has invalid length: {}", data.len());
                    Ok(0)
                }
            }
            None => {
                tracing::debug!("ALLOW_TVM_SOLIDITY_059 not found, returning 0");
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

        match self.buffered_get(self.account_database(), &key)? {
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
        match self.buffered_get(self.code_database(), &key)? {
            Some(data) => Ok((!data.is_empty()).then(|| Bytecode::new_raw(data.into()))),
            None => Ok(None),
        }
    }

    fn get_storage(&self, address: &Address, key: &U256) -> Result<U256> {
        let storage_key = self.contract_storage_key(address, key);
        match self.buffered_get(self.storage_row_database(), &storage_key)? {
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
        let existing_data = self.buffered_get(self.account_database(), &key)?;

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

        self.buffered_put(self.account_database(), key.clone(), data.clone())?;

        // Verify the write by reading it back (only when not using write buffer,
        // since buffered writes aren't visible until commit)
        if !self.has_write_buffer() {
            if let Ok(Some(read_data)) = self.storage_engine.get(self.account_database(), &key) {
                if read_data == data {
                    tracing::debug!("Verified account write for {} - data matches", address_tron);
                } else {
                    tracing::error!("Account write verification failed for {} - data mismatch!", address_tron);
                }
            } else {
                tracing::error!("Account write verification failed for {} - could not read back!", address_tron);
            }
        }

        Ok(())
    }

    fn set_code(&mut self, address: Address, code: Bytecode) -> Result<()> {
        let key = self.code_key(&address);
        // Persist the original contract bytecode (no analysis padding).
        //
        // REVM's analyzed legacy bytecode includes 33 bytes of zero padding for faster
        // interpreter execution. java-tron's CodeStore stores the raw runtime bytecode,
        // so we must persist the unpadded original bytes for conformance parity.
        let code_bytes = code.original_byte_slice();
        if code_bytes.is_empty() {
            // Don't store empty code blobs; absence in CodeStore represents empty code.
            return Ok(());
        }
        self.buffered_put(self.code_database(), key, code_bytes.to_vec())?;
        Ok(())
    }

    fn set_storage(&mut self, address: Address, key: U256, value: U256) -> Result<()> {
        let storage_key = self.contract_storage_key(&address, &key);
        if value == U256::ZERO {
            // TRON parity: Contract storage is a sparse KV store. Zero values are represented
            // by the absence of a key (deletion), not by an explicit 32-byte zero blob.
            self.buffered_delete(self.storage_row_database(), storage_key)?;
        } else {
            let data = value.to_be_bytes::<32>();
            self.buffered_put(self.storage_row_database(), storage_key, data.to_vec())?;
        }
        Ok(())
    }

    fn remove_account(&mut self, address: &Address) -> Result<()> {
        // Remove account data
        let account_key = self.account_key(address);
        self.buffered_delete(self.account_database(), account_key)?;

        // Remove code
        let code_key = self.code_key(address);
        self.buffered_delete(self.code_database(), code_key)?;

        // Note: We don't remove storage slots here as it would require iteration
        // In a real implementation, we might want to track storage slots separately
        // or use a different key scheme that allows prefix deletion

        Ok(())
    }

    fn tvm_spec_id(&self) -> Result<Option<revm::primitives::SpecId>> {
        use revm::primitives::SpecId;

        let read_flag = |key: &[u8]| -> Result<Option<i64>> {
            match self.storage_engine.get(self.dynamic_properties_database(), key)? {
                Some(data) => {
                    if data.len() >= 8 {
                        Ok(Some(i64::from_be_bytes([
                            data[0], data[1], data[2], data[3],
                            data[4], data[5], data[6], data[7],
                        ])))
                    } else if !data.is_empty() {
                        Ok(Some(data[0] as i64))
                    } else {
                        Ok(Some(0))
                    }
                }
                None => Ok(None),
            }
        };

        let london = read_flag(b"ALLOW_TVM_LONDON")?;
        let istanbul = read_flag(b"ALLOW_TVM_ISTANBUL")?;
        let constantinople = read_flag(b"ALLOW_TVM_CONSTANTINOPLE")?;

        let has_any = london.is_some() || istanbul.is_some() || constantinople.is_some();
        if !has_any {
            return Ok(None);
        }

        let london_enabled = london.unwrap_or(0) != 0;
        let istanbul_enabled = istanbul.unwrap_or(0) != 0;
        let spec_id = if london_enabled {
            SpecId::LONDON
        } else if istanbul_enabled {
            SpecId::ISTANBUL
        } else {
            // TVM energy accounting matches Constantinople-era net gas metering (EIP-1283).
            SpecId::CONSTANTINOPLE
        };

        Ok(Some(spec_id))
    }

    fn energy_fee_rate(&self) -> Result<Option<u64>> {
        let fee = self.get_energy_fee()?;
        if fee == 0 {
            Ok(None)
        } else {
            Ok(Some(fee))
        }
    }

    fn tron_address_prefix(&self) -> Result<u8> {
        Ok(self.address_prefix)
    }

    fn tron_dynamic_property_i64(&self, key: &[u8]) -> Result<Option<i64>> {
        match self.buffered_get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(Some(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ])))
                } else if !data.is_empty() {
                    Ok(Some(data[0] as i64))
                } else {
                    Ok(Some(0))
                }
            }
            None => Ok(None),
        }
    }

    fn tron_get_asset_issue(
        &self,
        key: &[u8],
        allow_same_token_name: i64,
    ) -> Result<Option<crate::protocol::AssetIssueContractData>> {
        EngineBackedEvmStateStore::get_asset_issue(self, key, allow_same_token_name)
    }

    fn tron_get_asset_balance_v2(&self, address: &Address, token_id: &[u8]) -> Result<i64> {
        EngineBackedEvmStateStore::get_asset_balance_v2(self, address, token_id)
    }

    fn tron_has_smart_contract(&self, contract_address: &[u8]) -> Result<Option<bool>> {
        Ok(Some(
            self.buffered_get(self.contract_database(), contract_address)?
                .is_some(),
        ))
    }
}

// ==========================================================================
// Phase 2.E: TRC-10 Extension Storage Methods
// ==========================================================================

impl EngineBackedEvmStateStore {
    /// Get asset issue store database name
    fn asset_issue_database(&self) -> &str {
        db_names::asset::ASSET_ISSUE
    }

    /// Get asset issue V2 store database name
    fn asset_issue_v2_database(&self) -> &str {
        db_names::asset::ASSET_ISSUE_V2
    }

    /// Get AllowSameTokenName dynamic property
    /// If 0: use asset name as key for AssetIssueStore
    /// If 1: use asset id as key for AssetIssueV2Store
    ///
    /// Java-tron requires this key to exist in DynamicPropertiesStore and will throw if missing.
    /// For backend robustness (and historical replay parity), default to 0 when absent to avoid
    /// enabling V2 mode prematurely.
    pub fn get_allow_same_token_name(&self) -> Result<i64> {
        // Note: java-tron stores this under a key with a leading space:
        //   private static final byte[] ALLOW_SAME_TOKEN_NAME = " ALLOW_SAME_TOKEN_NAME".getBytes();
        let key = b" ALLOW_SAME_TOKEN_NAME";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(val)
                } else {
                    Ok(0) // Default disabled (legacy mode)
                }
            },
            None => Ok(0) // Default disabled (legacy mode)
        }
    }

    /// Get TOKEN_ID_NUM dynamic property (TRC-10 issuance counter).
    ///
    /// Java stores this as a big-endian i64 under key "TOKEN_ID_NUM". The value represents
    /// the last-issued token id (java-tron increments before use).
    ///
    /// Default: 1_000_000 to match mainnet genesis (first issued token becomes 1_000_001).
    pub fn get_token_id_num(&self) -> Result<i64> {
        let key = b"TOKEN_ID_NUM";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    Ok(1_000_000)
                }
            }
            None => Ok(1_000_000),
        }
    }

    /// Persist TOKEN_ID_NUM dynamic property (big-endian i64).
    pub fn save_token_id_num(&mut self, value: i64) -> Result<()> {
        let key = b"TOKEN_ID_NUM";
        self.buffered_put(
            self.dynamic_properties_database(),
            key.to_vec(),
            value.to_be_bytes().to_vec(),
        )?;
        Ok(())
    }

    /// Get OneDayNetLimit dynamic property
    /// Default: 8640000000 bytes per day
    pub fn get_one_day_net_limit(&self) -> Result<i64> {
        let key = b"ONE_DAY_NET_LIMIT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(val)
                } else {
                    Ok(8_640_000_000) // Default value
                }
            },
            None => Ok(8_640_000_000) // Default value
        }
    }

    /// Get MAX_FROZEN_TIME dynamic property (FreezeBalanceContract validation).
    ///
    /// Java stores this as a 4-byte big-endian int under key "MAX_FROZEN_TIME".
    /// Default: 3 (DynamicPropertiesStore initialization).
    pub fn get_max_frozen_time(&self) -> Result<i64> {
        let key = b"MAX_FROZEN_TIME";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else if data.len() >= 4 {
                    Ok(i64::from(i32::from_be_bytes([data[0], data[1], data[2], data[3]])))
                } else {
                    Ok(3)
                }
            }
            None => Ok(3),
        }
    }

    /// Get MIN_FROZEN_TIME dynamic property (FreezeBalanceContract validation).
    ///
    /// Java stores this as a 4-byte big-endian int under key "MIN_FROZEN_TIME".
    /// Default: 3 (DynamicPropertiesStore initialization).
    pub fn get_min_frozen_time(&self) -> Result<i64> {
        let key = b"MIN_FROZEN_TIME";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else if data.len() >= 4 {
                    Ok(i64::from(i32::from_be_bytes([data[0], data[1], data[2], data[3]])))
                } else {
                    Ok(3)
                }
            }
            None => Ok(3),
        }
    }

    /// Get MAX_FROZEN_SUPPLY_NUMBER dynamic property (AssetIssueContract validation).
    ///
    /// Java stores this as a 4-byte big-endian int under key "MAX_FROZEN_SUPPLY_NUMBER".
    /// Default: 10 (DynamicPropertiesStore initialization).
    pub fn get_max_frozen_supply_number(&self) -> Result<i64> {
        let key = b"MAX_FROZEN_SUPPLY_NUMBER";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else if data.len() >= 4 {
                    Ok(i64::from(i32::from_be_bytes([data[0], data[1], data[2], data[3]])))
                } else {
                    Ok(10)
                }
            }
            None => Ok(10),
        }
    }

    /// Get MAX_FROZEN_SUPPLY_TIME dynamic property (AssetIssueContract validation).
    ///
    /// Java stores this as a 4-byte big-endian int under key "MAX_FROZEN_SUPPLY_TIME".
    /// Default: 3652 (DynamicPropertiesStore initialization).
    pub fn get_max_frozen_supply_time(&self) -> Result<i64> {
        let key = b"MAX_FROZEN_SUPPLY_TIME";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else if data.len() >= 4 {
                    Ok(i64::from(i32::from_be_bytes([data[0], data[1], data[2], data[3]])))
                } else {
                    Ok(3652)
                }
            }
            None => Ok(3652),
        }
    }

    /// Get MIN_FROZEN_SUPPLY_TIME dynamic property (AssetIssueContract validation).
    ///
    /// Java stores this as a 4-byte big-endian int under key "MIN_FROZEN_SUPPLY_TIME".
    /// Default: 1 (DynamicPropertiesStore initialization).
    pub fn get_min_frozen_supply_time(&self) -> Result<i64> {
        let key = b"MIN_FROZEN_SUPPLY_TIME";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else if data.len() >= 4 {
                    Ok(i64::from(i32::from_be_bytes([data[0], data[1], data[2], data[3]])))
                } else {
                    Ok(1)
                }
            }
            None => Ok(1),
        }
    }

    // ============================================================================
    // Strict Dynamic Property Getters (Task 5 - Java Parity)
    // ============================================================================
    //
    // These methods match Java's DynamicPropertiesStore behavior by returning errors
    // when keys are missing. Use these when strict_dynamic_properties is enabled in config.

    /// Get AssetIssueFee with strict mode (errors when missing).
    /// Java: "not found ASSET_ISSUE_FEE"
    pub fn get_asset_issue_fee_strict(&self) -> Result<u64> {
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
                    Err(anyhow::anyhow!("not found ASSET_ISSUE_FEE"))
                }
            },
            None => Err(anyhow::anyhow!("not found ASSET_ISSUE_FEE")),
        }
    }

    /// Get AllowSameTokenName with strict mode (errors when missing).
    /// Java: "not found ALLOW_SAME_TOKEN_NAME"
    pub fn get_allow_same_token_name_strict(&self) -> Result<i64> {
        let key = b" ALLOW_SAME_TOKEN_NAME";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(val)
                } else {
                    Err(anyhow::anyhow!("not found ALLOW_SAME_TOKEN_NAME"))
                }
            },
            None => Err(anyhow::anyhow!("not found ALLOW_SAME_TOKEN_NAME")),
        }
    }

    /// Get TOKEN_ID_NUM with strict mode (errors when missing).
    /// Java: "not found TOKEN_ID_NUM"
    pub fn get_token_id_num_strict(&self) -> Result<i64> {
        let key = b"TOKEN_ID_NUM";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else {
                    Err(anyhow::anyhow!("not found TOKEN_ID_NUM"))
                }
            }
            None => Err(anyhow::anyhow!("not found TOKEN_ID_NUM")),
        }
    }

    /// Get OneDayNetLimit with strict mode (errors when missing).
    /// Java: "not found ONE_DAY_NET_LIMIT"
    pub fn get_one_day_net_limit_strict(&self) -> Result<i64> {
        let key = b"ONE_DAY_NET_LIMIT";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    let val = i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7]
                    ]);
                    Ok(val)
                } else {
                    Err(anyhow::anyhow!("not found ONE_DAY_NET_LIMIT"))
                }
            },
            None => Err(anyhow::anyhow!("not found ONE_DAY_NET_LIMIT")),
        }
    }

    /// Get MAX_FROZEN_SUPPLY_NUMBER with strict mode (errors when missing).
    /// Java: "not found MAX_FROZEN_SUPPLY_NUMBER"
    pub fn get_max_frozen_supply_number_strict(&self) -> Result<i64> {
        let key = b"MAX_FROZEN_SUPPLY_NUMBER";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else if data.len() >= 4 {
                    Ok(i64::from(i32::from_be_bytes([data[0], data[1], data[2], data[3]])))
                } else {
                    Err(anyhow::anyhow!("not found MAX_FROZEN_SUPPLY_NUMBER"))
                }
            }
            None => Err(anyhow::anyhow!("not found MAX_FROZEN_SUPPLY_NUMBER")),
        }
    }

    /// Get MAX_FROZEN_SUPPLY_TIME with strict mode (errors when missing).
    /// Java: "not found MAX_FROZEN_SUPPLY_TIME"
    pub fn get_max_frozen_supply_time_strict(&self) -> Result<i64> {
        let key = b"MAX_FROZEN_SUPPLY_TIME";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else if data.len() >= 4 {
                    Ok(i64::from(i32::from_be_bytes([data[0], data[1], data[2], data[3]])))
                } else {
                    Err(anyhow::anyhow!("not found MAX_FROZEN_SUPPLY_TIME"))
                }
            }
            None => Err(anyhow::anyhow!("not found MAX_FROZEN_SUPPLY_TIME")),
        }
    }

    /// Get MIN_FROZEN_SUPPLY_TIME with strict mode (errors when missing).
    /// Java: "not found MIN_FROZEN_SUPPLY_TIME"
    pub fn get_min_frozen_supply_time_strict(&self) -> Result<i64> {
        let key = b"MIN_FROZEN_SUPPLY_TIME";
        match self.storage_engine.get(self.dynamic_properties_database(), key)? {
            Some(data) => {
                if data.len() >= 8 {
                    Ok(i64::from_be_bytes([
                        data[0], data[1], data[2], data[3],
                        data[4], data[5], data[6], data[7],
                    ]))
                } else if data.len() >= 4 {
                    Ok(i64::from(i32::from_be_bytes([data[0], data[1], data[2], data[3]])))
                } else {
                    Err(anyhow::anyhow!("not found MIN_FROZEN_SUPPLY_TIME"))
                }
            }
            None => Err(anyhow::anyhow!("not found MIN_FROZEN_SUPPLY_TIME")),
        }
    }

    /// Get asset issue by key (asset name or asset id depending on allowSameTokenName)
    pub fn get_asset_issue(&self, key: &[u8], allow_same_token_name: i64) -> Result<Option<crate::protocol::AssetIssueContractData>> {
        let db = if allow_same_token_name == 0 {
            self.asset_issue_database()
        } else {
            self.asset_issue_v2_database()
        };

        match self.storage_engine.get(db, key)? {
            Some(data) => {
                match crate::protocol::AssetIssueContractData::decode(&data[..]) {
                    Ok(asset_issue) => Ok(Some(asset_issue)),
                    Err(e) => {
                        tracing::warn!("Failed to decode AssetIssueContractData: {}", e);
                        Err(anyhow::anyhow!("Failed to decode AssetIssueContractData: {}", e))
                    }
                }
            },
            None => Ok(None)
        }
    }

    /// Put asset issue by key
    pub fn put_asset_issue(&mut self, key: &[u8], asset_issue: &crate::protocol::AssetIssueContractData, v2_store: bool) -> Result<()> {
        use prost::Message;

        let db = if v2_store {
            self.asset_issue_v2_database()
        } else {
            self.asset_issue_database()
        };

        let mut buf = Vec::with_capacity(asset_issue.encoded_len());
        asset_issue.encode(&mut buf)?;
        self.buffered_put(db, key.to_vec(), buf)?;

        Ok(())
    }

    // ============================================================================
    // Exchange Store Access Methods (Phase 2.F)
    // ============================================================================
    //
    // These methods provide access to the Exchange stores for Bancor-style AMM operations.
    //
    // Database names:
    // - ExchangeStore: "exchange" (legacy, used when allowSameTokenName=0)
    // - ExchangeV2Store: "exchange-v2" (primary store)
    //
    // Key format: 8-byte big-endian exchange_id (same as ProposalStore)
    //
    // Java references:
    // - ExchangeStore.java: chainbase/src/main/java/org/tron/core/store/ExchangeStore.java
    // - ExchangeV2Store.java: chainbase/src/main/java/org/tron/core/store/ExchangeV2Store.java
    // - ExchangeCapsule.java: chainbase/src/main/java/org/tron/core/capsule/ExchangeCapsule.java

    /// Get the database name for exchange store (legacy, allowSameTokenName=0)
    fn exchange_database(&self) -> &str {
        db_names::exchange::EXCHANGE
    }

    /// Get the database name for exchange V2 store (primary)
    fn exchange_v2_database(&self) -> &str {
        db_names::exchange::EXCHANGE_V2
    }

    /// Generate key for exchange store: 8-byte big-endian exchange ID
    /// Java reference: ExchangeCapsule.createDbKey() -> ByteArray.fromLong(exchangeId)
    fn exchange_key(&self, exchange_id: i64) -> Vec<u8> {
        use super::key_helpers::exchange_key;
        exchange_key(exchange_id)
    }

    /// Get exchange by ID from V2 store (primary)
    /// Returns the Exchange protobuf
    pub fn get_exchange(&self, exchange_id: i64) -> Result<Option<crate::protocol::Exchange>> {
        self.get_exchange_from_store(exchange_id, true)
    }

    /// Get exchange by ID from specific store
    /// v2_store=true uses "exchange-v2", v2_store=false uses "exchange"
    pub fn get_exchange_from_store(&self, exchange_id: i64, v2_store: bool) -> Result<Option<crate::protocol::Exchange>> {
        use crate::protocol::Exchange;
        use prost::Message;

        let key = self.exchange_key(exchange_id);
        let db = if v2_store { self.exchange_v2_database() } else { self.exchange_database() };

        tracing::debug!("Getting exchange {} from {}, key: {}", exchange_id, db, hex::encode(&key));

        match self.storage_engine.get(db, &key)? {
            Some(data) => {
                tracing::debug!("Found exchange data, length: {}", data.len());
                match Exchange::decode(data.as_slice()) {
                    Ok(exchange) => {
                        tracing::debug!(
                            "Decoded exchange {} - creator: {}, first_token: {}, second_token: {}, balances: {}/{}",
                            exchange.exchange_id,
                            hex::encode(&exchange.creator_address),
                            String::from_utf8_lossy(&exchange.first_token_id),
                            String::from_utf8_lossy(&exchange.second_token_id),
                            exchange.first_token_balance,
                            exchange.second_token_balance
                        );
                        Ok(Some(exchange))
                    },
                    Err(e) => {
                        tracing::error!("Failed to decode exchange {}: {}", exchange_id, e);
                        Err(anyhow::anyhow!("Failed to decode exchange: {}", e))
                    }
                }
            },
            None => {
                tracing::debug!("Exchange {} not found in {}", exchange_id, db);
                Ok(None)
            }
        }
    }

    /// Store exchange to V2 store (primary)
    pub fn put_exchange(&mut self, exchange: &crate::protocol::Exchange) -> Result<()> {
        self.put_exchange_to_store(exchange, true)
    }

    /// Store exchange to specific store
    /// v2_store=true uses "exchange-v2", v2_store=false uses "exchange"
    pub fn put_exchange_to_store(&mut self, exchange: &crate::protocol::Exchange, v2_store: bool) -> Result<()> {
        use prost::Message;

        let key = self.exchange_key(exchange.exchange_id);
        let db = if v2_store { self.exchange_v2_database() } else { self.exchange_database() };
        let data = exchange.encode_to_vec();

        tracing::debug!(
            "Storing exchange {} to {} - creator: {}, first_token: {}, second_token: {}, balances: {}/{}, key: {}",
            exchange.exchange_id,
            db,
            hex::encode(&exchange.creator_address),
            String::from_utf8_lossy(&exchange.first_token_id),
            String::from_utf8_lossy(&exchange.second_token_id),
            exchange.first_token_balance,
            exchange.second_token_balance,
            hex::encode(&key)
        );

        self.buffered_put(db, key, data)?;
        Ok(())
    }

    /// Check if exchange exists in V2 store
    pub fn has_exchange(&self, exchange_id: i64) -> Result<bool> {
        self.has_exchange_in_store(exchange_id, true)
    }

    /// Check if exchange exists in specific store
    pub fn has_exchange_in_store(&self, exchange_id: i64, v2_store: bool) -> Result<bool> {
        let key = self.exchange_key(exchange_id);
        let db = if v2_store { self.exchange_v2_database() } else { self.exchange_database() };
        match self.storage_engine.get(db, &key)? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    // --- Dynamic Properties for Exchange ---

    /// Get LATEST_EXCHANGE_NUM dynamic property
    /// Returns the highest exchange ID that has been created
    pub fn get_latest_exchange_num(&self) -> Result<i64> {
        let key = b"LATEST_EXCHANGE_NUM";
        match self.storage_engine.get(db_names::system::PROPERTIES, key)? {
            Some(data) => {
                if data.len() == 8 {
                    let num = i64::from_be_bytes(data.as_slice().try_into()?);
                    tracing::debug!("LATEST_EXCHANGE_NUM: {}", num);
                    Ok(num)
                } else {
                    tracing::warn!("LATEST_EXCHANGE_NUM has invalid length: {}", data.len());
                    Ok(0)
                }
            },
            None => {
                tracing::debug!("LATEST_EXCHANGE_NUM not found, returning 0");
                Ok(0)
            }
        }
    }

    /// Set LATEST_EXCHANGE_NUM dynamic property
    pub fn set_latest_exchange_num(&mut self, num: i64) -> Result<()> {
        let key = b"LATEST_EXCHANGE_NUM";
        let value = num.to_be_bytes();
        tracing::debug!("Setting LATEST_EXCHANGE_NUM to {}", num);
        self.buffered_put(db_names::system::PROPERTIES, key.to_vec(), value.to_vec())?;
        Ok(())
    }

    /// Get EXCHANGE_BALANCE_LIMIT dynamic property
    /// Maximum balance allowed for each token in an exchange
    /// Default in Java: 1_000_000_000_000_000L (1 quadrillion)
    pub fn get_exchange_balance_limit(&self) -> Result<i64> {
        let key = b"EXCHANGE_BALANCE_LIMIT";
        match self.storage_engine.get(db_names::system::PROPERTIES, key)? {
            Some(data) => {
                if data.len() == 8 {
                    let limit = i64::from_be_bytes(data.as_slice().try_into()?);
                    tracing::debug!("EXCHANGE_BALANCE_LIMIT: {}", limit);
                    Ok(limit)
                } else {
                    tracing::warn!("EXCHANGE_BALANCE_LIMIT has invalid length: {}", data.len());
                    Ok(1_000_000_000_000_000i64) // Default
                }
            },
            None => {
                tracing::debug!("EXCHANGE_BALANCE_LIMIT not found, returning default");
                Ok(1_000_000_000_000_000i64) // Default: 1 quadrillion
            }
        }
    }

    /// Get EXCHANGE_CREATE_FEE dynamic property
    /// Fee charged to create an exchange (in SUN)
    /// Default in Java: 1024_000_000_000L (1024 TRX)
    pub fn get_exchange_create_fee(&self) -> Result<i64> {
        let key = b"EXCHANGE_CREATE_FEE";
        match self.storage_engine.get(db_names::system::PROPERTIES, key)? {
            Some(data) => {
                if data.len() == 8 {
                    let fee = i64::from_be_bytes(data.as_slice().try_into()?);
                    tracing::debug!("EXCHANGE_CREATE_FEE: {}", fee);
                    Ok(fee)
                } else {
                    tracing::warn!("EXCHANGE_CREATE_FEE has invalid length: {}", data.len());
                    Ok(1024_000_000_000i64) // Default
                }
            },
            None => {
                tracing::debug!("EXCHANGE_CREATE_FEE not found, returning default");
                Ok(1024_000_000_000i64) // Default: 1024 TRX
            }
        }
    }

    /// Get ALLOW_STRICT_MATH dynamic property
    /// Controls whether strict math mode is used in AMM calculations
    /// When true, uses StrictMath.pow; when false, uses Math.pow
    /// Default: 0 (false)
    pub fn allow_strict_math(&self) -> Result<bool> {
        let key = b"ALLOW_STRICT_MATH";
        match self.storage_engine.get(db_names::system::PROPERTIES, key)? {
            Some(data) => {
                if data.len() == 8 {
                    let value = i64::from_be_bytes(data.as_slice().try_into()?);
                    Ok(value != 0)
                } else if !data.is_empty() {
                    // Single byte 0 or 1
                    Ok(data[0] != 0)
                } else {
                    Ok(false)
                }
            },
            None => {
                tracing::debug!("ALLOW_STRICT_MATH not found, returning false");
                Ok(false)
            }
        }
    }

    // ==========================================================================
    // Asset Balance Methods (for Exchange contracts)
    // ==========================================================================

    /// Get asset balance for an account (V2 format - allowSameTokenName=1)
    pub fn get_asset_balance_v2(&self, address: &Address, token_id: &[u8]) -> Result<i64> {
        // Get account and read from assetV2 map
        if let Some(account) = self.get_account_proto(address)? {
            // Convert token_id to string key
            let token_key = String::from_utf8_lossy(token_id).to_string();
            // Look up in assetV2 map
            if let Some(&balance) = account.asset_v2.get(&token_key) {
                return Ok(balance);
            }
        }
        Ok(0)
    }

    /// Reduce asset amount from an account (V2 format)
    pub fn reduce_asset_amount_v2(&mut self, address: &Address, token_id: &[u8], amount: i64) -> Result<()> {
        let mut account = self.get_account_proto(address)?
            .ok_or_else(|| anyhow::anyhow!("Account not found"))?;

        let token_key = String::from_utf8_lossy(token_id).to_string();
        let current = account.asset_v2.get(&token_key).copied().unwrap_or(0);

        if current < amount {
            return Err(anyhow::anyhow!("Insufficient asset balance"));
        }

        account.asset_v2.insert(token_key, current - amount);
        self.set_account_proto(address, &account)?;
        Ok(())
    }

    /// Add asset amount to an account (V2 format)
    pub fn add_asset_amount_v2(&mut self, address: &Address, token_id: &[u8], amount: i64) -> Result<()> {
        let mut account = self.get_account_proto(address)?
            .ok_or_else(|| anyhow::anyhow!("Account not found"))?;

        let token_key = String::from_utf8_lossy(token_id).to_string();
        let current = account.asset_v2.get(&token_key).copied().unwrap_or(0);

        account.asset_v2.insert(token_key, current + amount);
        self.set_account_proto(address, &account)?;
        Ok(())
    }

    /// Set/update account from proto
    pub fn set_account_proto(&mut self, address: &Address, account: &crate::protocol::Account) -> Result<()> {
        self.put_account_proto(address, account)
    }

    /// Add balance to an account (for crediting blackhole, etc.)
    pub fn add_balance(&mut self, address: &Address, amount: u64) -> Result<()> {
        let mut account = self.get_account_proto(address)?
            .ok_or_else(|| anyhow::anyhow!("Account not found"))?;

        account.balance = account.balance.checked_add(amount as i64)
            .ok_or_else(|| anyhow::anyhow!("Balance overflow"))?;

        self.set_account_proto(address, &account)?;
        Ok(())
    }

    // ==========================================================================
    // Market (DEX) Store Methods - Phase 2.G
    // ==========================================================================

    /// Get database name for MarketOrderStore (dbName: "market_order")
    fn market_order_database(&self) -> &str {
        db_names::market::MARKET_ORDER
    }

    /// Get database name for MarketAccountStore (dbName: "market_account")
    fn market_account_database(&self) -> &str {
        db_names::market::MARKET_ACCOUNT
    }

    /// Get database name for MarketPairToPriceStore (dbName: "market_pair_to_price")
    fn market_pair_to_price_database(&self) -> &str {
        db_names::market::MARKET_PAIR_TO_PRICE
    }

    /// Get database name for MarketPairPriceToOrderStore (dbName: "market_pair_price_to_order")
    fn market_pair_price_to_order_database(&self) -> &str {
        db_names::market::MARKET_PAIR_PRICE_TO_ORDER
    }

    /// Get ALLOW_MARKET_TRANSACTION dynamic property
    /// Controls whether market transactions are allowed
    /// Default: 0 (disabled)
    pub fn allow_market_transaction(&self) -> Result<bool> {
        let key = b"ALLOW_MARKET_TRANSACTION";
        match self.buffered_get(db_names::system::PROPERTIES, key)? {
            Some(data) => {
                if data.len() == 8 {
                    let value = i64::from_be_bytes(data.as_slice().try_into()?);
                    Ok(value != 0)
                } else if !data.is_empty() {
                    Ok(data[0] != 0)
                } else {
                    Ok(false)
                }
            },
            None => {
                tracing::debug!("ALLOW_MARKET_TRANSACTION not found, returning false");
                Ok(false)
            }
        }
    }

    /// Get MARKET_SELL_FEE dynamic property
    /// Fee for placing a sell order
    /// Default: 0
    pub fn get_market_sell_fee(&self) -> Result<i64> {
        let key = b"MARKET_SELL_FEE";
        match self.buffered_get(db_names::system::PROPERTIES, key)? {
            Some(data) => {
                if data.len() == 8 {
                    Ok(i64::from_be_bytes(data.as_slice().try_into()?))
                } else {
                    Ok(0)
                }
            },
            None => Ok(0)
        }
    }

    /// Get MARKET_CANCEL_FEE dynamic property
    /// Fee for canceling an order
    /// Default: 0
    pub fn get_market_cancel_fee(&self) -> Result<i64> {
        let key = b"MARKET_CANCEL_FEE";
        match self.buffered_get(db_names::system::PROPERTIES, key)? {
            Some(data) => {
                if data.len() == 8 {
                    Ok(i64::from_be_bytes(data.as_slice().try_into()?))
                } else {
                    Ok(0)
                }
            },
            None => Ok(0)
        }
    }

    /// Get MARKET_QUANTITY_LIMIT dynamic property
    /// Maximum quantity for market orders
    /// Default: Long.MAX_VALUE
    pub fn get_market_quantity_limit(&self) -> Result<i64> {
        let key = b"MARKET_QUANTITY_LIMIT";
        match self.buffered_get(db_names::system::PROPERTIES, key)? {
            Some(data) => {
                if data.len() == 8 {
                    Ok(i64::from_be_bytes(data.as_slice().try_into()?))
                } else {
                    Ok(i64::MAX)
                }
            },
            None => Ok(i64::MAX)
        }
    }

    /// Get a MarketOrder by order ID
    /// Key: order_id bytes (SHA3 hash)
    pub fn get_market_order(&self, order_id: &[u8]) -> Result<Option<crate::protocol::MarketOrder>> {
        use prost::Message;
        match self.buffered_get(self.market_order_database(), order_id)? {
            Some(data) => {
                let order = crate::protocol::MarketOrder::decode(data.as_slice())?;
                Ok(Some(order))
            },
            None => Ok(None)
        }
    }

    /// Put a MarketOrder
    pub fn put_market_order(&mut self, order_id: &[u8], order: &crate::protocol::MarketOrder) -> Result<()> {
        use prost::Message;
        let mut buf = Vec::new();
        order.encode(&mut buf)?;
        self.buffered_put(self.market_order_database(), order_id.to_vec(), buf)?;
        Ok(())
    }

    /// Check if a MarketOrder exists
    pub fn has_market_order(&self, order_id: &[u8]) -> Result<bool> {
        Ok(self.get_market_order(order_id)?.is_some())
    }

    /// Get MarketAccountOrder for an account
    /// Key: 21-byte TRON address (with 0x41 prefix)
    pub fn get_market_account_order(&self, address: &Address) -> Result<Option<crate::protocol::MarketAccountOrder>> {
        use prost::Message;
        let key = self.to_tron_address_21(address);
        match self.buffered_get(self.market_account_database(), &key)? {
            Some(data) => {
                let account_order = crate::protocol::MarketAccountOrder::decode(data.as_slice())?;
                Ok(Some(account_order))
            },
            None => Ok(None)
        }
    }

    /// Put MarketAccountOrder for an account
    pub fn put_market_account_order(&mut self, address: &Address, account_order: &crate::protocol::MarketAccountOrder) -> Result<()> {
        use prost::Message;
        let key = self.to_tron_address_21(address);
        let mut buf = Vec::new();
        account_order.encode(&mut buf)?;
        self.buffered_put(self.market_account_database(), key.to_vec(), buf)?;
        Ok(())
    }

    /// Get price count for a token pair
    /// Key: createPairKey(sellTokenId, buyTokenId) = 38 bytes (19 + 19)
    pub fn get_market_pair_price_count(&self, pair_key: &[u8]) -> Result<i64> {
        match self.buffered_get(self.market_pair_to_price_database(), pair_key)? {
            Some(data) => {
                if data.len() == 8 {
                    Ok(i64::from_be_bytes(data.as_slice().try_into()?))
                } else {
                    Ok(0)
                }
            },
            None => Ok(0)
        }
    }

    /// Set price count for a token pair
    pub fn set_market_pair_price_count(&mut self, pair_key: &[u8], count: i64) -> Result<()> {
        let data = count.to_be_bytes();
        self.buffered_put(self.market_pair_to_price_database(), pair_key.to_vec(), data.to_vec())?;
        Ok(())
    }

    /// Delete a token pair from MarketPairToPriceStore
    pub fn delete_market_pair(&mut self, pair_key: &[u8]) -> Result<()> {
        self.buffered_delete(self.market_pair_to_price_database(), pair_key.to_vec())?;
        Ok(())
    }

    /// Check if a token pair exists
    pub fn has_market_pair(&self, pair_key: &[u8]) -> Result<bool> {
        Ok(self.buffered_get(self.market_pair_to_price_database(), pair_key)?.is_some())
    }

    /// Get MarketOrderIdList for a price key
    /// Key: createPairPriceKey(sellTokenId, buyTokenId, sellQuantity, buyQuantity) = 54 bytes
    pub fn get_market_order_id_list(&self, price_key: &[u8]) -> Result<Option<crate::protocol::MarketOrderIdList>> {
        use prost::Message;
        match self.buffered_get(self.market_pair_price_to_order_database(), price_key)? {
            Some(data) => {
                let list = crate::protocol::MarketOrderIdList::decode(data.as_slice())?;
                Ok(Some(list))
            },
            None => Ok(None)
        }
    }

    /// Put MarketOrderIdList for a price key
    pub fn put_market_order_id_list(&mut self, price_key: &[u8], list: &crate::protocol::MarketOrderIdList) -> Result<()> {
        use prost::Message;
        let mut buf = Vec::new();
        list.encode(&mut buf)?;
        self.buffered_put(self.market_pair_price_to_order_database(), price_key.to_vec(), buf)?;
        Ok(())
    }

    /// Delete MarketOrderIdList for a price key
    pub fn delete_market_order_id_list(&mut self, price_key: &[u8]) -> Result<()> {
        self.buffered_delete(self.market_pair_price_to_order_database(), price_key.to_vec())?;
        Ok(())
    }

    /// Check if a price key exists in MarketPairPriceToOrderStore
    pub fn has_market_price_key(&self, price_key: &[u8]) -> Result<bool> {
        Ok(self.buffered_get(self.market_pair_price_to_order_database(), price_key)?.is_some())
    }

    /// List all price keys for a given token pair prefix from MarketPairPriceToOrderStore.
    ///
    /// This includes the special head key (price 0/0) if present.
    /// Keys are returned in the underlying RocksDB iteration order (lexicographic).
    /// Java-tron configures a custom comparator for this CF, so callers should
    /// apply TRON's MarketUtils.comparePriceKey ordering when needed.
    pub fn list_market_pair_price_keys(&self, pair_key_prefix: &[u8]) -> Result<Vec<Vec<u8>>> {
        let entries = self.buffered_prefix_query(
            self.market_pair_price_to_order_database(),
            pair_key_prefix,
        )?;
        Ok(entries.into_iter().map(|kv| kv.key).collect())
    }
}
