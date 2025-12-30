//! Storage adapter module for EVM state management.
//!
//! This module provides abstractions for EVM state storage, supporting both
//! in-memory (testing) and persistent (production) storage backends.
//!
//! # Architecture
//!
//! - `traits`: Core trait definitions (EvmStateStore)
//! - `types`: Domain types (WitnessInfo, FreezeRecord, Vote, VotesRecord, AccountAext, StateChangeRecord)
//! - `utils`: Utility functions (keccak256, address conversion)
//! - `db_names`: Database name constants matching Java's store dbName values (Phase 0.2)
//! - `in_memory`: In-memory storage implementation for testing
//! - `engine`: Production storage implementation backed by StorageEngine
//! - `database`: REVM Database wrapper with caching and state tracking
//! - `resource`: Resource accounting (bandwidth, energy)
//!
//! # Public API Compatibility
//!
//! All public exports are re-exported at this module level to maintain
//! compatibility with `lib.rs` expectations.

// Submodule declarations
pub mod traits;
pub mod types;
pub mod utils;
pub mod db_names;
pub mod key_helpers;
pub mod in_memory;
pub mod engine;
pub mod database;
pub mod resource;
pub mod write_buffer;

// Tests module (contains all storage_adapter tests)
#[cfg(test)]
mod tests;

// Public re-exports for API compatibility with lib.rs
pub use traits::EvmStateStore;
pub use types::{
    WitnessInfo, FreezeRecord, Vote, VotesRecord, AccountAext, StateChangeRecord,
};
pub use in_memory::InMemoryEvmStateStore;
pub use engine::EngineBackedEvmStateStore;
pub use database::EvmStateDatabase;
#[allow(unused_imports)]
pub use database::SnapshotHook;
pub use resource::{ResourceTracker, BandwidthPath};
pub use write_buffer::{ExecutionWriteBuffer, WriteBufferBuilder, WriteOp, TouchedKey};
