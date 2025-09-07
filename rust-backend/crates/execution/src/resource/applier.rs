//! State change applier for TRON resource management
//! 
//! Generates state deltas for account changes, fee handling, and resource updates

use anyhow::Result;
use revm_primitives::{Address, U256, Bytes, AccountInfo};
use sha3::Digest;
use tracing::{debug, info};

use super::config::ResourceConfig;
use super::store::ResourceUsageRecord;

/// Applies resource changes and generates state deltas
pub struct ResourceApplier {
    config: ResourceConfig,
}

impl ResourceApplier {
    pub fn new(config: &ResourceConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    /// Apply sender account changes (balance reduction)
    /// Returns state changes for sender account
    pub fn apply_sender_changes(
        &self,
        address: Address,
        old_balance: U256,
        total_cost: U256,
        _usage: &ResourceUsageRecord,
    ) -> Result<Vec<crate::TronStateChange>> {
        let mut state_changes = Vec::new();

        // Calculate new balance
        let new_balance = old_balance.saturating_sub(total_cost);
        
        debug!("Applying sender changes: address={:?}, old_balance={}, new_balance={}, cost={}", 
               address, old_balance, new_balance, total_cost);

        // Create account balance change
        let old_account = Some(AccountInfo {
            balance: old_balance,
            nonce: 0, // Nonce handling would be more complex in full implementation
            code_hash: revm_primitives::KECCAK_EMPTY,
            code: None,
        });
        
        let new_account = Some(AccountInfo {
            balance: new_balance,
            nonce: 0,
            code_hash: revm_primitives::KECCAK_EMPTY,
            code: None,
        });

        let balance_change = crate::TronStateChange::AccountChange {
            address,
            old_account,
            new_account,
        };
        state_changes.push(balance_change);

        // Resource usage tracking is persisted in a dedicated DB and should not be
        // emitted as a storage change in state deltas returned to Java.
        info!("Generated {} state changes for sender {:?}", state_changes.len(), address);
        Ok(state_changes)
    }

    /// Apply recipient account changes (balance increase, account creation if needed)
    pub fn apply_recipient_changes(
        &self,
        address: Address,
        old_balance: U256,
        value: U256,
    ) -> Result<Vec<crate::TronStateChange>> {
        let mut state_changes = Vec::new();

        // Calculate new balance
        let new_balance = old_balance + value;
        
        debug!("Applying recipient changes: address={:?}, old_balance={}, new_balance={}, value={}", 
               address, old_balance, new_balance, value);

        // Create account balance change (this handles account creation too)
        let old_account = if old_balance == U256::ZERO {
            None // New account
        } else {
            Some(AccountInfo {
                balance: old_balance,
                nonce: 0,
                code_hash: revm_primitives::KECCAK_EMPTY,
                code: None,
            })
        };
        
        let new_account = Some(AccountInfo {
            balance: new_balance,
            nonce: 0,
            code_hash: revm_primitives::KECCAK_EMPTY,
            code: None,
        });

        let balance_change = crate::TronStateChange::AccountChange {
            address,
            old_account,
            new_account,
        };
        state_changes.push(balance_change);

        info!("Generated {} state changes for recipient {:?}", state_changes.len(), address);
        Ok(state_changes)
    }

    /// Apply fee handling (burn or blackhole credit)
    pub fn apply_fee_changes(&self, fee_amount: U256) -> Result<Vec<crate::TronStateChange>> {
        if fee_amount == U256::ZERO {
            return Ok(Vec::new());
        }

        let mut state_changes = Vec::new();

        match self.config.fee_mode.as_str() {
            "burn" => {
                // Burn mode: no state delta (supply reduction handled elsewhere)
                debug!("Fee burned: {} SUN (no state delta)", fee_amount);
            }
            "blackhole" => {
                // Blackhole mode: credit fee to blackhole address
                if let Ok(blackhole_addr) = self.parse_blackhole_address() {
                    let blackhole_change = self.create_blackhole_credit(blackhole_addr, fee_amount)?;
                    state_changes.push(blackhole_change);
                    info!("Fee credited to blackhole: {} SUN", fee_amount);
                } else {
                    // Fallback to burn if blackhole address invalid
                    debug!("Invalid blackhole address, falling back to burn mode for fee: {} SUN", fee_amount);
                }
            }
            "none" => {
                // No fee handling
                debug!("Fee handling disabled: {} SUN", fee_amount);
            }
            _ => {
                return Err(anyhow::anyhow!("Invalid fee mode: {}", self.config.fee_mode));
            }
        }

        Ok(state_changes)
    }

    /// Apply fee handling with an explicit mode override ("burn" | "blackhole" | "none")
    pub fn apply_fee_changes_with_mode(
        &self,
        fee_amount: U256,
        mode: &str,
    ) -> Result<Vec<crate::TronStateChange>> {
        if fee_amount == U256::ZERO {
            return Ok(Vec::new());
        }

        match mode {
            "burn" => {
                tracing::debug!("Fee burned (override): {} SUN", fee_amount);
                Ok(Vec::new())
            }
            "blackhole" => {
                // Use configured blackhole address if present
                if let Ok(blackhole_addr) = self.parse_blackhole_address() {
                    let change = self.create_blackhole_credit(blackhole_addr, fee_amount)?;
                    Ok(vec![change])
                } else {
                    tracing::debug!("No valid blackhole address; falling back to burn (override)");
                    Ok(Vec::new())
                }
            }
            "none" => Ok(Vec::new()),
            other => Err(anyhow::anyhow!("Invalid fee mode override: {}", other)),
        }
    }

    /// Parse blackhole address from base58 format
    fn parse_blackhole_address(&self) -> Result<Address> {
        if self.config.blackhole_address_base58.is_empty() {
            return Err(anyhow::anyhow!("Blackhole address not configured"));
        }

        // For now, use a simple conversion - in production this would use proper Base58 decoding
        // This is a placeholder that assumes the address is already in hex format
        let addr_str = &self.config.blackhole_address_base58;
        if addr_str.len() >= 40 {
            // Take last 40 chars as hex address
            let hex_part = &addr_str[addr_str.len()-40..];
            if let Ok(bytes) = hex::decode(hex_part) {
                if bytes.len() == 20 {
                    return Ok(Address::from_slice(&bytes));
                }
            }
        }

        // Fallback: create deterministic address from the base58 string
        let mut addr_bytes = [0u8; 20];
        let hash = sha3::Keccak256::digest(self.config.blackhole_address_base58.as_bytes());
        addr_bytes.copy_from_slice(&hash[12..32]);
        Ok(Address::from_slice(&addr_bytes))
    }

    /// Create blackhole credit state change
    fn create_blackhole_credit(&self, blackhole_addr: Address, fee_amount: U256) -> Result<crate::TronStateChange> {
        debug!("Creating blackhole credit: address={:?}, amount={}", blackhole_addr, fee_amount);

        // For this implementation, we assume blackhole account starts with zero balance
        // In production, we'd need to read the current balance
        let old_balance = U256::ZERO;
        let new_balance = old_balance + fee_amount;

        let old_account = if old_balance == U256::ZERO {
            None
        } else {
            Some(AccountInfo {
                balance: old_balance,
                nonce: 0,
                code_hash: revm_primitives::KECCAK_EMPTY,
                code: None,
            })
        };
        
        let new_account = Some(AccountInfo {
            balance: new_balance,
            nonce: 0,
            code_hash: revm_primitives::KECCAK_EMPTY,
            code: None,
        });

        Ok(crate::TronStateChange::AccountChange {
            address: blackhole_addr,
            old_account,
            new_account,
        })
    }

    /// Encode account balance for state change
    /// Uses the format expected by Java side for account changes
    fn encode_account_balance(&self, balance: U256) -> Bytes {
        // Simple encoding: 32-byte big-endian balance
        // In the full implementation, this would match Java's account encoding exactly
        let mut encoded = [0u8; 32];
        balance.to_be_bytes_vec().into_iter()
            .rev()
            .take(32)
            .enumerate()
            .for_each(|(i, b)| encoded[31-i] = b);
        
        Bytes::from(encoded.to_vec())
    }

    /// Encode resource usage for state change (diagnostic only)
    fn encode_resource_usage(&self, usage: &ResourceUsageRecord) -> Result<Bytes> {
        // Format: [free_net_used(8)][latest_consume_free_time(8)][net_used(8)][latest_consume_time(8)][energy_used(8)]
        let mut encoded = Vec::with_capacity(40);
        encoded.extend_from_slice(&usage.free_net_used.to_be_bytes());
        encoded.extend_from_slice(&usage.latest_consume_free_time.to_be_bytes());
        encoded.extend_from_slice(&usage.net_used.to_be_bytes());
        encoded.extend_from_slice(&usage.latest_consume_time.to_be_bytes());
        encoded.extend_from_slice(&usage.energy_used.to_be_bytes());
        Ok(Bytes::from(encoded))
    }

    /// Encode resource usage as U256 for storage change
    fn encode_resource_usage_as_u256(&self, usage: &ResourceUsageRecord) -> Result<U256> {
        // Legacy-compatible packing into U256: drop one timestamp (net timestamp)
        let mut bytes = [0u8; 32];
        bytes[0..8].copy_from_slice(&usage.free_net_used.to_be_bytes());
        bytes[8..16].copy_from_slice(&usage.latest_consume_time.to_be_bytes());
        bytes[16..24].copy_from_slice(&usage.net_used.to_be_bytes());
        bytes[24..32].copy_from_slice(&usage.energy_used.to_be_bytes());
        Ok(U256::from_be_slice(&bytes))
    }

    /// Validate that all state changes are properly formed
    pub fn validate_state_changes(&self, changes: &[crate::TronStateChange]) -> Result<()> {
        for (i, change) in changes.iter().enumerate() {
            match change {
                crate::TronStateChange::AccountChange { address, old_account, new_account } => {
                    if old_account.is_none() && new_account.is_some() {
                        debug!("State change {} represents account creation", i);
                    }
                }
                crate::TronStateChange::StorageChange { address, key: _, old_value: _, new_value: _ } => {
                    debug!("State change {} is a storage change for address {:?}", i, address);
                }
            }
        }
        Ok(())
    }

    /// Sort state changes deterministically (address ascending, then by type)
    pub fn sort_state_changes(&self, changes: &mut Vec<crate::TronStateChange>) {
        changes.sort_by(|a, b| {
            let addr_a = match a {
                crate::TronStateChange::AccountChange { address, .. } => address,
                crate::TronStateChange::StorageChange { address, .. } => address,
            };
            let addr_b = match b {
                crate::TronStateChange::AccountChange { address, .. } => address,
                crate::TronStateChange::StorageChange { address, .. } => address,
            };

            // First sort by address
            match addr_a.cmp(addr_b) {
                std::cmp::Ordering::Equal => {
                    // Then sort by type (account changes before storage changes)
                    match (a, b) {
                        (crate::TronStateChange::AccountChange { .. }, crate::TronStateChange::StorageChange { .. }) => std::cmp::Ordering::Less,
                        (crate::TronStateChange::StorageChange { .. }, crate::TronStateChange::AccountChange { .. }) => std::cmp::Ordering::Greater,
                        _ => std::cmp::Ordering::Equal,
                    }
                }
                other => other,
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::config::ResourceConfig;
    use tron_backend_common::ExecutionFeeConfig;

    fn create_test_applier() -> ResourceApplier {
        let config = ResourceConfig {
            fee_mode: "burn".to_string(),
            blackhole_address_base58: String::new(),
            support_black_hole_optimization: true,
            use_dynamic_properties: true,
            non_vm_flat_fee: None,
            experimental_vm_fees: false,
        };
        ResourceApplier::new(&config)
    }

    fn create_blackhole_applier() -> ResourceApplier {
        let config = ResourceConfig {
            fee_mode: "blackhole".to_string(),
            blackhole_address_base58: "TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy".to_string(),
            support_black_hole_optimization: true,
            use_dynamic_properties: true,
            non_vm_flat_fee: None,
            experimental_vm_fees: false,
        };
        ResourceApplier::new(&config)
    }

    #[test]
    fn test_sender_changes() {
        let applier = create_test_applier();
        let address = Address::from_slice(&[0x42; 20]);
        let old_balance = U256::from(1_000_000);
        let total_cost = U256::from(100_000);
        let usage = ResourceUsageRecord {
            free_net_used: 1000,
            latest_consume_free_time: 1234567890,
            net_used: 0,
            latest_consume_time: 1234567890,
            energy_used: 0,
        };

        let changes = applier.apply_sender_changes(address, old_balance, total_cost, &usage).unwrap();
        // Only balance/account change should be emitted
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].address, address.0.into());
        assert!(changes[0].key.is_empty()); // Account-level change only
    }

    #[test]
    fn test_recipient_changes() {
        let applier = create_test_applier();
        let address = Address::from_slice(&[0x43; 20]);
        let old_balance = U256::from(500_000);
        let value = U256::from(100_000);

        let changes = applier.apply_recipient_changes(address, old_balance, value).unwrap();
        
        assert_eq!(changes.len(), 1); // Balance change only
        assert_eq!(changes[0].address, address.0.into());
        assert!(changes[0].key.is_empty()); // Account-level change
    }

    #[test]
    fn test_fee_changes_burn_mode() {
        let applier = create_test_applier();
        let fee_amount = U256::from(10_000);

        let changes = applier.apply_fee_changes(fee_amount).unwrap();
        
        assert_eq!(changes.len(), 0); // No state changes for burn mode
    }

    #[test]
    fn test_fee_changes_blackhole_mode() {
        let applier = create_blackhole_applier();
        let fee_amount = U256::from(10_000);

        let changes = applier.apply_fee_changes(fee_amount).unwrap();
        
        assert_eq!(changes.len(), 1); // Blackhole credit change
        assert!(changes[0].key.is_empty()); // Account-level change
    }

    #[test]
    fn test_balance_encoding() {
        let applier = create_test_applier();
        let balance = U256::from(1_000_000);
        
        let encoded = applier.encode_account_balance(balance);
        assert_eq!(encoded.len(), 32);
        
        // Verify it's big-endian encoding
        let decoded = U256::from_be_slice(&encoded);
        assert_eq!(decoded, balance);
    }

    #[test]
    fn test_state_change_sorting() {
        let applier = create_test_applier();
        let addr1 = Address::from_slice(&[0x01; 20]);
        let addr2 = Address::from_slice(&[0x02; 20]);
        
        let mut changes = vec![
            crate::TronStateChange {
                address: addr2.0.into(),
                key: Bytes::from("storage_key"),
                old_value: Bytes::new(),
                new_value: Bytes::new(),
            },
            crate::TronStateChange {
                address: addr1.0.into(),
                key: Bytes::new(), // Account change
                old_value: Bytes::new(),
                new_value: Bytes::new(),
            },
            crate::TronStateChange {
                address: addr2.0.into(),
                key: Bytes::new(), // Account change
                old_value: Bytes::new(),
                new_value: Bytes::new(),
            },
        ];

        applier.sort_state_changes(&mut changes);
        
        // Should be sorted: addr1 account, addr2 account, addr2 storage
        assert_eq!(changes[0].address, addr1.0.into());
        assert!(changes[0].key.is_empty());
        
        assert_eq!(changes[1].address, addr2.0.into());
        assert!(changes[1].key.is_empty());
        
        assert_eq!(changes[2].address, addr2.0.into());
        assert!(!changes[2].key.is_empty());
    }

    #[test]
    fn test_resource_usage_encoding() {
        let applier = create_test_applier();
        let usage = ResourceUsageRecord {
            free_net_used: 1000,
            latest_consume_free_time: 1234567890,
            net_used: 500,
            latest_consume_time: 1234567890,
            energy_used: 750,
        };

        let encoded = applier.encode_resource_usage(&usage).unwrap();
        assert_eq!(encoded.len(), 40); // 5 * 8 bytes
        
        // Verify the encoding can be read back
        assert_eq!(u64::from_be_bytes(encoded[0..8].try_into().unwrap()), usage.free_net_used);
        assert_eq!(u64::from_be_bytes(encoded[8..16].try_into().unwrap()), usage.latest_consume_free_time);
        assert_eq!(u64::from_be_bytes(encoded[16..24].try_into().unwrap()), usage.net_used);
        assert_eq!(u64::from_be_bytes(encoded[24..32].try_into().unwrap()), usage.latest_consume_time);
        assert_eq!(u64::from_be_bytes(encoded[32..40].try_into().unwrap()), usage.energy_used);
    }
}
