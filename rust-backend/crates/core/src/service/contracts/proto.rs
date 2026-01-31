// Protobuf parsing utilities
// Shared protobuf decoding helpers

// =============================================================================
// Protobuf Error Taxonomy
// =============================================================================
// These constants match protobuf-java 3.21.12 InvalidProtocolBufferException messages.
// Used for exact string parity with java-tron error messages.

/// Truncated input (EOF mid-field / truncated varint / truncated length-delimited)
/// From: InvalidProtocolBufferException.truncatedMessage()
pub const PROTOBUF_TRUNCATED_MESSAGE: &str = "While parsing a protocol message, the input ended unexpectedly in the middle of a field.  This could mean either that the input has been truncated or that an embedded message misreported its own length.";

/// Malformed varint (too long, >10 bytes)
/// From: InvalidProtocolBufferException.malformedVarint()
pub const PROTOBUF_MALFORMED_VARINT: &str = "CodedInputStream encountered a malformed varint.";

/// Invalid tag (zero)
/// From: InvalidProtocolBufferException.invalidTag()
pub const PROTOBUF_INVALID_TAG_ZERO: &str = "Protocol message contained an invalid tag (zero).";

/// Invalid wire type (6 or 7)
/// From: InvalidProtocolBufferException.invalidWireType()
pub const PROTOBUF_INVALID_WIRE_TYPE: &str = "Protocol message tag had invalid wire type.";

/// Typed result for varint parsing
/// Allows callers to distinguish truncated vs malformed varints
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarintError {
    /// EOF before varint completed (continuation bit set on last byte)
    Truncated,
    /// Varint exceeds 10 bytes (64-bit limit)
    TooLong,
}

impl std::fmt::Display for VarintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VarintError::Truncated => write!(f, "Unexpected end of varint"),
            VarintError::TooLong => write!(f, "Varint too long"),
        }
    }
}

impl std::error::Error for VarintError {}

/// Typed result for protobuf parsing
/// Allows callers to map to exact protobuf-java error messages
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtobufError {
    /// Truncated input (EOF mid-field, truncated varint, truncated length-delimited)
    Truncated,
    /// Malformed varint (too long)
    MalformedVarint,
    /// Invalid tag (zero)
    InvalidTagZero,
    /// Invalid wire type (6 or 7)
    InvalidWireType(u8),
    /// Other errors (with message)
    Other(String),
}

impl ProtobufError {
    /// Convert to protobuf-java 3.21.12 error message for exact parity
    pub fn to_java_message(&self) -> String {
        match self {
            ProtobufError::Truncated => PROTOBUF_TRUNCATED_MESSAGE.to_string(),
            ProtobufError::MalformedVarint => PROTOBUF_MALFORMED_VARINT.to_string(),
            ProtobufError::InvalidTagZero => PROTOBUF_INVALID_TAG_ZERO.to_string(),
            ProtobufError::InvalidWireType(_) => PROTOBUF_INVALID_WIRE_TYPE.to_string(),
            ProtobufError::Other(msg) => msg.clone(),
        }
    }
}

impl std::fmt::Display for ProtobufError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_java_message())
    }
}

impl std::error::Error for ProtobufError {}

impl From<VarintError> for ProtobufError {
    fn from(err: VarintError) -> Self {
        match err {
            VarintError::Truncated => ProtobufError::Truncated,
            VarintError::TooLong => ProtobufError::MalformedVarint,
        }
    }
}

/// Parsed AccountUpdateContract fields
/// Corresponds to protocol.AccountUpdateContract protobuf message:
///   bytes account_name = 1;
///   bytes owner_address = 2;
#[derive(Debug, Default, Clone)]
pub struct AccountUpdateContractParams {
    pub account_name: Vec<u8>,
    pub owner_address: Vec<u8>,
}

