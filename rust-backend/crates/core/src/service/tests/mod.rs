// Test module declarations
#[cfg(test)]
mod bandwidth;
#[cfg(test)]
mod contracts;
#[cfg(test)]
mod helpers;
#[cfg(test)]
mod integration;

// Note: contracts.rs was split into contracts/ subdirectory with separate files per contract type

#[test]
fn signed_java_long_balance_round_trips_through_u256() {
    let old_balance = -9_223_372_036_854_075_808i64;
    let fee = 9_999_000_000i64;
    let new_balance = old_balance + fee;

    let encoded = super::i64_to_u256(new_balance);

    assert_eq!(encoded.as_limbs()[0], new_balance as u64);
    assert_eq!(super::u256_to_i64(encoded).unwrap(), new_balance);
}
