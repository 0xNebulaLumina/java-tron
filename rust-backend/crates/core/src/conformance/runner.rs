//! Conformance test runner for executing fixtures and comparing results.
//!
//! This module provides two modes of fixture testing:
//! - `validate_fixture`: Structure-only validation (no execution)
//! - `run_fixture`: Full execution and state comparison (requires storage engine setup)

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::backend::ExecuteTransactionRequest;
use crate::conformance::kv_format::{compare_kv_data, read_kv_file, KvDiff};
use crate::conformance::metadata::FixtureMetadata;
use tron_backend_common::{ExecutionConfig, ModuleManager, RemoteExecutionConfig};
use tron_backend_storage::StorageEngine;
use tron_backend_execution::{
    EngineBackedEvmStateStore, ExecutionModule, TronTransaction, TronExecutionContext,
    TronContractType, TxMetadata,
};
use revm_primitives::{Address, U256, Bytes};
use crate::BackendService;

/// Result of running a conformance test
#[derive(Debug)]
pub struct ConformanceResult {
    /// Fixture metadata
    pub metadata: FixtureMetadata,
    /// Whether all comparisons passed
    pub passed: bool,
    /// Database differences by database name
    pub db_diffs: Vec<(String, KvDiff)>,
    /// Error message if execution failed
    pub error: Option<String>,
    /// Execution status from Rust backend
    pub execution_status: Option<String>,
}

impl ConformanceResult {
    pub fn failure(metadata: FixtureMetadata, error: String) -> Self {
        ConformanceResult {
            metadata,
            passed: false,
            db_diffs: Vec::new(),
            error: Some(error),
            execution_status: None,
        }
    }

    pub fn summary(&self) -> String {
        if self.passed {
            format!(
                "PASS: {}/{}",
                self.metadata.contract_type, self.metadata.case_name
            )
        } else {
            let mut msg = format!(
                "FAIL: {}/{}",
                self.metadata.contract_type, self.metadata.case_name
            );
            if let Some(ref err) = self.error {
                msg.push_str(&format!(" - {}", err));
            }
            for (db_name, diff) in &self.db_diffs {
                if !diff.is_empty() {
                    msg.push_str(&format!(" | {}: {}", db_name, diff.summary()));
                }
            }
            msg
        }
    }
}

/// Fixture test runner
pub struct ConformanceRunner {
    fixtures_dir: PathBuf,
}

impl ConformanceRunner {
    /// Create a new runner with the fixtures directory.
    pub fn new(fixtures_dir: impl AsRef<Path>) -> Self {
        ConformanceRunner {
            fixtures_dir: fixtures_dir.as_ref().to_path_buf(),
        }
    }