/// Parse AccountUpdateContract from protobuf bytes
/// Wire format: field 1 = account_name (bytes), field 2 = owner_address (bytes)
pub(crate) fn parse_account_update_contract(data: &[u8]) -> Result<AccountUpdateContractParams, String> {
    let mut params = AccountUpdateContractParams::default();
    let mut pos = 0;

    while pos < data.len() {
        // Read tag (field_number << 3 | wire_type)
        let (tag, tag_len) = read_varint(&data[pos..])?;
        pos += tag_len;

        let field_number = (tag >> 3) as u32;
        let wire_type = (tag & 0x7) as u8;

        match (field_number, wire_type) {
            // Field 1: account_name (bytes, wire type 2 = length-delimited)
            (1, 2) => {
                let (len, len_bytes) = read_varint(&data[pos..])?;
                pos += len_bytes;
                let len = len as usize;
                if pos + len > data.len() {
                    return Err("Truncated account_name field".to_string());
                }
                params.account_name = data[pos..pos + len].to_vec();
                pos += len;
            }
            // Field 2: owner_address (bytes, wire type 2 = length-delimited)
            (2, 2) => {
                let (len, len_bytes) = read_varint(&data[pos..])?;
                pos += len_bytes;
                let len = len as usize;
                if pos + len > data.len() {
                    return Err("Truncated owner_address field".to_string());
                }
                params.owner_address = data[pos..pos + len].to_vec();
                pos += len;
            }
            // Skip unknown fields
            (_, 0) => {
                // Varint - skip
                let (_val, val_len) = read_varint(&data[pos..])?;
                pos += val_len;
            }
            (_, 1) => {
                // 64-bit fixed - skip 8 bytes
                if pos + 8 > data.len() {
                    return Err("Truncated 64-bit field".to_string());
                }
                pos += 8;
            }
            (_, 2) => {
                // Length-delimited - skip
                let (len, len_bytes) = read_varint(&data[pos..])?;
                pos += len_bytes;
                pos += len as usize;
            }
            (_, 5) => {
                // 32-bit fixed - skip 4 bytes
                if pos + 4 > data.len() {
                    return Err("Truncated 32-bit field".to_string());
                }
                pos += 4;
            }
            _ => {
                return Err(format!("Unknown wire type {} for field {}", wire_type, field_number));
            }
        }
    }

    Ok(params)
}

/// Read a protobuf varint from a byte slice
/// Returns (value, bytes_read)
pub(crate) fn read_varint(data: &[u8]) -> Result<(u64, usize), String> {
    let mut result: u64 = 0;
    let mut shift = 0;
    let mut pos = 0;

    loop {
        if pos >= data.len() {
            return Err("Unexpected end of varint".to_string());
        }

        let byte = data[pos];
        pos += 1;

        result |= ((byte & 0x7F) as u64) << shift;

        if (byte & 0x80) == 0 {
            return Ok((result, pos));
        }

        shift += 7;
        if shift >= 64 {
            return Err("Varint too long".to_string());
        }
    }
}

/// Read a protobuf varint with typed error reporting
/// Returns (value, bytes_read) on success, or typed error for parity with protobuf-java
pub(crate) fn read_varint_typed(data: &[u8]) -> Result<(u64, usize), VarintError> {
    let mut result: u64 = 0;
    let mut shift = 0;
    let mut pos = 0;

    loop {
        if pos >= data.len() {
            return Err(VarintError::Truncated);
        }

        let byte = data[pos];
        pos += 1;

        result |= ((byte & 0x7F) as u64) << shift;

        if (byte & 0x80) == 0 {
            return Ok((result, pos));
        }

        shift += 7;
        if shift >= 64 {
            return Err(VarintError::TooLong);
        }
    }
}

