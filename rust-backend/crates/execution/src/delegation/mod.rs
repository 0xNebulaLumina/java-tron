//! Delegation module for TRON staking rewards.
//!
//! This module implements the delegation reward computation logic, porting
//! `MortgageService.withdrawReward()` from Java to Rust.
//!
//! Key concepts:
//! - Cycles: Time periods for reward accumulation (typically maintenance intervals)
//! - Vi (Vote Index): Cumulative reward index per vote for efficient computation
//! - Brokerage: Witness fee percentage (0-100)
//! - Account Vote Snapshot: Frozen voting state at cycle boundaries

mod keys;
mod types;

pub use keys::*;
pub use types::*;
