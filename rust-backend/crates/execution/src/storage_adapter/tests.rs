#[cfg(test)]
mod tests {
    // Import all types via crate-level re-exports
    use crate::{
        AccountAext, BandwidthPath, EngineBackedEvmStateStore, EvmStateDatabase, EvmStateStore,
        FreezeRecord, InMemoryEvmStateStore, ResourceTracker, StateChangeRecord, Vote, VotesRecord,
        WitnessInfo,
    };
    // Import utils functions explicitly
    use crate::storage_adapter::utils::{from_tron_address, to_tron_address};

    // Standard library imports
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};

    // External crate imports
    use revm::primitives::{Account, AccountInfo, Address, Bytecode, B256, U256};
    use revm::DatabaseCommit;

    #[test]
    fn test_snapshot_hooks() {
        let storage = InMemoryEvmStateStore::new();
        let mut db = EvmStateDatabase::new(storage);

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
        let storage = InMemoryEvmStateStore::new();
        let mut db = EvmStateDatabase::new(storage);

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
        let storage = InMemoryEvmStateStore::new();
        let mut db = EvmStateDatabase::new(storage);

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

        assert_eq!(
            actual_evm_hex, expected_evm_hex,
            "EVM address mismatch: expected {}, got {}",
            expected_evm_hex, actual_evm_hex
        );

        // Convert EVM address back to Tron address
        let converted_tron_address = to_tron_address(&evm_address);

        assert_eq!(
            converted_tron_address, tron_address,
            "Tron address mismatch: expected {}, got {}",
            tron_address, converted_tron_address
        );
    }

    #[test]
    fn test_tron_address_roundtrip() {
        // Test multiple addresses for round-trip conversion
        let test_cases = vec![
            // Add the specific example
            (
                "TB16q6kpSEW2WqvTJ9ua7HAoP9ugQ2HdHZ",
                "0x0B53CE4AA6F0C2F3C849F11F682702EC99622E2E",
            ),
        ];

        for (tron_addr, evm_hex) in test_cases {
            // Parse expected EVM address
            let expected_evm =
                Address::from_slice(&hex::decode(&evm_hex[2..]).expect("Invalid hex"));

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

    #[test]
    fn test_account_name_storage() {
        use crate::protocol::Account as ProtoAccount;

        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path())
            .expect("Failed to create storage engine");
        let mut adapter = EngineBackedEvmStateStore::new(storage_engine);

        let test_address = Address::from([1u8; 20]);
        let test_name = b"TestAccount";

        // Account must exist before setting name
        adapter.put_account_proto(&test_address, &ProtoAccount::default()).unwrap();

        // Test setting and getting account name
        assert!(adapter.set_account_name(test_address, test_name).is_ok());

        let retrieved_name = adapter.get_account_name(&test_address).unwrap();
        assert_eq!(retrieved_name, Some("TestAccount".to_string()));

        // Test non-existent account name
        let non_existent_address = Address::from([2u8; 20]);
        let no_name = adapter.get_account_name(&non_existent_address).unwrap();
        assert_eq!(no_name, None);
    }

    #[test]
    fn test_account_name_validation() {
        use crate::protocol::Account as ProtoAccount;

        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path())
            .expect("Failed to create storage engine");
        let mut adapter = EngineBackedEvmStateStore::new(storage_engine);

        let test_address = Address::from([1u8; 20]);
        let another_address = Address::from([2u8; 20]);

        // Accounts must exist before setting name
        adapter.put_account_proto(&test_address, &ProtoAccount::default()).unwrap();
        adapter.put_account_proto(&another_address, &ProtoAccount::default()).unwrap();

        // Test empty name (allowed — Java does not reject empty account names)
        let empty_name = b"";
        assert!(adapter.set_account_name(test_address, empty_name).is_ok());

        // Test name within 200-byte limit (allowed)
        let long_name = b"ThisIsAVeryLongAccountNameThatExceedsTheThirtyTwoByteLimitAndShouldFail";
        assert!(adapter.set_account_name(test_address, long_name).is_ok());

        // Test name exceeding 200-byte limit (should fail)
        let too_long_name = &[b'A'; 201];
        assert!(adapter.set_account_name(test_address, too_long_name).is_err());

        // Test valid name length
        let valid_name = b"ValidAccountName";
        assert!(adapter.set_account_name(test_address, valid_name).is_ok());

        // Test maximum length name (32 bytes)
        let max_length_name = b"ThisIsExactlyThirtyTwoBytesLong!";
        assert_eq!(max_length_name.len(), 32);
        assert!(adapter
            .set_account_name(another_address, max_length_name)
            .is_ok());
    }

    #[test]
    fn test_account_name_utf8_handling() {
        use crate::protocol::Account as ProtoAccount;

        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path())
            .expect("Failed to create storage engine");
        let mut adapter = EngineBackedEvmStateStore::new(storage_engine);

        let test_address = Address::from([1u8; 20]);
        let non_utf8_address = Address::from([2u8; 20]);

        // Accounts must exist before setting name
        adapter.put_account_proto(&test_address, &ProtoAccount::default()).unwrap();
        adapter.put_account_proto(&non_utf8_address, &ProtoAccount::default()).unwrap();

        // Test valid UTF-8 name
        let utf8_name = "ValidUTF8Name".as_bytes();
        assert!(adapter.set_account_name(test_address, utf8_name).is_ok());

        let retrieved_name = adapter.get_account_name(&test_address).unwrap();
        assert_eq!(retrieved_name, Some("ValidUTF8Name".to_string()));

        // Test non-UTF-8 bytes (should store but warn)
        let non_utf8_name = &[0xFF, 0xFE, 0xFD, 0xFC]; // Invalid UTF-8 sequence
        assert!(adapter
            .set_account_name(non_utf8_address, non_utf8_name)
            .is_ok());

        // Should fail to decode as UTF-8 but the setting should have succeeded
        let result = adapter.get_account_name(&non_utf8_address);
        assert!(result.is_err()); // Should error when trying to decode invalid UTF-8
    }

    #[test]
    fn test_witness_protobuf_encode_decode() {
        // Test protobuf encoding and decoding roundtrip
        let address = Address::from([
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc,
            0xde, 0xf0, 0x12, 0x34, 0x56, 0x78,
        ]);
        let witness_info = WitnessInfo {
            address,
            url: "https://test-witness.com".to_string(),
            vote_count: 1000,
        };

        // Encode as protobuf
        let protobuf_data = witness_info.serialize();
        assert!(
            !protobuf_data.is_empty(),
            "Protobuf data should not be empty"
        );

        // Decode protobuf
        let decoded =
            WitnessInfo::deserialize(&protobuf_data).expect("Protobuf decode should succeed");

        assert_eq!(decoded.address, witness_info.address);
        assert_eq!(decoded.url, witness_info.url);
        assert_eq!(decoded.vote_count, witness_info.vote_count);
    }

    #[test]
    fn test_witness_legacy_encode_decode() {
        // Test legacy encoding and decoding roundtrip
        let address = Address::from([
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc,
            0xde, 0xf0, 0x12, 0x34, 0x56, 0x78,
        ]);
        let witness_info = WitnessInfo {
            address,
            url: "https://legacy-witness.com".to_string(),
            vote_count: 2000,
        };

        // Encode as legacy
        let legacy_data = witness_info.serialize();
        assert!(!legacy_data.is_empty(), "Legacy data should not be empty");

        // Decode legacy
        let decoded = WitnessInfo::deserialize(&legacy_data).expect("Legacy decode should succeed");

        assert_eq!(decoded.address, witness_info.address);
        assert_eq!(decoded.url, witness_info.url);
        assert_eq!(decoded.vote_count, witness_info.vote_count);
    }

    #[test]
    fn test_witness_protobuf_address_formats() {
        use crate::protocol::Witness;
        use prost::Message;

        // Test 21-byte TRON address (0x41 prefix)
        let mut tron_addr_21 = vec![0x41];
        tron_addr_21.extend_from_slice(&[0x12; 20]);

        let witness_21 = Witness {
            address: tron_addr_21.clone(),
            vote_count: 100,
            url: "test".to_string(),
            pub_key: vec![],
            total_produced: 0,
            total_missed: 0,
            latest_block_num: 0,
            latest_slot_num: 0,
            is_jobs: true,
        };
        let data_21 = witness_21.encode_to_vec();

        let decoded_21 =
            WitnessInfo::deserialize(&data_21).expect("Should decode 21-byte TRON address");
        assert_eq!(decoded_21.address, Address::from([0x12; 20]));

        // Test 20-byte address (no prefix)
        let witness_20 = Witness {
            address: vec![0x34; 20],
            vote_count: 200,
            url: "test".to_string(),
            pub_key: vec![],
            total_produced: 0,
            total_missed: 0,
            latest_block_num: 0,
            latest_slot_num: 0,
            is_jobs: true,
        };
        let data_20 = witness_20.encode_to_vec();

        let decoded_20 = WitnessInfo::deserialize(&data_20).expect("Should decode 20-byte address");
        assert_eq!(decoded_20.address, Address::from([0x34; 20]));
    }

    #[test]
    fn test_witness_protobuf_negative_vote_count() {
        use crate::protocol::Witness;
        use prost::Message;

        let witness = Witness {
            address: vec![0x41; 21],
            vote_count: -100, // Negative vote count
            url: "test".to_string(),
            pub_key: vec![],
            total_produced: 0,
            total_missed: 0,
            latest_block_num: 0,
            latest_slot_num: 0,
            is_jobs: true,
        };
        let data = witness.encode_to_vec();

        // Should fail on negative vote count
        assert!(
            WitnessInfo::deserialize(&data).is_err(),
            "Should reject negative voteCount"
        );
    }

    #[test]
    fn test_witness_protobuf_invalid_address_length() {
        use crate::protocol::Witness;
        use prost::Message;

        let witness = Witness {
            address: vec![0x41; 19], // Invalid length
            vote_count: 100,
            url: "test".to_string(),
            pub_key: vec![],
            total_produced: 0,
            total_missed: 0,
            latest_block_num: 0,
            latest_slot_num: 0,
            is_jobs: true,
        };
        let data = witness.encode_to_vec();

        // Should fail on invalid address length
        assert!(
            WitnessInfo::deserialize(&data).is_err(),
            "Should reject invalid address length"
        );
    }

    #[test]
    fn test_witness_empty_url() {
        // Test that empty URLs are allowed
        let address = Address::from([0xcd; 20]);
        let witness_info = WitnessInfo {
            address,
            url: "".to_string(), // Empty URL
            vote_count: 0,
        };

        // Protobuf roundtrip
        let protobuf_data = witness_info.serialize();
        let decoded_pb = WitnessInfo::deserialize(&protobuf_data)
            .expect("Should decode empty URL from protobuf");
        assert_eq!(decoded_pb.url, "");

        // Legacy roundtrip
        let legacy_data = witness_info.serialize();
        let decoded_legacy =
            WitnessInfo::deserialize(&legacy_data).expect("Should decode empty URL from legacy");
        assert_eq!(decoded_legacy.url, "");
    }

    // Tron power computation tests

    #[test]
    fn test_tron_power_bandwidth_only() {
        let storage = InMemoryEvmStateStore::new();
        let address = Address::from([0xab; 20]);

        // Set freeze record for BANDWIDTH (resource=0)
        storage
            .set_freeze_record(&address, 0, 1_000_000, 1000000000)
            .expect("Should set freeze record");

        let power = storage
            .get_tron_power_in_sun(&address, false)
            .expect("Should compute tron power");
        assert_eq!(power, 1_000_000, "Expected power from bandwidth only");
    }

    #[test]
    fn test_tron_power_energy_only() {
        let storage = InMemoryEvmStateStore::new();
        let address = Address::from([0xbc; 20]);

        // Set freeze record for ENERGY (resource=1)
        storage
            .set_freeze_record(&address, 1, 2_000_000, 1000000000)
            .expect("Should set freeze record");

        let power = storage
            .get_tron_power_in_sun(&address, false)
            .expect("Should compute tron power");
        assert_eq!(power, 2_000_000, "Expected power from energy only");
    }

    #[test]
    fn test_tron_power_sum_bw_energy() {
        let storage = InMemoryEvmStateStore::new();
        let address = Address::from([0xcd; 20]);

        // Set freeze records for both BANDWIDTH and ENERGY
        storage
            .set_freeze_record(&address, 0, 1_000_000, 1000000000)
            .expect("Should set bandwidth freeze");
        storage
            .set_freeze_record(&address, 1, 2_000_000, 1000000000)
            .expect("Should set energy freeze");

        let power = storage
            .get_tron_power_in_sun(&address, false)
            .expect("Should compute tron power");
        assert_eq!(power, 3_000_000, "Expected sum of bandwidth + energy");
    }

    #[test]
    fn test_tron_power_includes_tron_power_legacy() {
        let storage = InMemoryEvmStateStore::new();
        let address = Address::from([0xde; 20]);

        // Set freeze record for TRON_POWER (resource=2) only
        storage
            .set_freeze_record(&address, 2, 500_000, 1000000000)
            .expect("Should set tron_power freeze");

        let power = storage
            .get_tron_power_in_sun(&address, false)
            .expect("Should compute tron power");
        assert_eq!(power, 500_000, "Expected power from legacy tron_power");
    }

    #[test]
    fn test_tron_power_all_three() {
        let storage = InMemoryEvmStateStore::new();
        let address = Address::from([0xef; 20]);

        // Set freeze records for all three resources
        storage
            .set_freeze_record(&address, 0, 1_000_000, 1000000000)
            .expect("Should set bandwidth freeze");
        storage
            .set_freeze_record(&address, 1, 2_000_000, 1000000000)
            .expect("Should set energy freeze");
        storage
            .set_freeze_record(&address, 2, 500_000, 1000000000)
            .expect("Should set tron_power freeze");

        let power = storage
            .get_tron_power_in_sun(&address, false)
            .expect("Should compute tron power");
        assert_eq!(power, 3_500_000, "Expected sum of all three resources");
    }

    #[test]
    fn test_tron_power_overflow_protection() {
        let storage = InMemoryEvmStateStore::new();
        let address = Address::from([0xf0; 20]);

        // Set freeze records that would overflow u64
        let near_max = u64::MAX - 100_000;
        storage
            .set_freeze_record(&address, 0, near_max, 1000000000)
            .expect("Should set bandwidth freeze");
        storage
            .set_freeze_record(&address, 1, 200_000, 1000000000)
            .expect("Should set energy freeze");

        // Should return error due to overflow
        let result = storage.get_tron_power_in_sun(&address, false);
        assert!(result.is_err(), "Expected overflow error");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("overflow"),
            "Error should mention overflow"
        );
    }

    #[test]
    fn test_tron_power_no_freeze_records() {
        let storage = InMemoryEvmStateStore::new();
        let address = Address::from([0xa1; 20]);

        // No freeze records set
        let power = storage
            .get_tron_power_in_sun(&address, false)
            .expect("Should compute tron power");
        assert_eq!(power, 0, "Expected zero power when no freeze records");
    }

    // ResourceTracker Tests

    #[test]
    fn test_resource_tracker_increase_no_time_delta() {
        // When now == lastTime, no recovery should occur
        let new_usage = ResourceTracker::increase(100, 50, 1000, 1000, 28800);
        assert_eq!(new_usage, 150, "No recovery when time delta is 0");
    }

    #[test]
    fn test_resource_tracker_increase_partial_recovery() {
        // lastUsage=1000, usage=200, lastTime=0, now=14400, windowSize=28800
        // Time delta = 14400 (half the window)
        // Recovery = 1000 * 14400 / 28800 = 500
        // New usage = max(0, 1000 - 500) + 200 = 500 + 200 = 700
        let new_usage = ResourceTracker::increase(1000, 200, 0, 14400, 28800);
        assert_eq!(new_usage, 700, "Half window should recover half usage");
    }

    #[test]
    fn test_resource_tracker_increase_full_recovery() {
        // lastUsage=1000, usage=200, lastTime=0, now=28800, windowSize=28800
        // Time delta = 28800 (full window)
        // Recovery = 1000 (full recovery)
        // New usage = max(0, 1000 - 1000) + 200 = 0 + 200 = 200
        let new_usage = ResourceTracker::increase(1000, 200, 0, 28800, 28800);
        assert_eq!(new_usage, 200, "Full window should fully recover");
    }

    #[test]
    fn test_resource_tracker_increase_beyond_window() {
        // Time delta exceeds window - should fully recover
        let new_usage = ResourceTracker::increase(1000, 200, 0, 50000, 28800);
        assert_eq!(new_usage, 200, "Beyond window should fully recover");
    }

    #[test]
    fn test_resource_tracker_recovery_zero_usage() {
        // Recovery with zero last usage
        let new_usage = ResourceTracker::recovery(0, 0, 14400, 28800);
        assert_eq!(new_usage, 0, "Recovery of zero usage should be zero");
    }

    #[test]
    fn test_resource_tracker_recovery_half_window() {
        // lastUsage=1000, lastTime=0, now=14400, windowSize=28800
        // Should recover to 500
        let recovered = ResourceTracker::recovery(1000, 0, 14400, 28800);
        assert_eq!(recovered, 500, "Half window recovery");
    }

    #[test]
    fn test_resource_tracker_increase_zero_window() {
        // When windowSize is 0, should just return the usage
        let new_usage = ResourceTracker::increase(1000, 200, 0, 14400, 0);
        assert_eq!(new_usage, 200, "Zero window should return usage only");
    }

    #[test]
    fn test_resource_tracker_increase_negative_time_delta() {
        // When now < lastTime, delta is negative so decay factor > 1.0
        // This matches Java's BandwidthProcessor.increase() behavior
        let new_usage = ResourceTracker::increase(1000, 200, 5000, 4000, 28800);
        assert_eq!(new_usage, 1234, "Negative time delta: decay > 1.0 per Java parity");
    }

    #[test]
    fn test_resource_tracker_increase_overflow_protection() {
        // Test with very large values to ensure no overflow
        let new_usage = ResourceTracker::increase(i64::MAX / 2, 100, 0, 100, 28800);
        // Should not panic and should return a reasonable value
        assert!(new_usage > 0, "Should handle large values without overflow");
    }

    #[test]
    fn test_resource_tracker_track_bandwidth_free_net_path() {
        use crate::storage_adapter::{AccountAext, BandwidthPath, ResourceTracker};

        let owner = Address::from([0xab; 20]);
        let current_aext = AccountAext::with_defaults();
        let free_net_limit = 5000i64;
        let bytes_used = 212i64;
        let now = 1000i64;

        let result = ResourceTracker::track_bandwidth(
            &owner,
            bytes_used,
            now,
            &current_aext,
            free_net_limit,
        );

        assert!(result.is_ok(), "Track bandwidth should succeed");
        let (path, before, after) = result.unwrap();

        assert_eq!(path, BandwidthPath::FreeNet, "Should use FREE_NET path");
        assert_eq!(
            before.free_net_usage, 0,
            "Before should have zero free_net_usage"
        );
        assert_eq!(
            after.free_net_usage, 212,
            "After should have 212 free_net_usage"
        );
        assert_eq!(
            after.latest_consume_free_time, 1000,
            "Should update consume time"
        );
    }

    #[test]
    fn test_resource_tracker_track_bandwidth_with_existing_usage() {
        use crate::storage_adapter::{AccountAext, BandwidthPath, ResourceTracker};

        let owner = Address::from([0xcd; 20]);
        let mut current_aext = AccountAext::with_defaults();
        current_aext.free_net_usage = 1000;
        current_aext.latest_consume_free_time = 0;

        let free_net_limit = 5000i64;
        let bytes_used = 212i64;
        let now = 14400i64; // Half window

        let result = ResourceTracker::track_bandwidth(
            &owner,
            bytes_used,
            now,
            &current_aext,
            free_net_limit,
        );

        assert!(result.is_ok(), "Track bandwidth should succeed");
        let (path, before, after) = result.unwrap();

        assert_eq!(path, BandwidthPath::FreeNet, "Should use FREE_NET path");
        // Before: recovered from 1000 by half = 500
        assert_eq!(
            before.free_net_usage, 500,
            "Before should have recovered to 500"
        );
        // After: 500 + 212 = 712
        assert_eq!(
            after.free_net_usage, 712,
            "After should have 712 free_net_usage"
        );
    }

    #[test]
    fn test_resource_tracker_track_bandwidth_exceeds_limit() {
        use crate::storage_adapter::{AccountAext, BandwidthPath, ResourceTracker};

        let owner = Address::from([0xef; 20]);
        let mut current_aext = AccountAext::with_defaults();
        current_aext.free_net_usage = 4900; // Close to limit
        current_aext.latest_consume_free_time = 0;

        let free_net_limit = 5000i64;
        let bytes_used = 500i64; // Would exceed limit
        let now = 100i64; // Small time delta

        let result = ResourceTracker::track_bandwidth(
            &owner,
            bytes_used,
            now,
            &current_aext,
            free_net_limit,
        );

        assert!(result.is_ok(), "Track bandwidth should succeed");
        let (path, _before, _after) = result.unwrap();

        // Should fall back to FEE when FREE_NET is insufficient
        assert_eq!(
            path,
            BandwidthPath::Fee,
            "Should use FEE path when limit exceeded"
        );
    }

    #[test]
    fn test_account_aext_serialization_roundtrip() {
        let aext = AccountAext {
            net_usage: 100,
            free_net_usage: 200,
            energy_usage: 0,
            latest_consume_time: 1000,
            latest_consume_free_time: 2000,
            latest_consume_time_for_energy: 0,
            net_window_size: 28800,
            net_window_optimized: false,
            energy_window_size: 28800,
            energy_window_optimized: false,
        };

        let serialized = aext.serialize();
        assert_eq!(serialized.len(), 66, "Serialized size should be 66 bytes");

        let deserialized = AccountAext::deserialize(&serialized).expect("Should deserialize");

        assert_eq!(deserialized.net_usage, 100);
        assert_eq!(deserialized.free_net_usage, 200);
        assert_eq!(deserialized.latest_consume_time, 1000);
        assert_eq!(deserialized.latest_consume_free_time, 2000);
        assert_eq!(deserialized.net_window_size, 28800);
        assert_eq!(deserialized.net_window_optimized, false);
    }

    #[test]
    fn test_account_aext_with_defaults() {
        let aext = AccountAext::with_defaults();

        assert_eq!(aext.net_usage, 0);
        assert_eq!(aext.free_net_usage, 0);
        assert_eq!(aext.net_window_size, 28800);
        assert_eq!(aext.energy_window_size, 28800);
        assert_eq!(aext.net_window_optimized, false);
        assert_eq!(aext.energy_window_optimized, false);
    }

    #[test]
    fn test_allow_change_delegation_reads_change_delegation_key() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path())
            .expect("Failed to create storage engine");

        storage_engine
            .put("properties", b"CHANGE_DELEGATION", &1i64.to_be_bytes())
            .expect("Failed to set CHANGE_DELEGATION");

        let adapter = EngineBackedEvmStateStore::new(storage_engine);
        assert!(adapter
            .allow_change_delegation()
            .expect("allow_change_delegation should succeed"));
    }

    #[test]
    fn test_total_net_weight_reads_buffered_writes() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path())
            .expect("Failed to create storage engine");

        let (adapter, _buffer) = EngineBackedEvmStateStore::new_with_buffer(storage_engine);

        adapter
            .add_total_net_weight(2)
            .expect("Should add to total net weight");

        let total = adapter
            .get_total_net_weight()
            .expect("Should read total net weight");
        assert_eq!(total, 2);
    }

    #[test]
    fn test_account_asset_v2_zero_value_encodes_value_field() {
        use crate::protocol::Account as ProtoAccount;
        use crate::storage_adapter::db_names;

        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let storage_engine = tron_backend_storage::StorageEngine::new(temp_dir.path())
            .expect("Failed to create storage engine");

        let adapter = EngineBackedEvmStateStore::new(storage_engine.clone());
        let prefix = adapter.address_prefix();

        let evm_address = Address::from([0x01u8; 20]);
        let mut tron_address = Vec::with_capacity(21);
        tron_address.push(prefix);
        tron_address.extend_from_slice(evm_address.as_slice());

        let mut account = ProtoAccount::default();
        account.account_name = b"other".to_vec();
        account.address = tron_address;
        account.balance = 1_001_000_000_000;
        account.asset_v2.insert("1000001".to_string(), 0);

        adapter
            .put_account_proto(&evm_address, &account)
            .expect("Failed to write account");

        let mut key = Vec::with_capacity(21);
        key.push(prefix);
        key.extend_from_slice(evm_address.as_slice());

        let stored = storage_engine
            .get(db_names::account::ACCOUNT, &key)
            .expect("Failed to read account bytes")
            .expect("Account bytes missing");

        // Field 56 (assetV2) tag + entry (len=11) + key + value(0)
        let expected_bytes = hex::decode("c2030b0a07313030303030311000").unwrap();
        assert!(
            stored
                .windows(expected_bytes.len())
                .any(|window| window == expected_bytes.as_slice()),
            "assetV2 entry did not encode the value field for 0: {}",
            hex::encode(stored)
        );
    }
}