/// Skip a protobuf field with bounds checking and typed errors
/// Returns bytes to skip on success
pub(crate) fn skip_protobuf_field_checked(
    data: &[u8],
    wire_type: u8,
) -> Result<usize, ProtobufError> {
    match wire_type {
        0 => {
            // Varint - read and discard
            let (_, bytes_read) = read_varint_typed(data)?;
            Ok(bytes_read)
        }
        1 => {
            // 64-bit fixed - must have 8 bytes
            if data.len() < 8 {
                return Err(ProtobufError::Truncated);
            }
            Ok(8)
        }
        2 => {
            // Length-delimited - read length then skip that many bytes
            let (length, bytes_read) = read_varint_typed(data)?;
            let total = bytes_read + length as usize;
            if total > data.len() {
                return Err(ProtobufError::Truncated);
            }
            Ok(total)
        }
        5 => {
            // 32-bit fixed - must have 4 bytes
            if data.len() < 4 {
                return Err(ProtobufError::Truncated);
            }
            Ok(4)
        }
        // Wire types 3 and 4 are START_GROUP and END_GROUP (deprecated)
        // Wire types 6 and 7 are invalid
        3 | 4 => {
            // Groups are deprecated but valid wire types in protobuf
            // protobuf-java accepts them, but we don't implement group skipping
            // For simplicity, treat as invalid wire type (may diverge from java for groups)
            Err(ProtobufError::InvalidWireType(wire_type))
        }
        6 | 7 => {
            // Invalid wire types
            Err(ProtobufError::InvalidWireType(wire_type))
        }
        _ => {
            // Should not happen (wire_type is 3 bits), but handle gracefully
            Err(ProtobufError::InvalidWireType(wire_type))
        }
    }
}