    /// Discover all fixtures in the directory.
    pub fn discover_fixtures(&self) -> Vec<FixtureInfo> {
        let mut fixtures = Vec::new();

        if let Ok(entries) = fs::read_dir(&self.fixtures_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // This is a contract type directory
                    if let Ok(cases) = fs::read_dir(&path) {
                        for case_entry in cases.flatten() {
                            let case_path = case_entry.path();
                            if case_path.is_dir() {
                                let metadata_path = case_path.join("metadata.json");
                                if metadata_path.exists() {
                                    fixtures.push(FixtureInfo {
                                        path: case_path,
                                        metadata_path,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        fixtures.sort_by(|a, b| a.path.cmp(&b.path));
        fixtures
    }

    /// Load a fixture's pre-execution state.
    pub fn load_pre_state(
        &self,
        fixture: &FixtureInfo,
    ) -> Result<BTreeMap<String, BTreeMap<Vec<u8>, Vec<u8>>>, String> {
        let pre_db_dir = fixture.path.join("pre_db");
        self.load_db_state(&pre_db_dir)
    }

    /// Load a fixture's expected post-execution state.
    pub fn load_expected_state(
        &self,
        fixture: &FixtureInfo,
    ) -> Result<BTreeMap<String, BTreeMap<Vec<u8>, Vec<u8>>>, String> {
        let post_db_dir = fixture.path.join("expected").join("post_db");
        self.load_db_state(&post_db_dir)
    }

    /// Load database state from a directory containing .kv files.
    fn load_db_state(
        &self,
        dir: &Path,
    ) -> Result<BTreeMap<String, BTreeMap<Vec<u8>, Vec<u8>>>, String> {
        let mut state = BTreeMap::new();

        if !dir.exists() {
            return Ok(state);
        }

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "kv") {
                    let db_name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_string())
                        .unwrap_or_default();

                    match read_kv_file(&path) {
                        Ok(data) => {
                            state.insert(db_name, data);
                        }
                        Err(e) => {
                            return Err(format!("Failed to read {}: {}", path.display(), e));
                        }
                    }
                }
            }
        }

        Ok(state)
    }

    /// Load the ExecuteTransactionRequest protobuf.
    pub fn load_request(&self, fixture: &FixtureInfo) -> Result<ExecuteTransactionRequest, String> {
        let request_path = fixture.path.join("request.pb");
        if !request_path.exists() {
            return Err("request.pb not found".to_string());
        }

        let bytes = fs::read(&request_path)
            .map_err(|e| format!("Failed to read request.pb: {}", e))?;

        use prost::Message;
        ExecuteTransactionRequest::decode(bytes.as_slice())
            .map_err(|e| format!("Failed to decode request.pb: {}", e))
    }

    /// Compare actual state against expected state.
    pub fn compare_states(
        &self,
        expected: &BTreeMap<String, BTreeMap<Vec<u8>, Vec<u8>>>,
        actual: &BTreeMap<String, BTreeMap<Vec<u8>, Vec<u8>>>,
        databases: &[String],
    ) -> Vec<(String, KvDiff)> {
        let mut diffs = Vec::new();

        for db_name in databases {
            let expected_db = expected.get(db_name).cloned().unwrap_or_default();
            let actual_db = actual.get(db_name).cloned().unwrap_or_default();

            let diff = compare_kv_data(&expected_db, &actual_db);
            if !diff.is_empty() {
                diffs.push((db_name.clone(), diff));
            }
        }

        diffs
    }

    /// Create an execution configuration with all contracts enabled for conformance testing.
    fn create_conformance_config() -> ExecutionConfig {
        ExecutionConfig {
            remote: RemoteExecutionConfig {
                system_enabled: true,
                // Enable all Phase 2 contracts for conformance testing
                // Phase 2.A: Proposal contracts
                proposal_create_enabled: true,
                proposal_approve_enabled: true,
                proposal_delete_enabled: true,
                // Phase 2.B: Account contracts
                set_account_id_enabled: true,
                account_permission_update_enabled: true,
                // Phase 2.C: Contract metadata
                update_setting_enabled: true,
                update_energy_limit_enabled: true,
                clear_abi_enabled: true,
                // Phase 2.C2: Brokerage
                update_brokerage_enabled: true,
                // Phase 2.D: Resource/delegation
                withdraw_expire_unfreeze_enabled: true,
                delegate_resource_enabled: true,
                undelegate_resource_enabled: true,
                cancel_all_unfreeze_v2_enabled: true,
                // Phase 2.E: TRC-10 extensions
                participate_asset_issue_enabled: true,
                unfreeze_asset_enabled: true,
                update_asset_enabled: true,
                // Phase 2.F: Exchange
                exchange_create_enabled: true,
                exchange_inject_enabled: true,
                exchange_withdraw_enabled: true,
                exchange_transaction_enabled: true,
                // Phase 2.G: Market
                market_sell_asset_enabled: true,
                market_cancel_order_enabled: true,
                // Enable other system contracts
                witness_create_enabled: true,
                witness_update_enabled: true,
                vote_witness_enabled: true,
                freeze_balance_enabled: true,
                unfreeze_balance_enabled: true,
                freeze_balance_v2_enabled: true,
                unfreeze_balance_v2_enabled: true,
                withdraw_balance_enabled: true,
                account_create_enabled: true,
                trc10_enabled: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Map fixture DB aliases to the actual storage DB names used by the adapter.
    ///
    /// The fixture generator uses some human-friendly names (e.g. "dynamic-properties"),
    /// but the underlying Java stores (and our adapter constants) use different names
    /// (e.g. DynamicPropertiesStore dbName = "properties").
    fn canonical_db_name(db_name: &str) -> &str {
        match db_name {
            "dynamic-properties" => "properties",
            other => other,
        }
    }

    /// Load pre-state KV files into a storage engine.
    fn load_pre_state_into_storage(
        &self,
        storage_engine: &StorageEngine,
        pre_state: &BTreeMap<String, BTreeMap<Vec<u8>, Vec<u8>>>,
    ) -> Result<(), String> {
        for (db_name, kv_map) in pre_state {
            let canonical_db_name = Self::canonical_db_name(db_name);
            for (key, value) in kv_map {
                storage_engine.put(canonical_db_name, key, value)
                    .map_err(|e| format!("Failed to write to {}: {:?}", db_name, e))?;
            }
        }
        Ok(())
    }

    /// Dump current state from storage engine for specified databases.
    fn dump_storage_state(
        &self,
        storage_engine: &StorageEngine,
        databases: &[String],
    ) -> Result<BTreeMap<String, BTreeMap<Vec<u8>, Vec<u8>>>, String> {
        let mut state = BTreeMap::new();

        for db_name in databases {
            let canonical_db_name = Self::canonical_db_name(db_name);
            let mut db_state = BTreeMap::new();
            // Iterate over all keys in the database using get_next with pagination
            let mut start_key = Vec::new();
            let batch_size = 1000i32;

            loop {
                let entries = storage_engine.get_next(canonical_db_name, &start_key, batch_size)
                    .map_err(|e| format!("Failed to iterate {}: {:?}", db_name, e))?;

                if entries.is_empty() {
                    break;
                }

                for entry in &entries {
                    db_state.insert(entry.key.clone(), entry.value.clone());
                }

                // Update start_key for next batch
                if let Some(last) = entries.last() {
                    start_key = last.key.clone();
                    // Append a byte to get the next key
                    start_key.push(0);
                } else {
                    break;
                }

                // If we got fewer entries than requested, we've reached the end
                if (entries.len() as i32) < batch_size {
                    break;
                }
            }

            state.insert(db_name.clone(), db_state);
        }

        Ok(state)
    }

    /// Convert protobuf request to internal transaction format.
    /// This is a simplified version for conformance testing.
    fn convert_request_to_transaction(request: &ExecuteTransactionRequest) -> Result<TronTransaction, String> {
        let tx = request.transaction.as_ref()
            .ok_or("Transaction is required")?;

        // Parse from address (strip 0x41 TRON prefix if present)
        let from_bytes = if tx.from.len() == 21 && (tx.from[0] == 0x41 || tx.from[0] == 0xa0) {
            &tx.from[1..]
        } else if tx.from.len() == 20 {
            &tx.from[..]
        } else {
            return Err(format!("Invalid from address length: {}", tx.from.len()));
        };
        let from = Address::from_slice(from_bytes);

        // Parse to address
        let to = if tx.to.is_empty() {
            None
        } else {
            let to_bytes = if tx.to.len() == 21 && (tx.to[0] == 0x41 || tx.to[0] == 0xa0) {
                &tx.to[1..]
            } else if tx.to.len() == 20 {
                &tx.to[..]
            } else {
                return Err(format!("Invalid to address length: {}", tx.to.len()));
            };
            Some(Address::from_slice(to_bytes))
        };

        // Parse value
        let value = if tx.value.len() <= 32 {
            U256::from_be_slice(&tx.value)
        } else {
            return Err("Invalid value length".to_string());
        };

        // Parse contract type
        let contract_type = TronContractType::try_from(tx.contract_type).ok();

        // Parse asset_id
        let asset_id = if tx.asset_id.is_empty() {
            None
        } else {
            Some(tx.asset_id.clone())
        };

        Ok(TronTransaction {
            from,
            to,
            value,
            data: Bytes::from(tx.data.clone()),
            gas_limit: if tx.energy_limit == 0 { 100000 } else { tx.energy_limit as u64 },
            gas_price: U256::ZERO, // TRON mode uses gas_price = 0
            nonce: tx.nonce as u64,
            metadata: TxMetadata {
                contract_type,
                asset_id,
            },
        })
    }

    /// Convert protobuf context to internal execution context.
    fn convert_request_to_context(request: &ExecuteTransactionRequest) -> Result<TronExecutionContext, String> {
        let ctx = request.context.as_ref()
            .ok_or("Execution context is required")?;

        // Parse coinbase (strip 0x41 prefix if present)
        let block_coinbase = if ctx.coinbase.len() == 21 && (ctx.coinbase[0] == 0x41 || ctx.coinbase[0] == 0xa0) {
            Address::from_slice(&ctx.coinbase[1..])
        } else if ctx.coinbase.len() == 20 {
            Address::from_slice(&ctx.coinbase)
        } else if ctx.coinbase.is_empty() {
            Address::ZERO
        } else {
            return Err(format!("Invalid coinbase length: {}", ctx.coinbase.len()));
        };

        Ok(TronExecutionContext {
            block_number: ctx.block_number as u64,
            block_timestamp: ctx.block_timestamp as u64,
            block_coinbase,
            block_difficulty: U256::ZERO,
            block_gas_limit: if ctx.energy_limit == 0 {
                // TRON does not have an EVM-style per-block gas limit.
                // Use a realistic default large enough for typical system-contract execution.
                ExecutionConfig::default().energy_limit
            } else {
                ctx.energy_limit as u64
            },
            chain_id: 2494104990, // TRON mainnet chain ID
            energy_price: ctx.energy_price as u64,
            bandwidth_price: 1000, // Default TRON bandwidth price
        })
    }

    /// Run a single fixture test with actual execution.
    /// This loads pre-state, executes the transaction, and compares post-state.
    pub fn run_fixture(&self, fixture: &FixtureInfo) -> ConformanceResult {
        // Load metadata
        let metadata = match FixtureMetadata::from_file(&fixture.metadata_path) {
            Ok(m) => m,
            Err(e) => {
                return ConformanceResult::failure(
                    FixtureMetadata::default_for_path(&fixture.path),
                    format!("Failed to load metadata: {}", e),
                );
            }
        };

        // Load request
        let request = match self.load_request(fixture) {
            Ok(r) => r,
            Err(e) => {
                return ConformanceResult::failure(metadata, format!("Failed to load request: {}", e));
            }
        };

        // Load pre-state
        let pre_state = match self.load_pre_state(fixture) {
            Ok(s) => s,
            Err(e) => {
                return ConformanceResult::failure(metadata, format!("Failed to load pre-state: {}", e));
            }
        };

        // Load expected post-state
        let expected_state = match self.load_expected_state(fixture) {
            Ok(s) => s,
            Err(e) => {
                return ConformanceResult::failure(metadata, format!("Failed to load expected state: {}", e));
            }
        };

        // Create temp directory for execution
        let temp_dir = match tempfile::tempdir() {
            Ok(d) => d,
            Err(e) => {
                return ConformanceResult::failure(metadata, format!("Failed to create temp dir: {}", e));
            }
        };

        // Create storage engine and load pre-state
        let storage_engine = match StorageEngine::new(temp_dir.path()) {
            Ok(e) => e,
            Err(e) => {
                return ConformanceResult::failure(metadata, format!("Failed to create storage engine: {:?}", e));
            }
        };

        if let Err(e) = self.load_pre_state_into_storage(&storage_engine, &pre_state) {
            return ConformanceResult::failure(metadata, e);
        }

        // Create a BackendService instance configured for conformance.
        // This ensures we exercise the same NON_VM dispatch path as the gRPC server.
        let config = Self::create_conformance_config();
        let mut module_manager = ModuleManager::new();
        module_manager.register("execution", Box::new(ExecutionModule::new(config.clone())));
        let backend_service = BackendService::new(module_manager);

        // Keep an execution module for VM tx kinds (not currently used by fixtures, but supported).
        let execution_module = ExecutionModule::new(config);

        // Convert protobuf request to internal transaction format
        let transaction = match Self::convert_request_to_transaction(&request) {
            Ok(tx) => tx,
            Err(e) => {
                return ConformanceResult::failure(metadata, format!("Failed to convert request: {}", e));
            }
        };

        // Convert execution context
        let context = match Self::convert_request_to_context(&request) {
            Ok(ctx) => ctx,
            Err(e) => {
                return ConformanceResult::failure(metadata, format!("Failed to convert context: {}", e));
            }
        };

        // Determine tx_kind from the protobuf request (proto3 default is NON_VM).
        let tx_kind = request
            .transaction
            .as_ref()
            .and_then(|tx| crate::backend::TxKind::try_from(tx.tx_kind).ok())
            .unwrap_or(crate::backend::TxKind::NonVm);

        // Execute transaction using the same dispatch logic as the real backend.
        let execution_result: Result<tron_backend_execution::TronExecutionResult, String> = match tx_kind {
            crate::backend::TxKind::NonVm => {
                let mut storage_adapter = EngineBackedEvmStateStore::new(storage_engine.clone());
                backend_service.execute_non_vm_contract(&mut storage_adapter, &transaction, &context)
            }
            crate::backend::TxKind::Vm => {
                let storage_adapter = EngineBackedEvmStateStore::new(storage_engine.clone());
                execution_module
                    .execute_transaction_with_storage(storage_adapter, &transaction, &context)
                    .map_err(|e| format!("VM execution error: {}", e))
            }
        };

        // Check execution result
        let execution_status = match &execution_result {
            Ok(result) => {
                if result.success {
                    "SUCCESS".to_string()
                } else {
                    format!("REVERT: {:?}", result.error)
                }
            }
            Err(e) => format!("ERROR: {}", e),
        };

        // Dump actual post-state (even for failure cases) and compare against fixture oracle.
        let actual_state = match self.dump_storage_state(&storage_engine, &metadata.databases_touched) {
            Ok(s) => s,
            Err(e) => {
                return ConformanceResult::failure(metadata, format!("Failed to dump post-state: {}", e));
            }
        };

        // Compare states
        let db_diffs = self.compare_states(&expected_state, &actual_state, &metadata.databases_touched);
        let state_ok = db_diffs.is_empty();

        // Check execution outcome vs expected status.
        let mut status_ok = true;
        let mut status_error: Option<String> = None;

        if metadata.expects_success() {
            match &execution_result {
                Ok(r) if r.success => {}
                Ok(r) => {
                    status_ok = false;
                    status_error = Some(format!("Expected SUCCESS but got REVERT: {:?}", r.error));
                }
                Err(e) => {
                    status_ok = false;
                    status_error = Some(format!("Expected SUCCESS but got ERROR: {}", e));
                }
            }
        } else if metadata.expects_validation_failure() || metadata.expected_status == "REVERT" {
            // Java fixture generator classifies non-success as either VALIDATION_FAILED or REVERT.
            match &execution_result {
                Ok(r) if !r.success => {}
                Err(_) => {}
                Ok(_) => {
                    status_ok = false;
                    status_error = Some(format!("Expected {} but execution succeeded", metadata.expected_status));
                }
            }

            if status_ok {
                if let Some(expected_msg) = metadata.expected_error_message.clone() {
                    let actual_msg = match &execution_result {
                        Ok(r) => r.error.clone().unwrap_or_default(),
                        Err(e) => e.clone(),
                    };
                    if !actual_msg.contains(&expected_msg) {
                        status_ok = false;
                        status_error = Some(format!(
                            "Error message mismatch: expected '{}', got '{}'",
                            expected_msg, actual_msg
                        ));
                    }
                }
            }
        } else {
            // Unknown/legacy status strings: treat anything other than SUCCESS as a failure-expected case.
            if metadata.expected_status != "SUCCESS" {
                match &execution_result {
                    Ok(r) if !r.success => {}
                    Err(_) => {}
                    Ok(_) => {
                        status_ok = false;
                        status_error = Some(format!("Expected {} but execution succeeded", metadata.expected_status));
                    }
                }
            }
        }

        let passed = status_ok && state_ok;
        ConformanceResult {
            metadata,
            passed,
            db_diffs,
            error: if passed {
                None
            } else if !status_ok {
                status_error
            } else {
                Some("State mismatch".to_string())
            },
            execution_status: Some(execution_status),
        }
    }

    /// Run a single fixture test (offline - no actual execution).
    /// This validates the fixture structure and can be extended to run actual execution.
    pub fn validate_fixture(&self, fixture: &FixtureInfo) -> ConformanceResult {
        // Load metadata
        let metadata = match FixtureMetadata::from_file(&fixture.metadata_path) {
            Ok(m) => m,
            Err(e) => {
                return ConformanceResult {
                    metadata: FixtureMetadata {
                        contract_type: "UNKNOWN".to_string(),
                        contract_type_num: 0,
                        case_name: fixture
                            .path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        case_category: "unknown".to_string(),
                        description: None,
                        generated_at: String::new(),
                        generator_version: String::new(),
                        block_number: 0,
                        block_timestamp: 0,
                        databases_touched: Vec::new(),
                        expected_status: String::new(),
                        expected_error_message: None,
                        owner_address: None,
                        dynamic_properties: Default::default(),
                        notes: Vec::new(),
                    },
                    passed: false,
                    db_diffs: Vec::new(),
                    error: Some(format!("Failed to load metadata: {}", e)),
                    execution_status: None,
                };
            }
        };

        // Check request.pb exists
        let request_path = fixture.path.join("request.pb");
        if !request_path.exists() {
            return ConformanceResult::failure(metadata, "request.pb not found".to_string());
        }

        // Check pre_db directory
        let pre_db_dir = fixture.path.join("pre_db");
        if !pre_db_dir.exists() {
            return ConformanceResult::failure(metadata, "pre_db directory not found".to_string());
        }

        // Check expected directory
        let expected_dir = fixture.path.join("expected");
        if !expected_dir.exists() {
            return ConformanceResult::failure(metadata, "expected directory not found".to_string());
        }

        // Validate all databases are present in pre_db
        for db_name in metadata.databases_touched.iter() {
            let kv_file = pre_db_dir.join(format!("{}.kv", db_name));
            if !kv_file.exists() {
                return ConformanceResult::failure(
                    metadata.clone(),
                    format!("Missing pre_db/{}.kv", db_name),
                );
            }
        }

        // For now, just validate structure - actual execution comparison would go here
        ConformanceResult {
            metadata,
            passed: true,
            db_diffs: Vec::new(),
            error: None,
            execution_status: Some("VALIDATED".to_string()),
        }
    }

    /// Run all discovered fixtures with actual execution.
    pub fn run_all(&self) -> Vec<ConformanceResult> {
        let fixtures = self.discover_fixtures();
        fixtures
            .iter()
            .map(|f| self.run_fixture(f))
            .collect()
    }

    /// Validate all discovered fixtures (structure only, no execution).
    pub fn validate_all(&self) -> Vec<ConformanceResult> {
        let fixtures = self.discover_fixtures();
        fixtures
            .iter()
            .map(|f| self.validate_fixture(f))
            .collect()
    }

    /// Print a summary of results.
    pub fn print_summary(results: &[ConformanceResult]) {
        let passed = results.iter().filter(|r| r.passed).count();
        let failed = results.len() - passed;

        println!("\n=== Conformance Test Results ===");
        println!("Total: {} | Passed: {} | Failed: {}", results.len(), passed, failed);
        println!();

        for result in results {
            println!("{}", result.summary());
        }

        if failed > 0 {
            println!("\n=== Failed Tests ===");
            for result in results.iter().filter(|r| !r.passed) {
                println!("\n{}/{}:", result.metadata.contract_type, result.metadata.case_name);
                if let Some(ref err) = result.error {
                    println!("  Error: {}", err);
                }
                for (db_name, diff) in &result.db_diffs {
                    println!("  {}: {}", db_name, diff.summary());
                    for key in &diff.added {
                        println!("    + {}", hex::encode(key));
                    }
                    for key in &diff.removed {
                        println!("    - {}", hex::encode(key));
                    }
                    for m in &diff.modified {
                        println!("    ~ {} (expected {} bytes, got {} bytes)",
                            hex::encode(&m.key),
                            m.expected.len(),
                            m.actual.len()
                        );
                    }
                }
            }
        }
    }
}

/// Information about a discovered fixture.
#[derive(Debug)]
pub struct FixtureInfo {
    pub path: PathBuf,
    pub metadata_path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn create_minimal_fixture(dir: &Path) {
        // Create directories
        fs::create_dir_all(dir.join("pre_db")).unwrap();
        fs::create_dir_all(dir.join("expected/post_db")).unwrap();

        // Create metadata.json
        let metadata = r#"{
            "contractType": "TEST_CONTRACT",
            "contractTypeNum": 99,
            "caseName": "test_case",
            "caseCategory": "happy",
            "generatedAt": "2025-01-15T10:30:00Z",
            "generatorVersion": "1.0.0",
            "blockNumber": 1000,
            "blockTimestamp": 1705312200000,
            "databasesTouched": ["account"],
            "expectedStatus": "SUCCESS"
        }"#;
        let mut file = fs::File::create(dir.join("metadata.json")).unwrap();
        file.write_all(metadata.as_bytes()).unwrap();

        // Create empty request.pb
        fs::File::create(dir.join("request.pb")).unwrap();

        // Create account.kv in pre_db
        let mut kv_data = BTreeMap::new();
        kv_data.insert(vec![0x01], vec![0xAA]);
        crate::conformance::kv_format::write_kv_file(
            &dir.join("pre_db/account.kv"),
            &kv_data,
        ).unwrap();
    }

    #[test]
    fn test_discover_fixtures() {
        let dir = tempdir().unwrap();
        let fixtures_dir = dir.path();

        // Create a fixture
        let fixture_dir = fixtures_dir.join("test_contract/test_case");
        create_minimal_fixture(&fixture_dir);

        let runner = ConformanceRunner::new(fixtures_dir);
        let fixtures = runner.discover_fixtures();

        assert_eq!(fixtures.len(), 1);
        assert!(fixtures[0].path.ends_with("test_contract/test_case"));
    }

    #[test]
    fn test_validate_fixture() {
        let dir = tempdir().unwrap();
        let fixtures_dir = dir.path();

        let fixture_dir = fixtures_dir.join("test_contract/test_case");
        create_minimal_fixture(&fixture_dir);

        let runner = ConformanceRunner::new(fixtures_dir);
        let fixtures = runner.discover_fixtures();

        let result = runner.validate_fixture(&fixtures[0]);
        assert!(result.passed, "Fixture should pass validation: {:?}", result.error);
    }

    /// Integration test that runs against real fixtures if they exist.
    /// This test is meant to be run manually or in CI when fixtures are available.
    ///
    /// Set CONFORMANCE_FIXTURES_DIR environment variable to the fixtures directory path.
    #[test]
    #[ignore] // Ignore by default - run with --ignored to execute
    fn test_run_real_fixtures() {
        // First check environment variable (set by CI script)
        if let Ok(env_path) = std::env::var("CONFORMANCE_FIXTURES_DIR") {
            let env_dir = std::path::PathBuf::from(&env_path);
            if env_dir.exists() && env_dir.is_dir() {
                println!("Found fixtures directory from env: {:?}", env_dir);
                run_fixtures_from_dir(&env_dir);
                return;
            }
        }

        // Try multiple possible fixture locations (relative to rust-backend crate)
        let possible_paths = [
            "conformance/fixtures",
            "../conformance/fixtures",
            "../../conformance/fixtures",
            "../../../conformance/fixtures",
            "../../../../conformance/fixtures",
        ];

        let fixtures_dir = possible_paths.iter()
            .map(|p| std::path::PathBuf::from(p))
            .find(|p| p.exists() && p.is_dir());

        let fixtures_dir = match fixtures_dir {
            Some(dir) => dir,
            None => {
                println!("No fixtures directory found. Skipping real fixture test.");
                println!("Checked paths: {:?}", possible_paths);
                println!("Set CONFORMANCE_FIXTURES_DIR env var to specify the path.");
                return;
            }
        };

        println!("Found fixtures directory: {:?}", fixtures_dir);
        run_fixtures_from_dir(&fixtures_dir);
    }

    fn run_fixtures_from_dir(fixtures_dir: &std::path::Path) {
        println!("Found fixtures directory: {:?}", fixtures_dir);

        let runner = ConformanceRunner::new(&fixtures_dir);
        let fixtures = runner.discover_fixtures();

        println!("Discovered {} fixtures", fixtures.len());

        if fixtures.is_empty() {
            println!("No fixtures found in {:?}. Skipping.", fixtures_dir);
            return;
        }

        // Run actual execution and compare states (not just structure validation)
        let results = runner.run_all();
        ConformanceRunner::print_summary(&results);

        let failed = results.iter().filter(|r| !r.passed).count();
        assert_eq!(failed, 0, "Some fixtures failed validation");
    }

    /// Test that the conversion helpers work correctly
    #[test]
    fn test_convert_request_to_transaction() {
        use crate::backend::{ExecuteTransactionRequest, TronTransaction as ProtoTx, ExecutionContext};

        // Create a minimal request
        let mut proto_tx = ProtoTx::default();
        // Use 21-byte TRON address with 0x41 prefix
        proto_tx.from = vec![0x41, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a,
                              0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14];
        proto_tx.value = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x0f, 0x42, 0x40]; // 1000000 in big-endian
        proto_tx.energy_limit = 100000;
        proto_tx.contract_type = 16; // ProposalCreateContract

        let request = ExecuteTransactionRequest {
            transaction: Some(proto_tx),
            context: Some(ExecutionContext {
                block_number: 1000,
                block_timestamp: 1705312200000,
                ..Default::default()
            }),
            ..Default::default()
        };

        let transaction = ConformanceRunner::convert_request_to_transaction(&request).unwrap();

        // Verify conversion worked
        assert_eq!(transaction.from.as_slice()[0], 0x01); // TRON prefix stripped
        assert_eq!(transaction.gas_limit, 100000);
        assert!(transaction.to.is_none()); // Empty to field
    }

    #[test]
    fn test_convert_request_to_context() {
        use crate::backend::{ExecuteTransactionRequest, TronTransaction as ProtoTx, ExecutionContext};

        let request = ExecuteTransactionRequest {
            transaction: Some(ProtoTx::default()),
            context: Some(ExecutionContext {
                block_number: 1000,
                block_timestamp: 1705312200000,
                energy_limit: 50000000,
                energy_price: 420,
                ..Default::default()
            }),
            ..Default::default()
        };

        let context = ConformanceRunner::convert_request_to_context(&request).unwrap();

        assert_eq!(context.block_number, 1000);
        assert_eq!(context.block_timestamp, 1705312200000);
        assert_eq!(context.block_gas_limit, 50000000);
        assert_eq!(context.energy_price, 420);
    }
}
