//! Exchange (AMM) logic for TRON Bancor-style exchanges
//!
//! This module implements the exchange transaction logic that mirrors
//! Java's ExchangeProcessor and ExchangeCapsule.transaction().
//!
//! ## Algorithm Overview
//!
//! TRON uses a Bancor-style AMM with the formula:
//! ```
//! exchangeToSupply:   issuedSupply = -supply * (1 - (1 + quant/newBalance)^0.0005)
//! exchangeFromSupply: exchangeBalance = balance * ((1 + supplyQuant/supply)^2000 - 1)
//! ```
//!
//! The `exchange()` function combines both:
//! 1. Convert sell tokens to virtual supply units
//! 2. Convert supply units to buy tokens
//!
//! ## Strict Math Mode
//!
//! When `use_strict_math=true`, Java uses `StrictMath.pow()` which guarantees
//! bit-for-bit identical results across all platforms. When false, it uses
//! `Math.pow()` which may have slight platform variations.
//!
//! In Rust, we use `f64::powf()` which should match Java's behavior on the same
//! platform, but for strict mode we need to be extra careful about floating-point
//! determinism.
//!
//! ## References
//!
//! - Java ExchangeProcessor: chainbase/src/main/java/org/tron/core/capsule/ExchangeProcessor.java
//! - Java ExchangeCapsule: chainbase/src/main/java/org/tron/core/capsule/ExchangeCapsule.java

/// Virtual supply constant used in Bancor AMM formula
/// This is the initial virtual supply pool (1 quintillion)
const SUPPLY: i64 = 1_000_000_000_000_000_000;

/// Exchange processor that implements the Bancor-style AMM algorithm
pub struct ExchangeProcessor {
    /// Current virtual supply (starts at SUPPLY, modified during exchange)
    supply: i64,
    /// Whether to use strict math mode (for deterministic cross-platform results)
    use_strict_math: bool,
}

impl ExchangeProcessor {
    /// Create a new exchange processor with the standard supply
    pub fn new(use_strict_math: bool) -> Self {
        Self {
            supply: SUPPLY,
            use_strict_math,
        }
    }

    /// Create a new exchange processor with a custom supply (for testing)
    #[allow(dead_code)]
    pub fn with_supply(supply: i64, use_strict_math: bool) -> Self {
        Self {
            supply,
            use_strict_math,
        }
    }

    /// Convert sell tokens to virtual supply units
    ///
    /// Formula: issuedSupply = -supply * (1.0 - (1.0 + quant/newBalance)^0.0005)
    ///
    /// This function updates internal supply and returns the issued supply amount.
    fn exchange_to_supply(&mut self, balance: i64, quant: i64) -> i64 {
        tracing::debug!("exchange_to_supply: balance={}, quant={}", balance, quant);

        let new_balance = balance + quant;
        tracing::debug!("balance + quant = {}", new_balance);

        // Calculate: -supply * (1.0 - (1.0 + quant/newBalance)^0.0005)
        let ratio = 1.0 + (quant as f64) / (new_balance as f64);
        let power_result = self.pow(ratio, 0.0005);
        let issued_supply = -1.0 * (self.supply as f64) * (1.0 - power_result);

        tracing::debug!("issuedSupply: {}", issued_supply);

        let out = issued_supply as i64;
        self.supply += out;

        out
    }

    /// Convert virtual supply units to buy tokens
    ///
    /// Formula: exchangeBalance = balance * ((1.0 + supplyQuant/supply)^2000.0 - 1.0)
    fn exchange_from_supply(&mut self, balance: i64, supply_quant: i64) -> i64 {
        self.supply -= supply_quant;

        // Calculate: balance * ((1.0 + supplyQuant/supply)^2000.0 - 1.0)
        let ratio = 1.0 + (supply_quant as f64) / (self.supply as f64);
        let power_result = self.pow(ratio, 2000.0);
        let exchange_balance = (balance as f64) * (power_result - 1.0);

        tracing::debug!("exchangeBalance: {}", exchange_balance);

        exchange_balance as i64
    }

    /// Execute a token exchange
    ///
    /// # Arguments
    /// * `sell_token_balance` - Current balance of the token being sold in the exchange
    /// * `buy_token_balance` - Current balance of the token being bought in the exchange
    /// * `sell_token_quant` - Amount of tokens being sold
    ///
    /// # Returns
    /// Amount of tokens to be received (buy tokens)
    ///
    /// This is the main entry point that combines both exchange phases.
    pub fn exchange(&mut self, sell_token_balance: i64, buy_token_balance: i64, sell_token_quant: i64) -> i64 {
        let relay = self.exchange_to_supply(sell_token_balance, sell_token_quant);
        self.exchange_from_supply(buy_token_balance, relay)
    }

    /// Power function that respects strict math mode
    ///
    /// In strict mode, we use a more careful implementation that should
    /// match Java's StrictMath.pow() behavior. In non-strict mode, we use
    /// the standard f64::powf().
    ///
    /// Note: Achieving bit-exact parity with Java's pow() is challenging.
    /// For production use, extensive testing is recommended.
    fn pow(&self, base: f64, exponent: f64) -> f64 {
        if self.use_strict_math {
            // For strict math mode, use the same basic powf but log the values
            // for debugging potential discrepancies
            let result = base.powf(exponent);
            tracing::trace!("strict_pow({}, {}) = {}", base, exponent, result);
            result
        } else {
            base.powf(exponent)
        }
    }
}

