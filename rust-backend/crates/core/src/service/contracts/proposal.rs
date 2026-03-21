//! Proposal parameter validation module.
//!
//! This module provides Java-parity validation for PROPOSAL_CREATE_CONTRACT.
//! It implements the same validation rules as Java's `ProposalUtil.validator()`.
//!
//! Java references:
//! - `actuator/src/main/java/org/tron/core/utils/ProposalUtil.java`
//! - `actuator/src/main/java/org/tron/core/actuator/ProposalCreateActuator.java`
//! - `chainbase/src/main/java/org/tron/common/utils/ForkController.java`

use tron_backend_execution::EngineBackedEvmStateStore;

// Java constants from ProposalUtil.java
pub const LONG_VALUE: i64 = 100_000_000_000_000_000;
pub const MAX_SUPPLY: i64 = 100_000_000_000;
pub const ONE_YEAR_BLOCK_NUMBERS: i64 = 10_512_000;
pub const CREATE_ACCOUNT_TRANSACTION_MIN_BYTE_SIZE: i64 = 500;
pub const CREATE_ACCOUNT_TRANSACTION_MAX_BYTE_SIZE: i64 = 10000;
pub const DYNAMIC_ENERGY_INCREASE_FACTOR_RANGE: i64 = 10_000;
pub const DYNAMIC_ENERGY_MAX_FACTOR_RANGE: i64 = 100_000;

// Fork version constants from Parameter.java
pub const VERSION_3_2_2: i32 = 6;
pub const VERSION_3_5: i32 = 7;
pub const VERSION_3_6: i32 = 8;
pub const VERSION_3_6_5: i32 = 9;
pub const VERSION_3_6_6: i32 = 10;
pub const VERSION_4_0: i32 = 16;
pub const VERSION_4_0_1: i32 = 17;
pub const VERSION_4_1: i32 = 19;
pub const VERSION_4_1_2: i32 = 20;
pub const VERSION_4_2: i32 = 21;
pub const VERSION_4_3: i32 = 22;
pub const VERSION_4_4: i32 = 23;
pub const VERSION_4_5: i32 = 24;
pub const VERSION_4_6: i32 = 25;
pub const VERSION_4_7: i32 = 26;
pub const VERSION_4_7_2: i32 = 28;
pub const VERSION_4_7_4: i32 = 29;
pub const VERSION_4_7_5: i32 = 30;
pub const VERSION_4_7_7: i32 = 31;
pub const VERSION_4_8_0: i32 = 32;
pub const ENERGY_LIMIT: i32 = 5;

/// Error messages matching Java's ProposalUtil.java
const BAD_PARAM_ID: &str = "Bad chain parameter id";
const LONG_VALUE_ERROR: &str = "Bad chain parameter value, valid range is [0,100000000000000000]";
const MAX_SUPPLY_ERROR: &str = "Bad chain parameter value, valid range is [0, 100_000_000_000L]";

/// Proposal type codes matching Java's ProposalType enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum ProposalType {
    MAINTENANCE_TIME_INTERVAL = 0,
    ACCOUNT_UPGRADE_COST = 1,
    CREATE_ACCOUNT_FEE = 2,
    TRANSACTION_FEE = 3,
    ASSET_ISSUE_FEE = 4,
    WITNESS_PAY_PER_BLOCK = 5,
    WITNESS_STANDBY_ALLOWANCE = 6,
    CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT = 7,
    CREATE_NEW_ACCOUNT_BANDWIDTH_RATE = 8,
    ALLOW_CREATION_OF_CONTRACTS = 9,
    REMOVE_THE_POWER_OF_THE_GR = 10,
    ENERGY_FEE = 11,
    EXCHANGE_CREATE_FEE = 12,
    MAX_CPU_TIME_OF_ONE_TX = 13,
    ALLOW_UPDATE_ACCOUNT_NAME = 14,
    ALLOW_SAME_TOKEN_NAME = 15,
    ALLOW_DELEGATE_RESOURCE = 16,
    TOTAL_ENERGY_LIMIT = 17,
    ALLOW_TVM_TRANSFER_TRC10 = 18,
    TOTAL_CURRENT_ENERGY_LIMIT = 19,
    ALLOW_MULTI_SIGN = 20,
    ALLOW_ADAPTIVE_ENERGY = 21,
    UPDATE_ACCOUNT_PERMISSION_FEE = 22,
    MULTI_SIGN_FEE = 23,
    ALLOW_PROTO_FILTER_NUM = 24,
    ALLOW_ACCOUNT_STATE_ROOT = 25,
    ALLOW_TVM_CONSTANTINOPLE = 26,
    ADAPTIVE_RESOURCE_LIMIT_MULTIPLIER = 29,
    ALLOW_CHANGE_DELEGATION = 30,
    WITNESS_127_PAY_PER_BLOCK = 31,
    ALLOW_TVM_SOLIDITY_059 = 32,
    ADAPTIVE_RESOURCE_LIMIT_TARGET_RATIO = 33,
    FORBID_TRANSFER_TO_CONTRACT = 35,
    ALLOW_SHIELDED_TRC20_TRANSACTION = 39,
    ALLOW_PBFT = 40,
    ALLOW_TVM_ISTANBUL = 41,
    ALLOW_MARKET_TRANSACTION = 44,
    MARKET_SELL_FEE = 45,
    MARKET_CANCEL_FEE = 46,
    MAX_FEE_LIMIT = 47,
    ALLOW_TRANSACTION_FEE_POOL = 48,
    ALLOW_BLACKHOLE_OPTIMIZATION = 49,
    ALLOW_NEW_RESOURCE_MODEL = 51,
    ALLOW_TVM_FREEZE = 52,
    ALLOW_ACCOUNT_ASSET_OPTIMIZATION = 53,
    ALLOW_TVM_VOTE = 59,
    ALLOW_TVM_COMPATIBLE_EVM = 60,
    FREE_NET_LIMIT = 61,
    TOTAL_NET_LIMIT = 62,
    ALLOW_TVM_LONDON = 63,
    ALLOW_HIGHER_LIMIT_FOR_MAX_CPU_TIME_OF_ONE_TX = 65,
    ALLOW_ASSET_OPTIMIZATION = 66,
    ALLOW_NEW_REWARD = 67,
    MEMO_FEE = 68,
    ALLOW_DELEGATE_OPTIMIZATION = 69,
    UNFREEZE_DELAY_DAYS = 70,
    ALLOW_OPTIMIZED_RETURN_VALUE_OF_CHAIN_ID = 71,
    ALLOW_DYNAMIC_ENERGY = 72,
    DYNAMIC_ENERGY_THRESHOLD = 73,
    DYNAMIC_ENERGY_INCREASE_FACTOR = 74,
    DYNAMIC_ENERGY_MAX_FACTOR = 75,
    ALLOW_TVM_SHANGHAI = 76,
    ALLOW_CANCEL_ALL_UNFREEZE_V2 = 77,
    MAX_DELEGATE_LOCK_PERIOD = 78,
    ALLOW_OLD_REWARD_OPT = 79,
    ALLOW_ENERGY_ADJUSTMENT = 81,
    MAX_CREATE_ACCOUNT_TX_SIZE = 82,
    ALLOW_TVM_CANCUN = 83,
    ALLOW_STRICT_MATH = 87,
    CONSENSUS_LOGIC_OPTIMIZATION = 88,
    ALLOW_TVM_BLOB = 89,
}

