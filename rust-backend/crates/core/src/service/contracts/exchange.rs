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
//! bit-for-bit identical results across all platforms. Java's StrictMath is
//! based on fdlibm (Freely Distributable Math Library).
//!
//! In Rust, when strict mode is enabled, we use the `rust-strictmath` crate
//! which is also based on fdlibm, providing the same deterministic behavior
//! as Java's StrictMath.pow().
//!
//! When strict mode is disabled, we use the standard `f64::powf()` which may
//! have slight platform variations (matching Java's Math.pow() behavior).
//!
//! ## References
//!
//! - Java ExchangeProcessor: chainbase/src/main/java/org/tron/core/capsule/ExchangeProcessor.java
//! - Java ExchangeCapsule: chainbase/src/main/java/org/tron/core/capsule/ExchangeCapsule.java
//! - Java StrictMath: uses fdlibm (https://www.netlib.org/fdlibm/)
//! - rust-strictmath: fdlibm port for Rust (https://github.com/loyispa/rust-strictmath)

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
    pub fn exchange(
        &mut self,
        sell_token_balance: i64,
        buy_token_balance: i64,
        sell_token_quant: i64,
    ) -> i64 {
        let relay = self.exchange_to_supply(sell_token_balance, sell_token_quant);
        self.exchange_from_supply(buy_token_balance, relay)
    }

    /// Power function that respects strict math mode
    ///
    /// In strict mode, we use `rust-strictmath::pow()` which is based on fdlibm,
    /// the same library that Java's StrictMath.pow() uses. This guarantees
    /// bit-for-bit identical results across all platforms.
    ///
    /// In non-strict mode, we use the standard `f64::powf()` which may have
    /// slight platform variations (matching Java's Math.pow() behavior).
    fn pow(&self, base: f64, exponent: f64) -> f64 {
        if self.use_strict_math {
            // Use fdlibm-based pow for cross-platform determinism
            // This matches Java's StrictMath.pow() behavior
            let result = rust_strictmath::pow(base, exponent);
            tracing::trace!("strict_pow({}, {}) = {} (fdlibm)", base, exponent, result);
            result
        } else {
            // Use platform-native pow (may vary slightly across platforms)
            // This matches Java's Math.pow() behavior
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

/// Calculate the amount of the other token needed for an injection using Java's `Math.multiplyExact`
/// semantics.
///
/// Java's `ExchangeInjectActuator.execute()` uses:
/// `floorDiv(multiplyExact(otherBalance, tokenQuant), tokenBalance)`
///
/// This can overflow even when the final division result would fit in a long.
pub fn calculate_inject_another_amount_multiply_exact(
    token_balance: i64,
    other_balance: i64,
    token_quant: i64,
) -> Result<i64, String> {
    let product = other_balance
        .checked_mul(token_quant)
        .ok_or_else(|| "Unexpected error: long overflow".to_string())?;
    Ok(product.div_euclid(token_balance))
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
/// using BigDecimal with 4 decimal places and ROUND_HALF_UP rounding:
///
/// ```java
/// double remainder = bigOtherBalance.multiply(bigTokenQuant)
///     .divide(bigTokenBalance, 4, BigDecimal.ROUND_HALF_UP).doubleValue()
///     - anotherTokenQuant;
/// if (remainder / anotherTokenQuant > 0.0001) {
///     throw new ContractValidateException("Not precise enough");
/// }
/// ```
///
/// To match Java's semantics exactly using integer math:
/// 1. another = floor(otherBalance * tokenQuant / tokenBalance)
/// 2. q4_scaled = round_half_up((otherBalance * tokenQuant * 10000) / tokenBalance)
/// 3. remainder_scaled = q4_scaled - (another * 10000)
/// 4. Reject if remainder_scaled > another (equivalent to remainder/another > 0.0001)
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
    let another_quant =
        calculate_withdraw_another_amount(token_balance, other_balance, token_quant);
    if another_quant <= 0 {
        return false;
    }

    // Use i128 to avoid overflow in intermediate calculations
    let numerator = (other_balance as i128) * (token_quant as i128);
    let denominator = token_balance as i128;

    // Calculate another_quant as floor(numerator / denominator) - already done above
    // Now calculate q4_scaled = round_half_up((numerator * 10000) / denominator)
    //
    // Java's BigDecimal.ROUND_HALF_UP rounds toward "nearest neighbor" unless both
    // neighbors are equidistant, in which case round up.
    // For positive numbers: add half of divisor before integer division
    // round_half_up(a/b) = (a + b/2) / b = (2*a + b) / (2*b)

    let scaled_numerator = numerator * 10000;
    // For half-up rounding: (2 * scaled_numerator + denominator) / (2 * denominator)
    let q4_scaled = (2 * scaled_numerator + denominator) / (2 * denominator);

    // remainder_scaled = q4_scaled - (another * 10000)
    let another_scaled = (another_quant as i128) * 10000;
    let remainder_scaled = q4_scaled - another_scaled;

    // Java rejects if: remainder / another > 0.0001
    // In scaled integers: remainder_scaled / 10000 / another > 0.0001
    //                   = remainder_scaled / another > 1
    //                   = remainder_scaled > another
    // But Java uses strict >, so we accept if remainder_scaled <= another
    remainder_scaled <= (another_quant as i128)
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
    if token_id.len() > 1 && token_id[0] == b'0' {
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

    #[test]
    fn test_strict_math_determinism() {
        // Test that strict math produces consistent results
        let mut processor1 = ExchangeProcessor::new(true); // strict math
        let mut processor2 = ExchangeProcessor::new(true); // strict math

        let sell_balance = 1_000_000_000i64;
        let buy_balance = 1_000_000_000i64;
        let sell_quant = 100_000_000i64;

        let result1 = processor1.exchange(sell_balance, buy_balance, sell_quant);
        let result2 = processor2.exchange(sell_balance, buy_balance, sell_quant);

        // Strict math should always produce identical results
        assert_eq!(result1, result2, "Strict math should be deterministic");
        println!("Strict math exchange result: {} -> {}", sell_quant, result1);
    }

    #[test]
    fn test_strict_vs_non_strict_math() {
        // Compare strict math vs non-strict math results
        // They should be close but may differ slightly on some platforms
        let mut processor_strict = ExchangeProcessor::new(true);
        let mut processor_normal = ExchangeProcessor::new(false);

        let sell_balance = 1_000_000_000i64;
        let buy_balance = 1_000_000_000i64;
        let sell_quant = 100_000_000i64;

        let result_strict = processor_strict.exchange(sell_balance, buy_balance, sell_quant);
        let result_normal = processor_normal.exchange(sell_balance, buy_balance, sell_quant);

        println!("Strict math result: {}", result_strict);
        println!("Normal math result: {}", result_normal);

        // Results should be very close (within a small margin due to floating-point)
        // but may not be exactly equal on all platforms
        let diff = (result_strict - result_normal).abs();
        assert!(
            diff <= 1,
            "Strict and normal math should produce very similar results, diff: {}",
            diff
        );
    }

    #[test]
    fn test_fdlibm_pow_known_values() {
        // Test fdlibm pow with known values to verify it matches expected behavior
        // These are the exponents used in the exchange algorithm
        let base = 1.1; // Example ratio

        let strict_result_0005 = rust_strictmath::pow(base, 0.0005);
        let strict_result_2000 = rust_strictmath::pow(base, 2000.0);

        println!("fdlibm pow({}, 0.0005) = {:.17}", base, strict_result_0005);
        println!("fdlibm pow({}, 2000.0) = {:.17}", base, strict_result_2000);

        // Verify the results are reasonable
        assert!(strict_result_0005 > 1.0 && strict_result_0005 < 1.001);
        assert!(strict_result_2000 > 1.0); // Large positive power should be > 1
    }

    // =====================================================================
    // Tests for is_withdraw_precise_enough()
    // These tests verify Java BigDecimal 4dp half-up rounding parity
    // =====================================================================

    #[test]
    fn test_withdraw_precision_basic_equal_balances() {
        // Equal balances: withdrawing 100 from 1000/1000 should give another=100
        // remainder = 0, always passes
        assert!(is_withdraw_precise_enough(1000, 1000, 100));
    }

    #[test]
    fn test_withdraw_precision_exact_division() {
        // Exact division: 100 * 200 / 1000 = 20, no remainder
        assert!(is_withdraw_precise_enough(1000, 200, 100));
    }

    #[test]
    fn test_withdraw_precision_small_remainder_passes() {
        // Small remainder that should pass
        // token_balance=10000, other_balance=10001, quant=1
        // another = floor(10001 * 1 / 10000) = 1
        // exact ratio = 1.0001, which is <= 0.0001 relative error
        assert!(is_withdraw_precise_enough(10000, 10001, 1));
    }

    #[test]
    fn test_withdraw_precision_another_zero_fails() {
        // another=0 should always fail
        // quant too small to produce any output
        assert!(!is_withdraw_precise_enough(1000000, 1, 1));
    }

    #[test]
    fn test_withdraw_precision_large_another_always_passes() {
        // When another >= 10000, the relative error threshold becomes >= 1.0
        // so it should always pass
        // token_balance=1000, other_balance=10000000, quant=1
        // another = floor(10000000 * 1 / 1000) = 10000
        assert!(is_withdraw_precise_enough(1000, 10000000, 1));
    }

    #[test]
    fn test_withdraw_precision_boundary_0001_exact() {
        // Test at exactly 0.0001 threshold
        // We need: remainder / another == 0.0001
        // If another = 10000, then remainder = 1 gives exactly 0.0001
        //
        // Find values where: floor(other * quant / token) = 10000
        // and: round_half_up((other * quant / token), 4) - 10000 = 0.0001 * 10000 = 1
        //
        // other=100010000, token=10000, quant=1
        // another = floor(100010000 / 10000) = 10001
        // q4 = round(100010000/10000, 4) = round(10001.0, 4) = 10001.0
        // remainder = 10001.0 - 10001 = 0
        // This passes (0 <= 0.0001)

        // Let's try: other=100001, token=10000, quant=1
        // another = floor(100001 / 10000) = 10
        // exact = 10.0001
        // q4_scaled = round_half_up(100001 * 10000 / 10000) = round_half_up(100001) = 100010 (in 4dp scaled)
        // Wait, let me recalculate with proper scaling
        // numerator = 100001 * 1 = 100001
        // another = 100001 / 10000 = 10
        // scaled_numerator = 100001 * 10000 = 1000010000
        // q4_scaled = (2 * 1000010000 + 10000) / (2 * 10000) = (2000020000 + 10000) / 20000 = 2000030000 / 20000 = 100001 (rounded to 100001)
        // Hmm, this is the value before the decimal shift
        // Let me verify: exact = 100001/10000 = 10.0001
        // q4 (4dp) = 10.0001 (no rounding needed, already 4 decimals)
        // q4_scaled = 100001 (representing 10.0001 * 10000)
        // another_scaled = 10 * 10000 = 100000
        // remainder_scaled = 100001 - 100000 = 1
        // Condition: remainder_scaled <= another? 1 <= 10? YES -> passes
        assert!(is_withdraw_precise_enough(10000, 100001, 1));
    }

    #[test]
    fn test_withdraw_precision_boundary_just_above_0001() {
        // remainder / another > 0.0001 should fail
        // another = 1, remainder > 0.0001 (i.e., > 0.0001 * 1 = 0.0001)
        //
        // Example: token_balance=10000, other_balance=10002, quant=1
        // another = floor(10002/10000) = 1
        // exact = 1.0002
        // q4 = round(1.0002, 4, HALF_UP) = 1.0002
        // remainder = 1.0002 - 1 = 0.0002
        // 0.0002 / 1 = 0.0002 > 0.0001 -> FAIL
        //
        // In scaled integers:
        // numerator = 10002
        // scaled_numerator = 100020000
        // q4_scaled = (2 * 100020000 + 10000) / 20000 = (200040000 + 10000) / 20000 = 200050000 / 20000 = 10002 (scaled: 10002 representing 1.0002)
        // another_scaled = 1 * 10000 = 10000
        // remainder_scaled = 10002 - 10000 = 2
        // 2 > 1? YES -> FAIL
        assert!(!is_withdraw_precise_enough(10000, 10002, 1));
    }

    #[test]
    fn test_withdraw_precision_boundary_half_up_rounding_effect() {
        // Test case where Java's half-up rounding makes a difference
        //
        // Find a case where the 5th decimal is >= 5, causing round-up
        // that pushes remainder from passing to failing (or vice versa)
        //
        // token_balance=10000, other_balance=10001, quant=1
        // exact = 1.0001
        // another = 1
        // q4 = 1.0001 (no rounding effect on 5th decimal since it's 0)
        // remainder = 0.0001
        // 0.0001 / 1 = 0.0001, which is NOT > 0.0001 -> PASS
        assert!(is_withdraw_precise_enough(10000, 10001, 1));

        // Now try with 5th decimal = 5 (exactly at half)
        // token_balance = 200000, other_balance = 200003, quant = 1
        // another = floor(200003/200000) = 1
        // exact = 1.000015
        // q4 = round(1.000015, 4, HALF_UP) = 1.0000 (rounds down since 15/10000 = 0.00015 -> 0.0002 when rounded at 4dp? Let me recalc)
        // Actually: 200003/200000 = 1.000015
        // To 4 decimals: 1.0000 | 15... The 5th digit is 1, so no rounding up at 4dp
        // q4 = 1.0000
        // remainder = 1.0000 - 1 = 0
        // 0 / 1 = 0 <= 0.0001 -> PASS
        assert!(is_withdraw_precise_enough(200000, 200003, 1));
    }

    #[test]
    fn test_withdraw_precision_half_up_rounds_up() {
        // Case where 5th decimal >= 5 causes round-up at 4dp
        // token_balance = 20000, other_balance = 20001, quant = 1
        // another = floor(20001/20000) = 1
        // exact = 1.00005
        // q4 = round(1.00005, 4, HALF_UP) = 1.0001 (5th decimal is 5, round up)
        // remainder = 1.0001 - 1 = 0.0001
        // 0.0001 / 1 = 0.0001 NOT > 0.0001 -> PASS
        //
        // scaled: numerator = 20001, scaled_num = 200010000
        // q4_scaled = (2 * 200010000 + 20000) / 40000 = (400020000 + 20000) / 40000 = 400040000 / 40000 = 10001
        // another_scaled = 10000
        // remainder_scaled = 10001 - 10000 = 1
        // 1 <= 1? YES -> PASS
        assert!(is_withdraw_precise_enough(20000, 20001, 1));
    }

    #[test]
    fn test_withdraw_precision_half_up_critical_boundary() {
        // The critical test: find where Java's 4dp half-up rounding
        // quantizes a value that would fail with exact float to pass
        //
        // Example: true remainder = 0.00014999...
        // With exact float: 0.00014999 > 0.0001 -> would FAIL
        // With Java 4dp HALF_UP: rounds to 0.0001 -> PASS
        //
        // token_balance = 100000, other_balance = 100015, quant = 1
        // another = floor(100015/100000) = 1
        // exact = 1.00015
        // q4 (HALF_UP) = 1.0002 (5th digit is 5, rounds up)
        // remainder = 1.0002 - 1 = 0.0002
        // 0.0002 / 1 = 0.0002 > 0.0001 -> FAIL
        assert!(!is_withdraw_precise_enough(100000, 100015, 1));

        // Now test 0.00014 which rounds down to 0.0001
        // token_balance = 500000, other_balance = 500007, quant = 1
        // exact = 1.000014
        // q4 (HALF_UP) = 1.0000 (5th digit is 1, rounds down)
        // remainder = 0
        // PASS
        assert!(is_withdraw_precise_enough(500000, 500007, 1));
    }

    #[test]
    fn test_withdraw_precision_large_values() {
        // Test with large production-like values
        // Common exchange balances might be in TRX (10^6 SUN per TRX)
        let token_balance = 1_000_000_000_000i64; // 1M TRX
        let other_balance = 500_000_000_000i64; // 500K TRX
        let quant = 1_000_000_000i64; // 1K TRX

        // another = floor(500_000_000_000 * 1_000_000_000 / 1_000_000_000_000)
        //         = floor(500_000_000) = 500_000_000
        // This is exact division, should pass
        assert!(is_withdraw_precise_enough(
            token_balance,
            other_balance,
            quant
        ));
    }

    #[test]
    fn test_withdraw_precision_real_world_scenario() {
        // Simulate a real withdrawal where rounding matters
        // token_balance = 12345678, other_balance = 87654, quant = 1000
        // another = floor(87654 * 1000 / 12345678) = floor(7.1007...) = 7
        let another = calculate_withdraw_another_amount(12345678, 87654, 1000);
        assert_eq!(another, 7);

        // exact = 87654000 / 12345678 = 7.1007...
        // q4 = round(7.1007, 4, HALF_UP) = 7.1007
        // remainder = 7.1007 - 7 = 0.1007
        // 0.1007 / 7 = 0.0143... > 0.0001 -> FAIL (too imprecise)
        assert!(!is_withdraw_precise_enough(12345678, 87654, 1000));

        // Try a more balanced example
        // token_balance = 1000000, other_balance = 999999, quant = 1000
        // another = floor(999999 * 1000 / 1000000) = floor(999.999) = 999
        let another2 = calculate_withdraw_another_amount(1000000, 999999, 1000);
        assert_eq!(another2, 999);

        // exact = 999999000 / 1000000 = 999.999
        // q4 = round(999.999, 4, HALF_UP) = 999.9990
        // remainder = 999.999 - 999 = 0.999
        // 0.999 / 999 = 0.001 > 0.0001 -> FAIL
        assert!(!is_withdraw_precise_enough(1000000, 999999, 1000));

        // Now with larger quant to reduce relative error
        // token_balance = 1000000, other_balance = 1000000, quant = 500000
        // another = floor(1000000 * 500000 / 1000000) = 500000
        let another3 = calculate_withdraw_another_amount(1000000, 1000000, 500000);
        assert_eq!(another3, 500000);
        // Exact division, always passes
        assert!(is_withdraw_precise_enough(1000000, 1000000, 500000));
    }
}
