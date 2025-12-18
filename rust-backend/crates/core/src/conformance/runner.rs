//! Conformance test runner for executing fixtures and comparing results.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::backend::{ExecuteTransactionRequest, ExecuteTransactionResponse};
use crate::conformance::kv_format::{compare_kv_data, read_kv_file, KvDiff};
use crate::conformance::metadata::FixtureMetadata;

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

    /// Run all discovered fixtures.
    pub fn run_all(&self) -> Vec<ConformanceResult> {
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
}
