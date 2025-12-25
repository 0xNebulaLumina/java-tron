//! Database name constants matching Java's store dbName values.
//!
//! ## Phase 0.2 Implementation
//!
//! This module provides centralized database name constants that match
//! Java's TronStoreWithRevoking store classes exactly. Case sensitivity
//! is critical - these values are used as RocksDB column family names.
//!
//! ## Naming Conventions (from Java)
//!
//! - Kebab-case: `account-index`, `asset-issue`, `exchange-v2`
//! - Underscore-case: `market_account`, `market_order`
//! - CamelCase: `DelegatedResource`, `DelegatedResourceAccountIndex`
//! - Simple lowercase: `delegation`, `proposal`, `contract`, `abi`
//!
//! ## References
//!
//! Java stores: `chainbase/src/main/java/org/tron/core/store/*Store.java`

/// Account-related database names
pub mod account {
    /// Main account store (AccountStore)
    /// Java: chainbase/src/main/java/org/tron/core/store/AccountStore.java
    pub const ACCOUNT: &str = "account";

    /// Account index by name (AccountIndexStore)
    /// Java: AccountIndexStore.java - dbName = "account-index"
    /// Note: Rust previously used "account-name" which was INCORRECT
    pub const ACCOUNT_INDEX: &str = "account-index";

    /// Account index by ID (AccountIdIndexStore)
    /// Java: AccountIdIndexStore.java - dbName = "accountid-index"
    pub const ACCOUNT_ID_INDEX: &str = "accountid-index";

    /// Account resource usage tracking (AEXT)
    /// Custom Rust-side tracking, may not exist in Java
    pub const ACCOUNT_RESOURCE: &str = "account-resource";
}

/// Delegation-related database names
pub mod delegation {
    /// Delegated resources (DelegatedResourceStore)
    /// Java: DelegatedResourceStore.java - dbName = "DelegatedResource"
    /// WARNING: CamelCase, not lowercase!
    pub const DELEGATED_RESOURCE: &str = "DelegatedResource";

    /// Delegated resource account index (DelegatedResourceAccountIndexStore)
    /// Java: DelegatedResourceAccountIndexStore.java - dbName = "DelegatedResourceAccountIndex"
    /// WARNING: CamelCase, not lowercase!
    pub const DELEGATED_RESOURCE_ACCOUNT_INDEX: &str = "DelegatedResourceAccountIndex";

    /// Delegation brokerage store (DelegationStore)
    /// Java: DelegationStore.java - dbName = "delegation"
    pub const DELEGATION: &str = "delegation";
}

/// Asset/TRC-10 database names
pub mod asset {
    /// TRC-10 asset issue store (AssetIssueStore)
    /// Java: AssetIssueStore.java - dbName = "asset-issue"
    pub const ASSET_ISSUE: &str = "asset-issue";

    /// TRC-10 asset issue V2 store (AssetIssueV2Store)
    /// Java: AssetIssueV2Store.java - dbName = "asset-issue-v2"
    pub const ASSET_ISSUE_V2: &str = "asset-issue-v2";
}

/// Contract/TVM database names
pub mod contract {
    /// Smart contract metadata store (ContractStore)
    /// Java: ContractStore.java - dbName = "contract"
    pub const CONTRACT: &str = "contract";

    /// Contract ABI store (AbiStore)
    /// Java: AbiStore.java - dbName = "abi"
    pub const ABI: &str = "abi";

    /// Contract bytecode store (CodeStore)
    /// Java: CodeStore.java - dbName = "code"
    pub const CODE: &str = "code";

    /// Contract storage state (ContractStateStore)
    /// Java: ContractStateStore.java - dbName = "contract-state"
    pub const CONTRACT_STATE: &str = "contract-state";
}

/// EVM contract storage rows
pub mod storage {
    /// Contract storage row store (StorageRowStore)
    /// Java: StorageRowStore.java - dbName = "storage-row"
    pub const STORAGE_ROW: &str = "storage-row";
}

/// Governance database names
pub mod governance {
    /// Proposal store (ProposalStore)
    /// Java: ProposalStore.java - dbName = "proposal"
    pub const PROPOSAL: &str = "proposal";

