//! Conformance testing framework for verifying Rust backend execution parity
//! with Java's embedded actuator execution.
//!
//! # Overview
//!
//! This module provides tools for:
//! - Reading/writing KV files (pre/post execution database state)
//! - Parsing fixture metadata
//! - Running conformance tests and comparing results
//!
//! # Usage
//!
//! ```ignore
//! use tron_backend_core::conformance::{ConformanceRunner, FixtureMetadata};
//!
//! let runner = ConformanceRunner::new("conformance/fixtures");
//! let results = runner.run_all();
//! ConformanceRunner::print_summary(&results);
//! ```
//!
//! # Fixture Structure
//!
//! Each fixture is a directory with:
//! - `metadata.json` - Test case metadata
//! - `request.pb` - ExecuteTransactionRequest protobuf bytes
//! - `pre_db/` - Pre-execution database state (.kv files)
//! - `expected/`
//!   - `post_db/` - Expected post-execution state (.kv files)
//!   - `result.pb` - Expected ExecutionResult protobuf bytes

pub mod kv_format;
pub mod metadata;
pub mod runner;

pub use kv_format::{read_kv_file, write_kv_file, compare_kv_data, KvDiff, KvError};
pub use metadata::FixtureMetadata;
pub use runner::{ConformanceRunner, ConformanceResult, FixtureInfo};