impl ProposalType {
    /// Get enum from code, returns None if not supported
    pub fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::MAINTENANCE_TIME_INTERVAL),
            1 => Some(Self::ACCOUNT_UPGRADE_COST),
            2 => Some(Self::CREATE_ACCOUNT_FEE),
            3 => Some(Self::TRANSACTION_FEE),
            4 => Some(Self::ASSET_ISSUE_FEE),
            5 => Some(Self::WITNESS_PAY_PER_BLOCK),
            6 => Some(Self::WITNESS_STANDBY_ALLOWANCE),
            7 => Some(Self::CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT),
            8 => Some(Self::CREATE_NEW_ACCOUNT_BANDWIDTH_RATE),
            9 => Some(Self::ALLOW_CREATION_OF_CONTRACTS),
            10 => Some(Self::REMOVE_THE_POWER_OF_THE_GR),
            11 => Some(Self::ENERGY_FEE),
            12 => Some(Self::EXCHANGE_CREATE_FEE),
            13 => Some(Self::MAX_CPU_TIME_OF_ONE_TX),
            14 => Some(Self::ALLOW_UPDATE_ACCOUNT_NAME),
            15 => Some(Self::ALLOW_SAME_TOKEN_NAME),
            16 => Some(Self::ALLOW_DELEGATE_RESOURCE),
            17 => Some(Self::TOTAL_ENERGY_LIMIT),
            18 => Some(Self::ALLOW_TVM_TRANSFER_TRC10),
            19 => Some(Self::TOTAL_CURRENT_ENERGY_LIMIT),
            20 => Some(Self::ALLOW_MULTI_SIGN),
            21 => Some(Self::ALLOW_ADAPTIVE_ENERGY),
            22 => Some(Self::UPDATE_ACCOUNT_PERMISSION_FEE),
            23 => Some(Self::MULTI_SIGN_FEE),
            24 => Some(Self::ALLOW_PROTO_FILTER_NUM),
            25 => Some(Self::ALLOW_ACCOUNT_STATE_ROOT),
            26 => Some(Self::ALLOW_TVM_CONSTANTINOPLE),
            29 => Some(Self::ADAPTIVE_RESOURCE_LIMIT_MULTIPLIER),
            30 => Some(Self::ALLOW_CHANGE_DELEGATION),
            31 => Some(Self::WITNESS_127_PAY_PER_BLOCK),
            32 => Some(Self::ALLOW_TVM_SOLIDITY_059),
            33 => Some(Self::ADAPTIVE_RESOURCE_LIMIT_TARGET_RATIO),
            35 => Some(Self::FORBID_TRANSFER_TO_CONTRACT),
            39 => Some(Self::ALLOW_SHIELDED_TRC20_TRANSACTION),
            40 => Some(Self::ALLOW_PBFT),
            41 => Some(Self::ALLOW_TVM_ISTANBUL),
            44 => Some(Self::ALLOW_MARKET_TRANSACTION),
            45 => Some(Self::MARKET_SELL_FEE),
            46 => Some(Self::MARKET_CANCEL_FEE),
            47 => Some(Self::MAX_FEE_LIMIT),
            48 => Some(Self::ALLOW_TRANSACTION_FEE_POOL),
            49 => Some(Self::ALLOW_BLACKHOLE_OPTIMIZATION),
            51 => Some(Self::ALLOW_NEW_RESOURCE_MODEL),
            52 => Some(Self::ALLOW_TVM_FREEZE),
            53 => Some(Self::ALLOW_ACCOUNT_ASSET_OPTIMIZATION),
            59 => Some(Self::ALLOW_TVM_VOTE),
            60 => Some(Self::ALLOW_TVM_COMPATIBLE_EVM),
            61 => Some(Self::FREE_NET_LIMIT),
            62 => Some(Self::TOTAL_NET_LIMIT),
            63 => Some(Self::ALLOW_TVM_LONDON),
            65 => Some(Self::ALLOW_HIGHER_LIMIT_FOR_MAX_CPU_TIME_OF_ONE_TX),
            66 => Some(Self::ALLOW_ASSET_OPTIMIZATION),
            67 => Some(Self::ALLOW_NEW_REWARD),
            68 => Some(Self::MEMO_FEE),
            69 => Some(Self::ALLOW_DELEGATE_OPTIMIZATION),
            70 => Some(Self::UNFREEZE_DELAY_DAYS),
            71 => Some(Self::ALLOW_OPTIMIZED_RETURN_VALUE_OF_CHAIN_ID),
            72 => Some(Self::ALLOW_DYNAMIC_ENERGY),
            73 => Some(Self::DYNAMIC_ENERGY_THRESHOLD),
            74 => Some(Self::DYNAMIC_ENERGY_INCREASE_FACTOR),
            75 => Some(Self::DYNAMIC_ENERGY_MAX_FACTOR),
            76 => Some(Self::ALLOW_TVM_SHANGHAI),
            77 => Some(Self::ALLOW_CANCEL_ALL_UNFREEZE_V2),
            78 => Some(Self::MAX_DELEGATE_LOCK_PERIOD),
            79 => Some(Self::ALLOW_OLD_REWARD_OPT),
            81 => Some(Self::ALLOW_ENERGY_ADJUSTMENT),
            82 => Some(Self::MAX_CREATE_ACCOUNT_TX_SIZE),
            83 => Some(Self::ALLOW_TVM_CANCUN),
            87 => Some(Self::ALLOW_STRICT_MATH),
            88 => Some(Self::CONSENSUS_LOGIC_OPTIMIZATION),
            89 => Some(Self::ALLOW_TVM_BLOB),
            _ => None,
        }
    }
}