    /// Witness store (WitnessStore)
    /// Java: WitnessStore.java - dbName = "witness"
    pub const WITNESS: &str = "witness";

    /// Votes store (VotesStore)
    /// Java: VotesStore.java - dbName = "votes"
    pub const VOTES: &str = "votes";
}

/// Exchange database names
pub mod exchange {
    /// Exchange V1 store (ExchangeStore)
    /// Java: ExchangeStore.java - dbName = "exchange"
    pub const EXCHANGE: &str = "exchange";

    /// Exchange V2 store (ExchangeV2Store)
    /// Java: ExchangeV2Store.java - dbName = "exchange-v2"
    pub const EXCHANGE_V2: &str = "exchange-v2";
}

/// Market (DEX) database names
pub mod market {
    /// Market account store (MarketAccountStore)
    /// Java: MarketAccountStore.java - dbName = "market_account"
    pub const MARKET_ACCOUNT: &str = "market_account";

    /// Market order store (MarketOrderStore)
    /// Java: MarketOrderStore.java - dbName = "market_order"
    pub const MARKET_ORDER: &str = "market_order";

    /// Market pair to price store (MarketPairToPriceStore)
    /// Java: MarketPairToPriceStore.java - dbName = "market_pair_to_price"
    pub const MARKET_PAIR_TO_PRICE: &str = "market_pair_to_price";

    /// Market pair price to order store (MarketPairPriceToOrderStore)
    /// Java: MarketPairPriceToOrderStore.java - dbName = "market_pair_price_to_order"
    pub const MARKET_PAIR_PRICE_TO_ORDER: &str = "market_pair_price_to_order";
}

/// Resource freeze/stake database names
pub mod freeze {
    /// Freeze records (custom Rust tracking)
    /// Maps to TRON's freeze/unfreeze V2 ledger tracking
    pub const FREEZE_RECORDS: &str = "freeze-records";
}

/// System/Dynamic properties database names
pub mod system {
    /// Dynamic properties store (DynamicPropertiesStore)
    /// Java: DynamicPropertiesStore.java - dbName = "properties"
    pub const PROPERTIES: &str = "properties";

    /// Recent blocks store (RecentBlockStore)
    /// Java: RecentBlockStore.java - dbName = "recent-block"
    pub const RECENT_BLOCK: &str = "recent-block";

    /// Block index store (BlockIndexStore)
    /// Java: BlockIndexStore.java - dbName = "block-index"
    pub const BLOCK_INDEX: &str = "block-index";

    /// Block store (BlockStore)
    /// Java: BlockStore.java - dbName = "block"
    pub const BLOCK: &str = "block";

    /// Transaction store (TransactionStore)
    /// Java: TransactionStore.java - dbName = "trans"
    pub const TRANSACTION: &str = "trans";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camel_case_names_are_correct() {
        // These are critical - case must match exactly
        assert_eq!(delegation::DELEGATED_RESOURCE, "DelegatedResource");
        assert_eq!(delegation::DELEGATED_RESOURCE_ACCOUNT_INDEX, "DelegatedResourceAccountIndex");
    }

    #[test]
    fn test_underscore_names_are_correct() {
        // Market stores use underscore convention
        assert_eq!(market::MARKET_ACCOUNT, "market_account");
        assert_eq!(market::MARKET_ORDER, "market_order");
        assert_eq!(market::MARKET_PAIR_TO_PRICE, "market_pair_to_price");
        assert_eq!(market::MARKET_PAIR_PRICE_TO_ORDER, "market_pair_price_to_order");
    }

    #[test]
    fn test_kebab_case_names_are_correct() {
        assert_eq!(account::ACCOUNT_INDEX, "account-index");
        assert_eq!(account::ACCOUNT_ID_INDEX, "accountid-index");
        assert_eq!(asset::ASSET_ISSUE, "asset-issue");
        assert_eq!(asset::ASSET_ISSUE_V2, "asset-issue-v2");
        assert_eq!(exchange::EXCHANGE_V2, "exchange-v2");
    }
}