/// Write a protobuf varint to a byte vector
pub(crate) fn write_varint(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80; // Set continuation bit
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

/// Write a signed int64 as zigzag-encoded varint (for sint64 fields)
#[allow(dead_code)]
pub(crate) fn write_sint64(buf: &mut Vec<u8>, value: i64) {
    // ZigZag encoding: (n << 1) ^ (n >> 63)
    let encoded = ((value << 1) ^ (value >> 63)) as u64;
    write_varint(buf, encoded);
}

/// Write a protobuf field tag (field_number << 3 | wire_type)
pub(crate) fn write_tag(buf: &mut Vec<u8>, field_number: u32, wire_type: u8) {
    let tag = ((field_number as u64) << 3) | (wire_type as u64);
    write_varint(buf, tag);
}

/// Transaction.Result protobuf builder
/// Matches Protocol.Transaction.Result message structure
///
/// Field numbers from Tron.proto Transaction.Result:
/// - int64 fee = 1;
/// - code ret = 2;
/// - contractResult contractRet = 3;
/// - string assetIssueID = 14;
/// - int64 withdraw_amount = 15;
/// - int64 unfreeze_amount = 16;
/// - int64 exchange_received_amount = 18;
/// - int64 exchange_inject_another_amount = 19;
/// - int64 exchange_withdraw_another_amount = 20;
/// - int64 exchange_id = 21;
/// - int64 shielded_transaction_fee = 22;
/// - bytes orderId = 25;
/// - repeated MarketOrderDetail orderDetails = 26;
/// - int64 withdraw_expire_amount = 27;
/// - map<string, int64> cancel_unfreezeV2_amount = 28;
#[derive(Default)]
pub struct TransactionResultBuilder {
    /// int64 fee = 1
    pub fee: Option<i64>,
    /// code ret = 2 (enum: SUCESS=0, FAILED=1, etc)
    pub ret: Option<i32>,
    pub withdraw_amount: Option<i64>,
    pub unfreeze_amount: Option<i64>,
    pub withdraw_expire_amount: Option<i64>,
    /// string assetIssueID = 14
    pub asset_issue_id: Option<String>,
    pub exchange_id: Option<i64>,
    pub exchange_received_amount: Option<i64>,
    pub exchange_inject_another_amount: Option<i64>,
    pub exchange_withdraw_another_amount: Option<i64>,
    pub shielded_transaction_fee: Option<i64>,
    /// bytes orderId = 25
    pub order_id: Option<Vec<u8>>,
    /// map<string, int64> cancel_unfreezeV2_amount = 28
    /// Keys are resource names: "BANDWIDTH", "ENERGY", "TRON_POWER"
    pub cancel_unfreezeV2_amount: Option<Vec<(String, i64)>>,
}

impl TransactionResultBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the fee (field 1)
    pub fn with_fee(mut self, fee: i64) -> Self {
        self.fee = Some(fee);
        self
    }

    /// Set the result code (field 2)
    /// 0 = SUCESS, 1 = FAILED
    #[allow(dead_code)]
    pub fn with_ret(mut self, ret: i32) -> Self {
        self.ret = Some(ret);
        self
    }

    pub fn with_withdraw_amount(mut self, amount: i64) -> Self {
        self.withdraw_amount = Some(amount);
        self
    }

    pub fn with_unfreeze_amount(mut self, amount: i64) -> Self {
        self.unfreeze_amount = Some(amount);
        self
    }

    pub fn with_withdraw_expire_amount(mut self, amount: i64) -> Self {
        self.withdraw_expire_amount = Some(amount);
        self
    }

    /// Set assetIssueID (field 14)
    pub fn with_asset_issue_id(mut self, asset_issue_id: &str) -> Self {
        self.asset_issue_id = Some(asset_issue_id.to_string());
        self
    }

    pub fn with_exchange_id(mut self, id: i64) -> Self {
        self.exchange_id = Some(id);
        self
    }

    pub fn with_exchange_received_amount(mut self, amount: i64) -> Self {
        self.exchange_received_amount = Some(amount);
        self
    }

    pub fn with_exchange_inject_another_amount(mut self, amount: i64) -> Self {
        self.exchange_inject_another_amount = Some(amount);
        self
    }

    pub fn with_exchange_withdraw_another_amount(mut self, amount: i64) -> Self {
        self.exchange_withdraw_another_amount = Some(amount);
        self
    }

    #[allow(dead_code)]
    pub fn with_shielded_transaction_fee(mut self, fee: i64) -> Self {
        self.shielded_transaction_fee = Some(fee);
        self
    }

    /// Set order ID for MarketSellAsset contract
    /// Field 25: bytes orderId
    pub fn with_order_id(mut self, order_id: &[u8]) -> Self {
        self.order_id = Some(order_id.to_vec());
        self
    }

    /// Set cancel_unfreezeV2_amount map for CancelAllUnfreezeV2 contract
    /// Takes amounts for bandwidth, energy, and tron_power
    ///
    /// Java parity note: The map always contains all 3 keys (even when value is 0).
    /// Order matches Java HashMap iteration: ENERGY, TRON_POWER, BANDWIDTH
    pub fn with_cancel_unfreeze_v2_amounts(mut self, bandwidth: i64, energy: i64, tron_power: i64) -> Self {
        // Always include all 3 keys in Java HashMap iteration order:
        // ENERGY, TRON_POWER, BANDWIDTH
        let amounts = vec![
            ("ENERGY".to_string(), energy),
            ("TRON_POWER".to_string(), tron_power),
            ("BANDWIDTH".to_string(), bandwidth),
        ];
        self.cancel_unfreezeV2_amount = Some(amounts);
        self
    }

    /// Build the Transaction.Result protobuf bytes
    pub fn build(self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Wire type 0 = varint for int64 fields
        const WIRE_TYPE_VARINT: u8 = 0;

        // Field 1: fee (int64)
        if let Some(fee) = self.fee {
            write_tag(&mut buf, 1, WIRE_TYPE_VARINT);
            write_varint(&mut buf, fee as u64);
        }

        // Field 2: ret (enum code, wire type 0 = varint)
        if let Some(ret) = self.ret {
            write_tag(&mut buf, 2, WIRE_TYPE_VARINT);
            write_varint(&mut buf, ret as u64);
        }

        // Field 14: assetIssueID (string, wire type 2 = length-delimited)
        if let Some(ref asset_issue_id) = self.asset_issue_id {
            write_tag(&mut buf, 14, 2);
            write_varint(&mut buf, asset_issue_id.len() as u64);
            buf.extend_from_slice(asset_issue_id.as_bytes());
        }

        // Field 15: withdraw_amount
        if let Some(amount) = self.withdraw_amount {
            write_tag(&mut buf, 15, WIRE_TYPE_VARINT);
            write_varint(&mut buf, amount as u64);
        }

        // Field 16: unfreeze_amount
        if let Some(amount) = self.unfreeze_amount {
            write_tag(&mut buf, 16, WIRE_TYPE_VARINT);
            write_varint(&mut buf, amount as u64);
        }

        // Field 18: exchange_received_amount
        if let Some(amount) = self.exchange_received_amount {
            write_tag(&mut buf, 18, WIRE_TYPE_VARINT);
            write_varint(&mut buf, amount as u64);
        }

        // Field 19: exchange_inject_another_amount
        if let Some(amount) = self.exchange_inject_another_amount {
            write_tag(&mut buf, 19, WIRE_TYPE_VARINT);
            write_varint(&mut buf, amount as u64);
        }

        // Field 20: exchange_withdraw_another_amount
        if let Some(amount) = self.exchange_withdraw_another_amount {
            write_tag(&mut buf, 20, WIRE_TYPE_VARINT);
            write_varint(&mut buf, amount as u64);
        }

        // Field 21: exchange_id
        if let Some(id) = self.exchange_id {
            write_tag(&mut buf, 21, WIRE_TYPE_VARINT);
            write_varint(&mut buf, id as u64);
        }

        // Field 22: shielded_transaction_fee
        if let Some(fee) = self.shielded_transaction_fee {
            write_tag(&mut buf, 22, WIRE_TYPE_VARINT);
            write_varint(&mut buf, fee as u64);
        }

        // Field 25: orderId (bytes, wire type 2 = length-delimited)
        if let Some(ref order_id) = self.order_id {
            write_tag(&mut buf, 25, 2);
            write_varint(&mut buf, order_id.len() as u64);
            buf.extend_from_slice(order_id);
        }

        // Field 27: withdraw_expire_amount
        if let Some(amount) = self.withdraw_expire_amount {
            write_tag(&mut buf, 27, WIRE_TYPE_VARINT);
            write_varint(&mut buf, amount as u64);
        }

        // Field 28: cancel_unfreezeV2_amount (map<string, int64>)
        // Protobuf map is encoded as repeated message with key=1, value=2
        if let Some(amounts) = self.cancel_unfreezeV2_amount {
            for (key, value) in amounts {
                // Each map entry is encoded as a length-delimited message
                let mut entry_buf = Vec::new();

                // Key (field 1, wire type 2 = length-delimited for string)
                write_tag(&mut entry_buf, 1, 2);
                write_varint(&mut entry_buf, key.len() as u64);
                entry_buf.extend_from_slice(key.as_bytes());

                // Value (field 2, wire type 0 = varint for int64)
                write_tag(&mut entry_buf, 2, WIRE_TYPE_VARINT);
                write_varint(&mut entry_buf, value as u64);

                // Write the map entry as field 28, wire type 2 (length-delimited)
                write_tag(&mut buf, 28, 2);
                write_varint(&mut buf, entry_buf.len() as u64);
                buf.extend_from_slice(&entry_buf);
            }
        }

        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_account_update_contract_basic() {
        // Build a simple AccountUpdateContract proto:
        // field 1 (account_name) = "TestName"
        // field 2 (owner_address) = [0x41, 0x01..0x01] (21 bytes)
        let mut data = Vec::new();

        // Field 1: account_name (tag = (1 << 3) | 2 = 10)
        data.push(10); // tag
        data.push(8);  // length
        data.extend_from_slice(b"TestName");

        // Field 2: owner_address (tag = (2 << 3) | 2 = 18)
        data.push(18); // tag
        data.push(21); // length
        data.push(0x41); // TRON prefix
        data.extend_from_slice(&[1u8; 20]);

        let result = parse_account_update_contract(&data).unwrap();
        assert_eq!(result.account_name, b"TestName");
        assert_eq!(result.owner_address.len(), 21);
        assert_eq!(result.owner_address[0], 0x41);
    }

    #[test]
    fn test_parse_account_update_contract_empty_name() {
        // AccountUpdateContract with empty account_name (allowed by Java)
        let mut data = Vec::new();

        // Only field 2: owner_address
        data.push(18); // tag
        data.push(21); // length
        data.push(0x41);
        data.extend_from_slice(&[2u8; 20]);

        let result = parse_account_update_contract(&data).unwrap();
        assert!(result.account_name.is_empty(), "Empty name should be allowed");
        assert_eq!(result.owner_address.len(), 21);
    }

    #[test]
    fn test_parse_account_update_contract_empty_data() {
        // Empty protobuf should parse to defaults
        let result = parse_account_update_contract(&[]).unwrap();
        assert!(result.account_name.is_empty());
        assert!(result.owner_address.is_empty());
    }

    #[test]
    fn test_parse_account_update_contract_truncated() {
        // Truncated length-delimited field should fail
        let mut data = Vec::new();
        data.push(10); // tag for field 1
        data.push(100); // length says 100 bytes
        data.extend_from_slice(b"short"); // but only 5 bytes

        let result = parse_account_update_contract(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Truncated"));
    }

    #[test]
    fn test_write_varint() {
        let mut buf = Vec::new();
        write_varint(&mut buf, 150);
        assert_eq!(buf, vec![0x96, 0x01]);

        let mut buf = Vec::new();
        write_varint(&mut buf, 1);
        assert_eq!(buf, vec![0x01]);

        let mut buf = Vec::new();
        write_varint(&mut buf, 300);
        assert_eq!(buf, vec![0xac, 0x02]);
    }

    #[test]
    fn test_transaction_result_builder_withdraw_amount() {
        let result = TransactionResultBuilder::new()
            .with_withdraw_amount(1000000)
            .build();

        // Field 15, wire type 0: tag = (15 << 3) | 0 = 120 = 0x78
        // Value 1000000 = 0xF4240 encoded as varint
        assert!(!result.is_empty());
        assert_eq!(result[0], 0x78); // tag for field 15, wire type 0
    }

    #[test]
    fn test_transaction_result_builder_unfreeze_amount() {
        let result = TransactionResultBuilder::new()
            .with_unfreeze_amount(5000000)
            .build();

        // Field 16, wire type 0: tag = (16 << 3) | 0 = 128 = 0x80 0x01
        assert!(!result.is_empty());
        assert_eq!(result[0], 0x80); // first byte of tag
        assert_eq!(result[1], 0x01); // second byte of tag (varint continuation)
    }

    #[test]
    fn test_transaction_result_builder_multiple_fields() {
        let result = TransactionResultBuilder::new()
            .with_withdraw_amount(1000)
            .with_unfreeze_amount(2000)
            .build();

        // Should contain both fields
        assert!(result.len() > 4);
    }

    #[test]
    fn test_transaction_result_builder_asset_issue_id() {
        let asset_issue_id = "1000001";
        let result = TransactionResultBuilder::new()
            .with_asset_issue_id(asset_issue_id)
            .build();

        // Field 14, wire type 2: tag = (14 << 3) | 2 = 114 = 0x72
        assert!(!result.is_empty());
        assert_eq!(result[0], 0x72);
        assert!(result.windows(asset_issue_id.len()).any(|w| w == asset_issue_id.as_bytes()));
    }

    #[test]
    fn test_cancel_unfreeze_v2_all_keys_present_when_values_zero() {
        // Java parity: map should contain all 3 keys even when values are 0
        let result = TransactionResultBuilder::new()
            .with_cancel_unfreeze_v2_amounts(0, 0, 0)
            .build();

        // Verify all 3 resource types are present
        let bytes_str = String::from_utf8_lossy(&result);
        assert!(bytes_str.contains("ENERGY"), "ENERGY key should be present");
        assert!(bytes_str.contains("TRON_POWER"), "TRON_POWER key should be present");
        assert!(bytes_str.contains("BANDWIDTH"), "BANDWIDTH key should be present");
    }

    #[test]
    fn test_cancel_unfreeze_v2_partial_values() {
        // Even if only one resource has a non-zero value, all 3 keys should be present
        let result = TransactionResultBuilder::new()
            .with_cancel_unfreeze_v2_amounts(1000000, 0, 0)
            .build();

        let bytes_str = String::from_utf8_lossy(&result);
        assert!(bytes_str.contains("ENERGY"), "ENERGY key should be present");
        assert!(bytes_str.contains("TRON_POWER"), "TRON_POWER key should be present");
        assert!(bytes_str.contains("BANDWIDTH"), "BANDWIDTH key should be present");
    }

    #[test]
    fn test_cancel_unfreeze_v2_key_order() {
        // Java parity: order should be ENERGY, TRON_POWER, BANDWIDTH
        let result = TransactionResultBuilder::new()
            .with_cancel_unfreeze_v2_amounts(100, 200, 300)
            .build();

        // Find positions of keys in serialized bytes
        let energy_pos = result.windows(6).position(|w| w == b"ENERGY");
        let tron_power_pos = result.windows(10).position(|w| w == b"TRON_POWER");
        let bandwidth_pos = result.windows(9).position(|w| w == b"BANDWIDTH");

        assert!(energy_pos.is_some(), "ENERGY not found");
        assert!(tron_power_pos.is_some(), "TRON_POWER not found");
        assert!(bandwidth_pos.is_some(), "BANDWIDTH not found");

        // Verify order: ENERGY < TRON_POWER < BANDWIDTH
        let e = energy_pos.unwrap();
        let t = tron_power_pos.unwrap();
        let b = bandwidth_pos.unwrap();
        assert!(e < t, "ENERGY should come before TRON_POWER (got {} vs {})", e, t);
        assert!(t < b, "TRON_POWER should come before BANDWIDTH (got {} vs {})", t, b);
    }

    #[test]
    fn test_withdraw_expire_amount_absent_when_zero() {
        // Java parity: withdraw_expire_amount (field 27) should NOT be encoded when 0
        // This is handled at the call site, but verify the builder doesn't encode when not set
        let result = TransactionResultBuilder::new()
            .with_cancel_unfreeze_v2_amounts(100, 0, 0)
            .build();

        // Field 27 tag = (27 << 3) | 0 = 216 = 0xD8 0x01
        // If field 27 were present, we'd see 0xD8 0x01 in the bytes
        let has_field_27 = result.windows(2).any(|w| w == [0xD8, 0x01]);
        assert!(!has_field_27, "Field 27 (withdraw_expire_amount) should not be present when not set");
    }

    #[test]
    fn test_withdraw_expire_amount_present_when_nonzero() {
        // Field 27 should be present when value is non-zero
        let result = TransactionResultBuilder::new()
            .with_withdraw_expire_amount(5000000000)
            .with_cancel_unfreeze_v2_amounts(0, 0, 0)
            .build();

        // Field 27 tag = (27 << 3) | 0 = 216 = 0xD8 0x01
        let has_field_27 = result.windows(2).any(|w| w == [0xD8, 0x01]);
        assert!(has_field_27, "Field 27 (withdraw_expire_amount) should be present when non-zero");
    }

    #[test]
    fn test_cancel_unfreeze_v2_matches_fixture_happy_path() {
        // Verify encoding matches fixture: conformance/fixtures/cancel_all_unfreeze_v2_contract/happy_path/expected/result.pb
        // This is: ENERGY=0, TRON_POWER=0, BANDWIDTH=5000000000 (0x80e497d012 varint)
        let result = TransactionResultBuilder::new()
            .with_cancel_unfreeze_v2_amounts(5000000000, 0, 0)  // bandwidth=5B, energy=0, tron_power=0
            .build();

        // Expected fixture bytes (without withdraw_expire_amount since it's 0)
        // From: xxd -p conformance/fixtures/cancel_all_unfreeze_v2_contract/happy_path/expected/result.pb
        let expected = hex::decode("e2010a0a06454e455247591000e2010e0a0a54524f4e5f504f5745521000e201110a0942414e4457494454481080e497d012").unwrap();
        assert_eq!(result, expected, "Encoded bytes should match fixture exactly");
    }

    #[test]
    fn test_cancel_unfreeze_v2_matches_fixture_all_expired() {
        // Verify encoding matches fixture: conformance/fixtures/cancel_all_unfreeze_v2_contract/edge_all_entries_expired_withdraw_only/expected/result.pb
        // This is: withdraw_expire_amount=6000000000 (field 27), ENERGY=0, TRON_POWER=0, BANDWIDTH=0
        let result = TransactionResultBuilder::new()
            .with_withdraw_expire_amount(6000000000)
            .with_cancel_unfreeze_v2_amounts(0, 0, 0)
            .build();

        // From: xxd -p conformance/fixtures/cancel_all_unfreeze_v2_contract/edge_all_entries_expired_withdraw_only/expected/result.pb
        let expected = hex::decode("d80180f882ad16e2010a0a06454e455247591000e2010e0a0a54524f4e5f504f5745521000e2010d0a0942414e4457494454481000").unwrap();
        assert_eq!(result, expected, "Encoded bytes should match fixture exactly");
    }
}