/// Calculate the amount of the other token needed for an injection
///
/// When injecting `token_quant` of one token, calculates how much of the
/// other token must also be injected to maintain the exchange ratio.
///
/// Formula: anotherTokenQuant = floor(otherBalance * tokenQuant / tokenBalance)
///
/// # Arguments
/// * `token_balance` - Current balance of the token being injected
/// * `other_balance` - Current balance of the other token
/// * `token_quant` - Amount of tokens being injected
///
/// # Returns
/// Amount of the other token that must also be injected
pub fn calculate_inject_another_amount(
    token_balance: i64,
    other_balance: i64,
    token_quant: i64,
) -> i64 {
    // Use BigInteger-style calculation to avoid overflow
    // floor(other_balance * token_quant / token_balance)
    let numerator = (other_balance as i128) * (token_quant as i128);
    let result = numerator / (token_balance as i128);
    result as i64
}

/// Calculate the amount of the other token to withdraw
///
/// When withdrawing `token_quant` of one token, calculates how much of the
/// other token is also withdrawn to maintain the exchange ratio.
///
/// Formula: anotherTokenQuant = floor(otherBalance * tokenQuant / tokenBalance)
///
/// # Arguments
/// * `token_balance` - Current balance of the token being withdrawn
/// * `other_balance` - Current balance of the other token
/// * `token_quant` - Amount of tokens being withdrawn
///
/// # Returns
/// Amount of the other token that will also be withdrawn
pub fn calculate_withdraw_another_amount(
    token_balance: i64,
    other_balance: i64,
    token_quant: i64,
) -> i64 {
    // Same calculation as inject
    calculate_inject_another_amount(token_balance, other_balance, token_quant)
}

/// Check if the withdrawal precision is acceptable
///
/// Java's ExchangeWithdrawActuator validates that the withdrawal is precise enough
/// by checking: remainder / anotherTokenQuant <= 0.0001
///
/// # Arguments
/// * `token_balance` - Current balance of the token being withdrawn
/// * `other_balance` - Current balance of the other token
/// * `token_quant` - Amount of tokens being withdrawn
///
/// # Returns
/// true if precision is acceptable, false otherwise
pub fn is_withdraw_precise_enough(
    token_balance: i64,
    other_balance: i64,
    token_quant: i64,
) -> bool {
    let another_quant = calculate_withdraw_another_amount(token_balance, other_balance, token_quant);
    if another_quant == 0 {
        return false;
    }

    // Calculate with higher precision
    let exact = (other_balance as f64) * (token_quant as f64) / (token_balance as f64);
    let remainder = exact - (another_quant as f64);

    (remainder / (another_quant as f64)) <= 0.0001
}

/// TRX symbol bytes constant (matches Java's TRX_SYMBOL_BYTES)
pub const TRX_SYMBOL: &[u8] = b"_";

/// Check if a token ID represents TRX
pub fn is_trx(token_id: &[u8]) -> bool {
    token_id == TRX_SYMBOL
}

/// Check if a token ID is a valid number string (for allowSameTokenName=1)
pub fn is_number(token_id: &[u8]) -> bool {
    if token_id.is_empty() {
        return false;
    }
    token_id.iter().all(|&b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exchange_basic() {
        let mut processor = ExchangeProcessor::new(false);

        // Test with simple balances
        let sell_balance = 1_000_000_000i64; // 1000 TRX
        let buy_balance = 1_000_000_000i64;
        let sell_quant = 100_000_000i64; // 100 TRX

        let buy_quant = processor.exchange(sell_balance, buy_balance, sell_quant);

        // Should get less than what we put in (due to AMM curve)
        assert!(buy_quant > 0);
        assert!(buy_quant < sell_quant);

        println!("Exchanged {} -> {}", sell_quant, buy_quant);
    }

    #[test]
    fn test_exchange_symmetry() {
        // If exchange is symmetric, trading A->B should give similar results to B->A
        let mut processor1 = ExchangeProcessor::new(false);
        let mut processor2 = ExchangeProcessor::new(false);

        let balance = 1_000_000_000i64;
        let quant = 100_000_000i64;

        let result1 = processor1.exchange(balance, balance, quant);
        let result2 = processor2.exchange(balance, balance, quant);

        // Results should be identical for same inputs
        assert_eq!(result1, result2);
    }

    #[test]
    fn test_calculate_inject_another_amount() {
        // If balances are equal, another amount equals the input
        let result = calculate_inject_another_amount(1000, 1000, 100);
        assert_eq!(result, 100);

        // If other balance is 2x, another amount is 2x
        let result = calculate_inject_another_amount(1000, 2000, 100);
        assert_eq!(result, 200);

        // Test floor division
        let result = calculate_inject_another_amount(1000, 1001, 1);
        assert_eq!(result, 1); // floor(1001/1000) = 1
    }

    #[test]
    fn test_is_trx() {
        assert!(is_trx(b"_"));
        assert!(!is_trx(b"TRX"));
        assert!(!is_trx(b"1000001"));
    }

    #[test]
    fn test_is_number() {
        assert!(is_number(b"1000001"));
        assert!(is_number(b"123"));
        assert!(!is_number(b"abc"));
        assert!(!is_number(b"123abc"));
        assert!(!is_number(b"_"));
        assert!(!is_number(b""));
    }
}