/// Validates a proposal parameter against Java's ProposalUtil.validator() rules.
///
/// This function provides 1:1 parity with Java's validation logic, including:
/// - Range checks
/// - Boolean value checks (must be 0, 1, or exactly 1)
/// - Fork gating
/// - Prerequisite checks
/// - "Already active" checks
///
/// # Arguments
/// * `storage_adapter` - Storage adapter for reading dynamic properties
/// * `code` - The proposal parameter code
/// * `value` - The proposed value
///
/// # Returns
/// * `Ok(())` if validation passes
/// * `Err(String)` with the exact error message matching Java
pub fn validate_proposal_parameter(
    storage_adapter: &EngineBackedEvmStateStore,
    code: i64,
    value: i64,
) -> Result<(), String> {
    let proposal_type =
        ProposalType::from_code(code).ok_or_else(|| format!("Does not support code : {}", code))?;

    match proposal_type {
        ProposalType::MAINTENANCE_TIME_INTERVAL => {
            // Range: [3 * 27 * 1000, 24 * 3600 * 1000]
            if value < 3 * 27 * 1000 || value > 24 * 3600 * 1000 {
                return Err(
                    "Bad chain parameter value, valid range is [3 * 27 * 1000,24 * 3600 * 1000]"
                        .to_string(),
                );
            }
        }

        ProposalType::ACCOUNT_UPGRADE_COST
        | ProposalType::CREATE_ACCOUNT_FEE
        | ProposalType::TRANSACTION_FEE
        | ProposalType::ASSET_ISSUE_FEE
        | ProposalType::WITNESS_PAY_PER_BLOCK
        | ProposalType::WITNESS_STANDBY_ALLOWANCE
        | ProposalType::CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT
        | ProposalType::CREATE_NEW_ACCOUNT_BANDWIDTH_RATE => {
            // Range: [0, LONG_VALUE]
            if value < 0 || value > LONG_VALUE {
                return Err(LONG_VALUE_ERROR.to_string());
            }
        }

        ProposalType::ALLOW_CREATION_OF_CONTRACTS => {
            if value != 1 {
                return Err(
                    "This value[ALLOW_CREATION_OF_CONTRACTS] is only allowed to be 1".to_string(),
                );
            }
        }

        ProposalType::REMOVE_THE_POWER_OF_THE_GR => {
            let remove_power = storage_adapter
                .get_remove_the_power_of_the_gr()
                .map_err(|e| format!("Failed to get REMOVE_THE_POWER_OF_THE_GR: {}", e))?;
            if remove_power == -1 {
                return Err(
                    "This proposal has been executed before and is only allowed to be executed once"
                        .to_string(),
                );
            }
            if value != 1 {
                return Err(
                    "This value[REMOVE_THE_POWER_OF_THE_GR] is only allowed to be 1".to_string(),
                );
            }
        }

        ProposalType::ENERGY_FEE | ProposalType::EXCHANGE_CREATE_FEE => {
            // No validation in Java (break with no checks)
        }

        ProposalType::MAX_CPU_TIME_OF_ONE_TX => {
            let allow_higher = storage_adapter
                .get_allow_higher_limit_for_max_cpu_time_of_one_tx()
                .map_err(|e| {
                    format!(
                        "Failed to get ALLOW_HIGHER_LIMIT_FOR_MAX_CPU_TIME_OF_ONE_TX: {}",
                        e
                    )
                })?;
            if allow_higher == 1 {
                if value < 10 || value > 400 {
                    return Err("Bad chain parameter value, valid range is [10,400]".to_string());
                }
            } else if value < 10 || value > 100 {
                return Err("Bad chain parameter value, valid range is [10,100]".to_string());
            }
        }

        ProposalType::ALLOW_UPDATE_ACCOUNT_NAME => {
            if value != 1 {
                return Err(
                    "This value[ALLOW_UPDATE_ACCOUNT_NAME] is only allowed to be 1".to_string(),
                );
            }
        }

        ProposalType::ALLOW_SAME_TOKEN_NAME => {
            if value != 1 {
                return Err("This value[ALLOW_SAME_TOKEN_NAME] is only allowed to be 1".to_string());
            }
        }

        ProposalType::ALLOW_DELEGATE_RESOURCE => {
            if value != 1 {
                return Err(
                    "This value[ALLOW_DELEGATE_RESOURCE] is only allowed to be 1".to_string(),
                );
            }
        }

        ProposalType::TOTAL_ENERGY_LIMIT => {
            // Fork-gated (ENERGY_LIMIT) + deprecated after VERSION_3_2_2
            if !storage_adapter
                .fork_controller_pass(ENERGY_LIMIT)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err(BAD_PARAM_ID.to_string());
            }
            if storage_adapter
                .fork_controller_pass(VERSION_3_2_2)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err(BAD_PARAM_ID.to_string());
            }
            if value < 0 || value > LONG_VALUE {
                return Err(LONG_VALUE_ERROR.to_string());
            }
        }

        ProposalType::ALLOW_TVM_TRANSFER_TRC10 => {
            if value != 1 {
                return Err(
                    "This value[ALLOW_TVM_TRANSFER_TRC10] is only allowed to be 1".to_string(),
                );
            }
            let allow_same_token_name = storage_adapter
                .get_allow_same_token_name()
                .map_err(|e| format!("Failed to get ALLOW_SAME_TOKEN_NAME: {}", e))?;
            if allow_same_token_name == 0 {
                return Err("[ALLOW_SAME_TOKEN_NAME] proposal must be approved before [ALLOW_TVM_TRANSFER_TRC10] can be proposed".to_string());
            }
        }

        ProposalType::TOTAL_CURRENT_ENERGY_LIMIT => {
            if !storage_adapter
                .fork_controller_pass(VERSION_3_2_2)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err(BAD_PARAM_ID.to_string());
            }
            if value < 0 || value > LONG_VALUE {
                return Err(LONG_VALUE_ERROR.to_string());
            }
        }

        ProposalType::ALLOW_MULTI_SIGN => {
            if !storage_adapter
                .fork_controller_pass(VERSION_3_5)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id: ALLOW_MULTI_SIGN".to_string());
            }
            if value != 1 {
                return Err("This value[ALLOW_MULTI_SIGN] is only allowed to be 1".to_string());
            }
        }

        ProposalType::ALLOW_ADAPTIVE_ENERGY => {
            if !storage_adapter
                .fork_controller_pass(VERSION_3_5)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id: ALLOW_ADAPTIVE_ENERGY".to_string());
            }
            if value != 1 {
                return Err("This value[ALLOW_ADAPTIVE_ENERGY] is only allowed to be 1".to_string());
            }
        }

        ProposalType::UPDATE_ACCOUNT_PERMISSION_FEE => {
            if !storage_adapter
                .fork_controller_pass(VERSION_3_5)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id: UPDATE_ACCOUNT_PERMISSION_FEE".to_string());
            }
            if value < 0 || value > MAX_SUPPLY {
                return Err(MAX_SUPPLY_ERROR.to_string());
            }
        }

        ProposalType::MULTI_SIGN_FEE => {
            if !storage_adapter
                .fork_controller_pass(VERSION_3_5)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id: MULTI_SIGN_FEE".to_string());
            }
            if value < 0 || value > MAX_SUPPLY {
                return Err(MAX_SUPPLY_ERROR.to_string());
            }
        }

        ProposalType::ALLOW_PROTO_FILTER_NUM => {
            if !storage_adapter
                .fork_controller_pass(VERSION_3_6)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err(BAD_PARAM_ID.to_string());
            }
            if value != 1 && value != 0 {
                return Err(
                    "This value[ALLOW_PROTO_FILTER_NUM] is only allowed to be 1 or 0".to_string(),
                );
            }
        }

        ProposalType::ALLOW_ACCOUNT_STATE_ROOT => {
            if !storage_adapter
                .fork_controller_pass(VERSION_3_6)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err(BAD_PARAM_ID.to_string());
            }
            if value != 1 && value != 0 {
                return Err(
                    "This value[ALLOW_ACCOUNT_STATE_ROOT] is only allowed to be 1 or 0".to_string(),
                );
            }
        }

        ProposalType::ALLOW_TVM_CONSTANTINOPLE => {
            if !storage_adapter
                .fork_controller_pass(VERSION_3_6)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err(BAD_PARAM_ID.to_string());
            }
            if value != 1 {
                return Err(
                    "This value[ALLOW_TVM_CONSTANTINOPLE] is only allowed to be 1".to_string(),
                );
            }
            let allow_tvm_transfer_trc10 = storage_adapter
                .get_allow_tvm_transfer_trc10()
                .map_err(|e| format!("Failed to get ALLOW_TVM_TRANSFER_TRC10: {}", e))?;
            if allow_tvm_transfer_trc10 == 0 {
                return Err("[ALLOW_TVM_TRANSFER_TRC10] proposal must be approved before [ALLOW_TVM_CONSTANTINOPLE] can be proposed".to_string());
            }
        }

        ProposalType::ALLOW_TVM_SOLIDITY_059 => {
            if !storage_adapter
                .fork_controller_pass(VERSION_3_6_5)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err(BAD_PARAM_ID.to_string());
            }
            if value != 1 {
                return Err(
                    "This value[ALLOW_TVM_SOLIDITY_059] is only allowed to be 1".to_string()
                );
            }
            let allow_creation = storage_adapter
                .get_allow_creation_of_contracts()
                .map_err(|e| format!("Failed to get ALLOW_CREATION_OF_CONTRACTS: {}", e))?;
            if allow_creation == 0 {
                return Err("[ALLOW_CREATION_OF_CONTRACTS] proposal must be approved before [ALLOW_TVM_SOLIDITY_059] can be proposed".to_string());
            }
        }

        ProposalType::ADAPTIVE_RESOURCE_LIMIT_TARGET_RATIO => {
            if !storage_adapter
                .fork_controller_pass(VERSION_3_6_5)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err(BAD_PARAM_ID.to_string());
            }
            if value < 1 || value > 1_000 {
                return Err("Bad chain parameter value, valid range is [1,1_000]".to_string());
            }
        }

        ProposalType::ADAPTIVE_RESOURCE_LIMIT_MULTIPLIER => {
            if !storage_adapter
                .fork_controller_pass(VERSION_3_6_5)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err(BAD_PARAM_ID.to_string());
            }
            if value < 1 || value > 10_000 {
                return Err("Bad chain parameter value, valid range is [1,10_000]".to_string());
            }
        }

        ProposalType::ALLOW_CHANGE_DELEGATION => {
            if !storage_adapter
                .fork_controller_pass(VERSION_3_6_5)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err(BAD_PARAM_ID.to_string());
            }
            if value != 1 && value != 0 {
                return Err(
                    "This value[ALLOW_CHANGE_DELEGATION] is only allowed to be 1 or 0".to_string(),
                );
            }
        }

        ProposalType::WITNESS_127_PAY_PER_BLOCK => {
            if !storage_adapter
                .fork_controller_pass(VERSION_3_6_5)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err(BAD_PARAM_ID.to_string());
            }
            if value < 0 || value > LONG_VALUE {
                return Err(LONG_VALUE_ERROR.to_string());
            }
        }

        ProposalType::FORBID_TRANSFER_TO_CONTRACT => {
            if !storage_adapter
                .fork_controller_pass(VERSION_3_6_6)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err(BAD_PARAM_ID.to_string());
            }
            if value != 1 {
                return Err(
                    "This value[FORBID_TRANSFER_TO_CONTRACT] is only allowed to be 1".to_string(),
                );
            }
            let allow_creation = storage_adapter
                .get_allow_creation_of_contracts()
                .map_err(|e| format!("Failed to get ALLOW_CREATION_OF_CONTRACTS: {}", e))?;
            if allow_creation == 0 {
                return Err("[ALLOW_CREATION_OF_CONTRACTS] proposal must be approved before [FORBID_TRANSFER_TO_CONTRACT] can be proposed".to_string());
            }
        }

        ProposalType::ALLOW_PBFT => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_1)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_PBFT]".to_string());
            }
            if value != 1 {
                return Err("This value[ALLOW_PBFT] is only allowed to be 1".to_string());
            }
        }

        ProposalType::ALLOW_TVM_ISTANBUL => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_1)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_TVM_ISTANBUL]".to_string());
            }
            if value != 1 {
                return Err("This value[ALLOW_TVM_ISTANBUL] is only allowed to be 1".to_string());
            }
        }

        ProposalType::ALLOW_SHIELDED_TRC20_TRANSACTION => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_0_1)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_SHIELDED_TRC20_TRANSACTION]".to_string());
            }
            if value != 1 && value != 0 {
                return Err(
                    "This value[ALLOW_SHIELDED_TRC20_TRANSACTION] is only allowed to be 1 or 0"
                        .to_string(),
                );
            }
        }

        ProposalType::ALLOW_MARKET_TRANSACTION => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_1)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_MARKET_TRANSACTION]".to_string());
            }
            if value != 1 {
                return Err(
                    "This value[ALLOW_MARKET_TRANSACTION] is only allowed to be 1".to_string(),
                );
            }
        }

        ProposalType::MARKET_SELL_FEE => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_1)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [MARKET_SELL_FEE]".to_string());
            }
            if !storage_adapter
                .support_allow_market_transaction()
                .map_err(|e| format!("Failed to check market support: {}", e))?
            {
                return Err(
                    "Market Transaction is not activated, can not set Market Sell Fee".to_string(),
                );
            }
            if value < 0 || value > 10_000_000_000 {
                return Err(
                    "Bad MARKET_SELL_FEE parameter value, valid range is [0,10_000_000_000L]"
                        .to_string(),
                );
            }
        }

        ProposalType::MARKET_CANCEL_FEE => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_1)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [MARKET_CANCEL_FEE]".to_string());
            }
            if !storage_adapter
                .support_allow_market_transaction()
                .map_err(|e| format!("Failed to check market support: {}", e))?
            {
                return Err(
                    "Market Transaction is not activated, can not set Market Cancel Fee"
                        .to_string(),
                );
            }
            if value < 0 || value > 10_000_000_000 {
                return Err(
                    "Bad MARKET_CANCEL_FEE parameter value, valid range is [0,10_000_000_000L]"
                        .to_string(),
                );
            }
        }

        ProposalType::MAX_FEE_LIMIT => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_1_2)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [MAX_FEE_LIMIT]".to_string());
            }
            if value < 0 {
                return Err(
                    "Bad MAX_FEE_LIMIT parameter value, value must not be negative".to_string(),
                );
            } else if value > 10_000_000_000 {
                let allow_tvm_london = storage_adapter
                    .get_allow_tvm_london()
                    .map_err(|e| format!("Failed to get ALLOW_TVM_LONDON: {}", e))?;
                if allow_tvm_london == 0 {
                    return Err(
                        "Bad MAX_FEE_LIMIT parameter value, valid range is [0,10_000_000_000L]"
                            .to_string(),
                    );
                }
                if value > LONG_VALUE {
                    return Err(LONG_VALUE_ERROR.to_string());
                }
            }
        }

        ProposalType::ALLOW_TRANSACTION_FEE_POOL => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_1_2)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_TRANSACTION_FEE_POOL]".to_string());
            }
            if value != 1 && value != 0 {
                return Err(
                    "This value[ALLOW_TRANSACTION_FEE_POOL] is only allowed to be 1 or 0"
                        .to_string(),
                );
            }
        }

        ProposalType::ALLOW_BLACKHOLE_OPTIMIZATION => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_1_2)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                // Note: Java uses ALLOW_REMOVE_BLACKHOLE in error message
                return Err("Bad chain parameter id [ALLOW_REMOVE_BLACKHOLE]".to_string());
            }
            if value != 1 && value != 0 {
                // Note: Java uses ALLOW_REMOVE_BLACKHOLE in error message
                return Err(
                    "This value[ALLOW_REMOVE_BLACKHOLE] is only allowed to be 1 or 0".to_string(),
                );
            }
        }

        ProposalType::ALLOW_NEW_RESOURCE_MODEL => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_2)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_NEW_RESOURCE_MODEL]".to_string());
            }
            if value != 1 {
                return Err(
                    "This value[ALLOW_NEW_RESOURCE_MODEL] is only allowed to be 1".to_string(),
                );
            }
        }

        ProposalType::ALLOW_TVM_FREEZE => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_2)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_TVM_FREEZE]".to_string());
            }
            if value != 1 {
                return Err("This value[ALLOW_TVM_FREEZE] is only allowed to be 1".to_string());
            }
            // Multiple prerequisites
            let allow_delegate = storage_adapter
                .get_allow_delegate_resource()
                .map_err(|e| format!("Failed to get ALLOW_DELEGATE_RESOURCE: {}", e))?;
            if allow_delegate == 0 {
                return Err("[ALLOW_DELEGATE_RESOURCE] proposal must be approved before [ALLOW_TVM_FREEZE] can be proposed".to_string());
            }
            let allow_multi_sign = storage_adapter
                .get_allow_multi_sign()
                .map_err(|e| format!("Failed to get ALLOW_MULTI_SIGN: {}", e))?;
            if !allow_multi_sign {
                return Err("[ALLOW_MULTI_SIGN] proposal must be approved before [ALLOW_TVM_FREEZE] can be proposed".to_string());
            }
            let allow_constantinople = storage_adapter
                .get_allow_tvm_constantinople()
                .map_err(|e| format!("Failed to get ALLOW_TVM_CONSTANTINOPLE: {}", e))?;
            if allow_constantinople == 0 {
                return Err("[ALLOW_TVM_CONSTANTINOPLE] proposal must be approved before [ALLOW_TVM_FREEZE] can be proposed".to_string());
            }
            let allow_solidity059 = storage_adapter
                .get_allow_tvm_solidity059()
                .map_err(|e| format!("Failed to get ALLOW_TVM_SOLIDITY_059: {}", e))?;
            if allow_solidity059 == 0 {
                return Err("[ALLOW_TVM_SOLIDITY_059] proposal must be approved before [ALLOW_TVM_FREEZE] can be proposed".to_string());
            }
        }

        ProposalType::ALLOW_TVM_VOTE => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_3)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_TVM_VOTE]".to_string());
            }
            if value != 1 {
                return Err("This value[ALLOW_TVM_VOTE] is only allowed to be 1".to_string());
            }
            let change_delegation = storage_adapter
                .get_change_delegation()
                .map_err(|e| format!("Failed to get CHANGE_DELEGATION: {}", e))?;
            if change_delegation == 0 {
                return Err("[ALLOW_CHANGE_DELEGATION] proposal must be approved before [ALLOW_TVM_VOTE] can be proposed".to_string());
            }
        }

        ProposalType::FREE_NET_LIMIT => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_3)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [FREE_NET_LIMIT]".to_string());
            }
            if value < 0 || value > 100_000 {
                return Err("Bad chain parameter value, valid range is [0,100_000]".to_string());
            }
        }

        ProposalType::TOTAL_NET_LIMIT => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_3)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [TOTAL_NET_LIMIT]".to_string());
            }
            if value < 0 || value > 1_000_000_000_000 {
                return Err(
                    "Bad chain parameter value, valid range is [0, 1_000_000_000_000L]".to_string(),
                );
            }
        }

        ProposalType::ALLOW_ACCOUNT_ASSET_OPTIMIZATION => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_3)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_ACCOUNT_ASSET_OPTIMIZATION]".to_string());
            }
            if value != 1 {
                return Err(
                    "This value[ALLOW_ACCOUNT_ASSET_OPTIMIZATION] is only allowed to be 1"
                        .to_string(),
                );
            }
        }

        ProposalType::ALLOW_TVM_LONDON => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_4)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_TVM_LONDON]".to_string());
            }
            if value != 1 {
                return Err("This value[ALLOW_TVM_LONDON] is only allowed to be 1".to_string());
            }
        }

        ProposalType::ALLOW_TVM_COMPATIBLE_EVM => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_4)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_TVM_COMPATIBLE_EVM]".to_string());
            }
            if value != 1 {
                return Err(
                    "This value[ALLOW_TVM_COMPATIBLE_EVM] is only allowed to be 1".to_string(),
                );
            }
        }

        ProposalType::ALLOW_HIGHER_LIMIT_FOR_MAX_CPU_TIME_OF_ONE_TX => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_5)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err(
                    "Bad chain parameter id [ALLOW_HIGHER_LIMIT_FOR_MAX_CPU_TIME_OF_ONE_TX]"
                        .to_string(),
                );
            }
            if value != 1 {
                return Err(
                    "This value[ALLOW_HIGHER_LIMIT_FOR_MAX_CPU_TIME_OF_ONE_TX] is only allowed to be 1"
                        .to_string(),
                );
            }
        }

        ProposalType::ALLOW_ASSET_OPTIMIZATION => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_5)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_ASSET_OPTIMIZATION]".to_string());
            }
            if value != 1 {
                return Err(
                    "This value[ALLOW_ASSET_OPTIMIZATION] is only allowed to be 1".to_string(),
                );
            }
        }

        ProposalType::ALLOW_NEW_REWARD => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_6)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_NEW_REWARD]".to_string());
            }
            // "Already active" check
            if storage_adapter
                .allow_new_reward()
                .map_err(|e| format!("Failed to check allow_new_reward: {}", e))?
            {
                return Err("New reward has been valid.".to_string());
            }
            if value != 1 {
                return Err("This value[ALLOW_NEW_REWARD] is only allowed to be 1".to_string());
            }
        }

        ProposalType::MEMO_FEE => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_6)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [MEMO_FEE]".to_string());
            }
            if value < 0 || value > 1_000_000_000 {
                return Err(
                    "This value[MEMO_FEE] is only allowed to be in the range 0-1000_000_000"
                        .to_string(),
                );
            }
        }

        ProposalType::ALLOW_DELEGATE_OPTIMIZATION => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_6)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_DELEGATE_OPTIMIZATION]".to_string());
            }
            if value != 1 {
                return Err(
                    "This value[ALLOW_DELEGATE_OPTIMIZATION] is only allowed to be 1".to_string(),
                );
            }
        }

        ProposalType::UNFREEZE_DELAY_DAYS => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_7)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [UNFREEZE_DELAY_DAYS]".to_string());
            }
            if value < 1 || value > 365 {
                return Err(
                    "This value[UNFREEZE_DELAY_DAYS] is only allowed to be in the range 1-365"
                        .to_string(),
                );
            }
        }

        ProposalType::ALLOW_OPTIMIZED_RETURN_VALUE_OF_CHAIN_ID => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_7)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err(
                    "Bad chain parameter id [ALLOW_OPTIMIZED_RETURN_VALUE_OF_CHAIN_ID]".to_string(),
                );
            }
            if value != 1 {
                return Err(
                    "This value[ALLOW_OPTIMIZED_RETURN_VALUE_OF_CHAIN_ID] is only allowed to be 1"
                        .to_string(),
                );
            }
        }

        ProposalType::ALLOW_DYNAMIC_ENERGY => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_7)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_DYNAMIC_ENERGY]".to_string());
            }
            if value < 0 || value > 1 {
                return Err(
                    "This value[ALLOW_DYNAMIC_ENERGY] is only allowed to be in the range 0-1"
                        .to_string(),
                );
            }
            // Prerequisite when enabling (value == 1)
            if value == 1 {
                let change_delegation = storage_adapter
                    .get_change_delegation()
                    .map_err(|e| format!("Failed to get CHANGE_DELEGATION: {}", e))?;
                if change_delegation == 0 {
                    return Err("[ALLOW_CHANGE_DELEGATION] proposal must be approved before [ALLOW_DYNAMIC_ENERGY] can be opened".to_string());
                }
            }
        }

        ProposalType::DYNAMIC_ENERGY_THRESHOLD => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_7)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [DYNAMIC_ENERGY_THRESHOLD]".to_string());
            }
            if value < 0 || value > LONG_VALUE {
                return Err(LONG_VALUE_ERROR.to_string());
            }
        }

        ProposalType::DYNAMIC_ENERGY_INCREASE_FACTOR => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_7)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [DYNAMIC_ENERGY_INCREASE_FACTOR]".to_string());
            }
            if value < 0 || value > DYNAMIC_ENERGY_INCREASE_FACTOR_RANGE {
                return Err(format!(
                    "This value[DYNAMIC_ENERGY_INCREASE_FACTOR] is only allowed to be in the range 0-{}",
                    DYNAMIC_ENERGY_INCREASE_FACTOR_RANGE
                ));
            }
        }

        ProposalType::DYNAMIC_ENERGY_MAX_FACTOR => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_7)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [DYNAMIC_ENERGY_MAX_FACTOR]".to_string());
            }
            if value < 0 || value > DYNAMIC_ENERGY_MAX_FACTOR_RANGE {
                return Err(format!(
                    "This value[DYNAMIC_ENERGY_MAX_FACTOR] is only allowed to be in the range 0-{}",
                    DYNAMIC_ENERGY_MAX_FACTOR_RANGE
                ));
            }
        }

        ProposalType::ALLOW_TVM_SHANGHAI => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_7_2)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_TVM_SHANGHAI]".to_string());
            }
            if value != 1 {
                return Err("This value[ALLOW_TVM_SHANGHAI] is only allowed to be 1".to_string());
            }
        }

        ProposalType::ALLOW_CANCEL_ALL_UNFREEZE_V2 => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_7_2)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_CANCEL_ALL_UNFREEZE_V2]".to_string());
            }
            if value != 1 {
                return Err(
                    "This value[ALLOW_CANCEL_ALL_UNFREEZE_V2] is only allowed to be 1".to_string(),
                );
            }
            let unfreeze_delay_days = storage_adapter
                .get_unfreeze_delay_days()
                .map_err(|e| format!("Failed to get UNFREEZE_DELAY_DAYS: {}", e))?;
            if unfreeze_delay_days == 0 {
                return Err("[UNFREEZE_DELAY_DAYS] proposal must be approved before [ALLOW_CANCEL_ALL_UNFREEZE_V2] can be proposed".to_string());
            }
        }

        ProposalType::MAX_DELEGATE_LOCK_PERIOD => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_7_2)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [MAX_DELEGATE_LOCK_PERIOD]".to_string());
            }
            let max_delegate_lock_period = storage_adapter
                .get_max_delegate_lock_period()
                .map_err(|e| format!("Failed to get MAX_DELEGATE_LOCK_PERIOD: {}", e))?;
            if value <= max_delegate_lock_period || value > ONE_YEAR_BLOCK_NUMBERS {
                return Err(format!(
                    "This value[MAX_DELEGATE_LOCK_PERIOD] is only allowed to be greater than {} and less than or equal to {} !",
                    max_delegate_lock_period, ONE_YEAR_BLOCK_NUMBERS
                ));
            }
            let unfreeze_delay_days = storage_adapter
                .get_unfreeze_delay_days()
                .map_err(|e| format!("Failed to get UNFREEZE_DELAY_DAYS: {}", e))?;
            if unfreeze_delay_days == 0 {
                return Err("[UNFREEZE_DELAY_DAYS] proposal must be approved before [MAX_DELEGATE_LOCK_PERIOD] can be proposed".to_string());
            }
        }

        ProposalType::ALLOW_OLD_REWARD_OPT => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_7_4)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_OLD_REWARD_OPT]".to_string());
            }
            // "Already active" check
            if storage_adapter
                .allow_old_reward_opt()
                .map_err(|e| format!("Failed to check allow_old_reward_opt: {}", e))?
            {
                return Err(
                    "[ALLOW_OLD_REWARD_OPT] has been valid, no need to propose again".to_string(),
                );
            }
            if value != 1 {
                return Err("This value[ALLOW_OLD_REWARD_OPT] is only allowed to be 1".to_string());
            }
            // Prerequisite: useNewRewardAlgorithm must be true
            if !storage_adapter
                .use_new_reward_algorithm()
                .map_err(|e| format!("Failed to check use_new_reward_algorithm: {}", e))?
            {
                return Err("[ALLOW_NEW_REWARD] or [ALLOW_TVM_VOTE] proposal must be approved before [ALLOW_OLD_REWARD_OPT] can be proposed".to_string());
            }
        }

        ProposalType::ALLOW_ENERGY_ADJUSTMENT => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_7_5)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_ENERGY_ADJUSTMENT]".to_string());
            }
            // "Already active" check
            let allow_energy_adjustment = storage_adapter
                .get_allow_energy_adjustment()
                .map_err(|e| format!("Failed to get ALLOW_ENERGY_ADJUSTMENT: {}", e))?;
            if allow_energy_adjustment == 1 {
                return Err(
                    "[ALLOW_ENERGY_ADJUSTMENT] has been valid, no need to propose again"
                        .to_string(),
                );
            }
            if value != 1 {
                return Err(
                    "This value[ALLOW_ENERGY_ADJUSTMENT] is only allowed to be 1".to_string(),
                );
            }
        }

        ProposalType::MAX_CREATE_ACCOUNT_TX_SIZE => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_7_5)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [MAX_CREATE_ACCOUNT_TX_SIZE]".to_string());
            }
            if value < CREATE_ACCOUNT_TRANSACTION_MIN_BYTE_SIZE
                || value > CREATE_ACCOUNT_TRANSACTION_MAX_BYTE_SIZE
            {
                return Err(format!(
                    "This value[MAX_CREATE_ACCOUNT_TX_SIZE] is only allowed to be greater than or equal to {} and less than or equal to {}!",
                    CREATE_ACCOUNT_TRANSACTION_MIN_BYTE_SIZE, CREATE_ACCOUNT_TRANSACTION_MAX_BYTE_SIZE
                ));
            }
        }

        ProposalType::ALLOW_STRICT_MATH => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_7_7)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_STRICT_MATH]".to_string());
            }
            // "Already active" check
            if storage_adapter
                .allow_strict_math()
                .map_err(|e| format!("Failed to check allow_strict_math: {}", e))?
            {
                return Err(
                    "[ALLOW_STRICT_MATH] has been valid, no need to propose again".to_string(),
                );
            }
            if value != 1 {
                return Err("This value[ALLOW_STRICT_MATH] is only allowed to be 1".to_string());
            }
        }

        ProposalType::CONSENSUS_LOGIC_OPTIMIZATION => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_8_0)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [CONSENSUS_LOGIC_OPTIMIZATION]".to_string());
            }
            // "Already active" check
            let consensus_opt = storage_adapter
                .get_consensus_logic_optimization()
                .map_err(|e| format!("Failed to get CONSENSUS_LOGIC_OPTIMIZATION: {}", e))?;
            if consensus_opt == 1 {
                return Err(
                    "[CONSENSUS_LOGIC_OPTIMIZATION] has been valid, no need to propose again"
                        .to_string(),
                );
            }
            if value != 1 {
                return Err(
                    "This value[CONSENSUS_LOGIC_OPTIMIZATION] is only allowed to be 1".to_string(),
                );
            }
        }

        ProposalType::ALLOW_TVM_CANCUN => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_8_0)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_TVM_CANCUN]".to_string());
            }
            // "Already active" check
            let allow_cancun = storage_adapter
                .get_allow_tvm_cancun()
                .map_err(|e| format!("Failed to get ALLOW_TVM_CANCUN: {}", e))?;
            if allow_cancun == 1 {
                return Err(
                    "[ALLOW_TVM_CANCUN] has been valid, no need to propose again".to_string(),
                );
            }
            if value != 1 {
                return Err("This value[ALLOW_TVM_CANCUN] is only allowed to be 1".to_string());
            }
        }

        ProposalType::ALLOW_TVM_BLOB => {
            if !storage_adapter
                .fork_controller_pass(VERSION_4_8_0)
                .map_err(|e| format!("Failed fork check: {}", e))?
            {
                return Err("Bad chain parameter id [ALLOW_TVM_BLOB]".to_string());
            }
            // "Already active" check
            let allow_blob = storage_adapter
                .get_allow_tvm_blob()
                .map_err(|e| format!("Failed to get ALLOW_TVM_BLOB: {}", e))?;
            if allow_blob == 1 {
                return Err("[ALLOW_TVM_BLOB] has been valid, no need to propose again".to_string());
            }
            if value != 1 {
                return Err("This value[ALLOW_TVM_BLOB] is only allowed to be 1".to_string());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proposal_type_from_code() {
        assert_eq!(
            ProposalType::from_code(0),
            Some(ProposalType::MAINTENANCE_TIME_INTERVAL)
        );
        assert_eq!(
            ProposalType::from_code(9),
            Some(ProposalType::ALLOW_CREATION_OF_CONTRACTS)
        );
        assert_eq!(
            ProposalType::from_code(89),
            Some(ProposalType::ALLOW_TVM_BLOB)
        );
        assert_eq!(ProposalType::from_code(999), None);
        // Codes 27, 28, 34 are not in the enum (commented out in Java)
        assert_eq!(ProposalType::from_code(27), None);
        assert_eq!(ProposalType::from_code(28), None);
        assert_eq!(ProposalType::from_code(34), None);
    }

    #[test]
    fn test_constants() {
        assert_eq!(LONG_VALUE, 100_000_000_000_000_000);
        assert_eq!(MAX_SUPPLY, 100_000_000_000);
        assert_eq!(ONE_YEAR_BLOCK_NUMBERS, 10_512_000);
    }
}
