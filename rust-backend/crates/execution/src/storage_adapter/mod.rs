// Storage adapter module - bridges EVM execution with Tron's multi-database storage architecture

// Submodules
pub mod utils;
pub mod types;
pub mod traits;
pub mod inmem;
pub mod resource_tracker;
pub mod engine_backed;
pub mod database;

// Re-export commonly used types and traits
pub use traits::EvmStateStore;
pub use types::{
    AccountAext, FreezeRecord, Vote, VotesRecord, WitnessInfo, StateChangeRecord
};
pub use inmem::InMemoryEvmStateStore;
pub use engine_backed::EngineBackedEvmStateStore;
pub use database::{EvmStateDatabase, SnapshotHook};
pub use utils::{keccak256, to_tron_address};
#[cfg(test)]
pub use utils::from_tron_address;
pub use resource_tracker::{ResourceTracker, BandwidthPath};
