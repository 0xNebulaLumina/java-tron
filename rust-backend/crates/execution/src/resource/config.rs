//! Resource configuration for TRON bandwidth and fee management

use anyhow::Result;
use tron_backend_common::ExecutionConfig;

/// Configuration for resource management
#[derive(Debug, Clone)]
pub struct ResourceConfig {
    /// Fee handling mode: "burn", "blackhole", or "none"
    pub fee_mode: String,
    
    /// Base58-encoded blackhole address (if fee_mode = "blackhole")
    pub blackhole_address_base58: String,
    
    /// Whether black hole optimization is supported
    pub support_black_hole_optimization: bool,
    
    /// Whether to read dynamic properties from storage
    pub use_dynamic_properties: bool,
    
    /// Fallback flat fee for non-VM transactions (when not using dynamic properties)
    pub non_vm_flat_fee: Option<u64>,
    
    /// Enable experimental VM fee handling
    pub experimental_vm_fees: bool,
}

impl ResourceConfig {
    pub fn from_execution_config(config: &ExecutionConfig) -> Result<Self> {
        Ok(Self {
            fee_mode: config.fees.mode.clone(),
            blackhole_address_base58: config.fees.blackhole_address_base58.clone(),
            support_black_hole_optimization: config.fees.support_black_hole_optimization,
            use_dynamic_properties: config.fees.use_dynamic_properties,
            non_vm_flat_fee: config.fees.non_vm_blackhole_credit_flat,
            experimental_vm_fees: config.fees.experimental_vm_blackhole_credit,
        })
    }

    /// Validate the resource configuration
    pub fn validate(&self) -> Result<()> {
        if self.fee_mode == "blackhole" && self.blackhole_address_base58.is_empty() {
            return Err(anyhow::anyhow!(
                "Blackhole address required when fee_mode = 'blackhole'"
            ));
        }

        if !matches!(self.fee_mode.as_str(), "burn" | "blackhole" | "none") {
            return Err(anyhow::anyhow!(
                "Invalid fee_mode: '{}'. Must be 'burn', 'blackhole', or 'none'",
                self.fee_mode
            ));
        }

        Ok(())
    }

    /// Check if fees should be applied to blackhole account
    pub fn should_credit_blackhole(&self) -> bool {
        self.fee_mode == "blackhole"
    }

    /// Check if fees should be burned (no state delta)
    pub fn should_burn_fees(&self) -> bool {
        self.fee_mode == "burn"
    }

    /// Check if fee processing is disabled
    pub fn fees_disabled(&self) -> bool {
        self.fee_mode == "none"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tron_backend_common::ExecutionFeeConfig;

    #[test]
    fn test_config_from_execution_config() {
        let exec_config = ExecutionConfig {
            fees: ExecutionFeeConfig {
                mode: "blackhole".to_string(),
                blackhole_address_base58: "TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy".to_string(),
                use_dynamic_properties: true,
                ..ExecutionFeeConfig::default()
            },
            ..ExecutionConfig::default()
        };

        let resource_config = ResourceConfig::from_execution_config(&exec_config).unwrap();
        assert_eq!(resource_config.fee_mode, "blackhole");
        assert!(!resource_config.blackhole_address_base58.is_empty());
        assert!(resource_config.use_dynamic_properties);
    }

    #[test]
    fn test_config_validation() {
        // Valid burn mode
        let config = ResourceConfig {
            fee_mode: "burn".to_string(),
            blackhole_address_base58: String::new(),
            support_black_hole_optimization: true,
            use_dynamic_properties: false,
            non_vm_flat_fee: None,
            experimental_vm_fees: false,
        };
        assert!(config.validate().is_ok());

        // Invalid blackhole mode (missing address)
        let config = ResourceConfig {
            fee_mode: "blackhole".to_string(),
            blackhole_address_base58: String::new(),
            ..config
        };
        assert!(config.validate().is_err());

        // Valid blackhole mode
        let config = ResourceConfig {
            blackhole_address_base58: "TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy".to_string(),
            ..config
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_fee_mode_checks() {
        let burn_config = ResourceConfig {
            fee_mode: "burn".to_string(),
            blackhole_address_base58: String::new(),
            support_black_hole_optimization: true,
            use_dynamic_properties: false,
            non_vm_flat_fee: None,
            experimental_vm_fees: false,
        };
        assert!(burn_config.should_burn_fees());
        assert!(!burn_config.should_credit_blackhole());
        assert!(!burn_config.fees_disabled());

        let blackhole_config = ResourceConfig {
            fee_mode: "blackhole".to_string(),
            ..burn_config
        };
        assert!(!blackhole_config.should_burn_fees());
        assert!(blackhole_config.should_credit_blackhole());
        assert!(!blackhole_config.fees_disabled());

        let none_config = ResourceConfig {
            fee_mode: "none".to_string(),
            ..burn_config
        };
        assert!(!none_config.should_burn_fees());
        assert!(!none_config.should_credit_blackhole());
        assert!(none_config.fees_disabled());
    }
}