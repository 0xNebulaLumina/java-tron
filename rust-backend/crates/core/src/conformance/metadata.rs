//! Fixture metadata parsing for conformance testing.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// Metadata for a conformance test fixture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureMetadata {
    /// Contract type name (e.g., "PROPOSAL_CREATE_CONTRACT")
    #[serde(rename = "contractType")]
    pub contract_type: String,

    /// Numeric contract type value
    #[serde(rename = "contractTypeNum")]
    pub contract_type_num: i32,

    /// Test case name (snake_case)
    #[serde(rename = "caseName")]
    pub case_name: String,

    /// Test category: "happy", "validate_fail", or "edge"
    #[serde(rename = "caseCategory")]
    pub case_category: String,

    /// Human-readable description
    #[serde(default)]
    pub description: Option<String>,

    /// ISO 8601 timestamp when generated
    #[serde(rename = "generatedAt")]
    pub generated_at: String,

    /// Generator version
    #[serde(rename = "generatorVersion")]
    pub generator_version: String,

    /// Block number used in execution context
    #[serde(rename = "blockNumber")]
    pub block_number: i64,

    /// Block timestamp in milliseconds
    #[serde(rename = "blockTimestamp")]
    pub block_timestamp: i64,

    /// List of databases touched by this test
    #[serde(rename = "databasesTouched")]
    pub databases_touched: Vec<String>,

    /// Expected execution status
    #[serde(rename = "expectedStatus", default = "default_status")]
    pub expected_status: String,

    /// Expected error message (for failure cases)
    #[serde(rename = "expectedErrorMessage")]
    pub expected_error_message: Option<String>,

    /// Owner address (hex string)
    #[serde(rename = "ownerAddress")]
    pub owner_address: Option<String>,

    /// Dynamic properties set for this test
    #[serde(rename = "dynamicProperties", default)]
    pub dynamic_properties: HashMap<String, serde_json::Value>,

    /// Additional notes
    #[serde(default)]
    pub notes: Vec<String>,
}

fn default_status() -> String {
    "SUCCESS".to_string()
}

impl FixtureMetadata {
    /// Load metadata from a JSON file.
    pub fn from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let metadata: FixtureMetadata = serde_json::from_reader(reader)?;
        Ok(metadata)
    }

    /// Check if this is a successful execution case.
    pub fn expects_success(&self) -> bool {
        self.expected_status == "SUCCESS"
    }

    /// Check if this is a validation failure case.
    pub fn expects_validation_failure(&self) -> bool {
        self.expected_status == "VALIDATION_FAILED"
    }

    /// Get fixture directory name.
    pub fn fixture_dir_name(&self) -> String {
        format!("{}/{}", self.contract_type.to_lowercase(), self.case_name)
    }

    /// Create a default metadata instance for a given path (used when loading fails).
    pub fn default_for_path(path: &std::path::Path) -> Self {
        FixtureMetadata {
            contract_type: "UNKNOWN".to_string(),
            contract_type_num: 0,
            case_name: path
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_parse_metadata() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("metadata.json");

        let json = r#"{
            "contractType": "PROPOSAL_CREATE_CONTRACT",
            "contractTypeNum": 16,
            "caseName": "happy_path_create",
            "caseCategory": "happy",
            "description": "Create a new proposal",
            "generatedAt": "2025-01-15T10:30:00Z",
            "generatorVersion": "1.0.0",
            "blockNumber": 1000,
            "blockTimestamp": 1705312200000,
            "databasesTouched": ["account", "proposal", "dynamic-properties"],
            "expectedStatus": "SUCCESS",
            "ownerAddress": "41abd4b9367799eaa3197fecb144eb71de1e049abc"
        }"#;

        let mut file = File::create(&path).unwrap();
        file.write_all(json.as_bytes()).unwrap();

        let metadata = FixtureMetadata::from_file(&path).unwrap();

        assert_eq!(metadata.contract_type, "PROPOSAL_CREATE_CONTRACT");
        assert_eq!(metadata.contract_type_num, 16);
        assert_eq!(metadata.case_name, "happy_path_create");
        assert_eq!(metadata.case_category, "happy");
        assert_eq!(metadata.block_number, 1000);
        assert!(metadata.expects_success());
        assert!(!metadata.expects_validation_failure());
    }

    #[test]
    fn test_parse_failure_metadata() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("metadata.json");

        let json = r#"{
            "contractType": "PROPOSAL_CREATE_CONTRACT",
            "contractTypeNum": 16,
            "caseName": "validate_fail_not_witness",
            "caseCategory": "validate_fail",
            "generatedAt": "2025-01-15T10:30:00Z",
            "generatorVersion": "1.0.0",
            "blockNumber": 1000,
            "blockTimestamp": 1705312200000,
            "databasesTouched": ["account"],
            "expectedStatus": "VALIDATION_FAILED",
            "expectedErrorMessage": "Witness not found"
        }"#;

        let mut file = File::create(&path).unwrap();
        file.write_all(json.as_bytes()).unwrap();

        let metadata = FixtureMetadata::from_file(&path).unwrap();

        assert!(!metadata.expects_success());
        assert!(metadata.expects_validation_failure());
        assert_eq!(
            metadata.expected_error_message.as_deref(),
            Some("Witness not found")
        );
    }
}
