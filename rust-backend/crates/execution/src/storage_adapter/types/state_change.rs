use revm::primitives::{Address, AccountInfo, U256};

/// Represents different types of state changes with old and new values
#[derive(Debug, Clone)]
pub enum StateChangeRecord {
    /// Storage slot change within a contract
    StorageChange {
        address: Address,
        key: U256,
        old_value: U256,
        new_value: U256,
    },
    /// Account-level change (balance, nonce, code, etc.)
    AccountChange {
        address: Address,
        old_account: Option<AccountInfo>,
        new_account: Option<AccountInfo>,
    },
}
