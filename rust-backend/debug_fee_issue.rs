use std::collections::HashMap;
use revm::primitives::{Address, U256, AccountInfo, Bytecode};
use tron_backend_execution::{StorageModuleAdapter, TronTransaction, TronExecutionContext};
use tron_backend_storage::StorageEngine;

fn main() -> anyhow::Result<()> {
    println!("=== Debugging LackOfFundForMaxFee Issue ===");
    
    // Initialize storage engine
    let storage_engine = StorageEngine::new("./data")?;
    let storage_adapter = StorageModuleAdapter::new(storage_engine);
    
    // Test account address (example)
    let test_address = Address::from([0x41, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56]);
    
    // Check account balance
    println!("1. Testing account balance retrieval:");
    match storage_adapter.get_account(&test_address) {
        Ok(Some(account)) => {
            println!("   Account found - balance: {}, nonce: {}", account.balance, account.nonce);
        }
        Ok(None) => {
            println!("   Account not found in storage");
        }
        Err(e) => {
            println!("   Error retrieving account: {}", e);
        }
    }
    
    // Test different gas price scenarios
    println!("\n2. Testing fee calculations:");
    
    // Scenario 1: High energy_price (typical Tron value in SUN)
    let high_energy_price = 420_000_000u64; // 420 SUN per energy unit
    let gas_limit = 100_000u64;
    let max_fee_high = U256::from(high_energy_price) * U256::from(gas_limit);
    println!("   High energy_price scenario:");
    println!("     energy_price: {} SUN", high_energy_price);
    println!("     gas_limit: {}", gas_limit);
    println!("     max_fee: {} ({})", max_fee_high, max_fee_high);
    
    // Scenario 2: Reasonable gas_price (converted)
    let reasonable_gas_price = 1_000u64; // 1000 wei equivalent
    let max_fee_reasonable = U256::from(reasonable_gas_price) * U256::from(gas_limit);
    println!("   Reasonable gas_price scenario:");
    println!("     gas_price: {} wei", reasonable_gas_price);
    println!("     gas_limit: {}", gas_limit);
    println!("     max_fee: {} ({})", max_fee_reasonable, max_fee_reasonable);
    
    // Test transaction creation with different parameters
    println!("\n3. Testing transaction creation:");
    
    let tx_high_price = TronTransaction {
        from: test_address,
        to: Some(Address::from([0x41, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd])),
        value: U256::from(1000000), // 1 TRX in SUN
        data: revm::primitives::Bytes::new(),
        gas_limit: 100_000,
        gas_price: U256::from(high_energy_price), // This is the problem!
        nonce: 1,
    };
    
    println!("   Transaction with high gas_price:");
    println!("     gas_limit: {}", tx_high_price.gas_limit);
    println!("     gas_price: {}", tx_high_price.gas_price);
    println!("     max_fee: {}", tx_high_price.gas_price * U256::from(tx_high_price.gas_limit));
    
    // Check if we can find any accounts with balances
    println!("\n4. Scanning for accounts with balances:");
    // This would require iterating through the database, which is complex
    // For now, let's just test a few known patterns
    
    let test_addresses = vec![
        Address::ZERO,
        Address::from([0x41; 20]), // Tron genesis pattern
        Address::from([0x00; 20]), // Zero address
    ];
    
    for addr in test_addresses {
        match storage_adapter.get_account(&addr) {
            Ok(Some(account)) => {
                println!("   Address {:?} - balance: {}, nonce: {}", addr, account.balance, account.nonce);
            }
            Ok(None) => {
                println!("   Address {:?} - not found", addr);
            }
            Err(e) => {
                println!("   Address {:?} - error: {}", addr, e);
            }
        }
    }
    
    println!("\n=== Analysis ===");
    println!("The issue is likely that:");
    println!("1. energy_price from Java (in SUN) is used directly as gas_price in REVM");
    println!("2. This creates extremely high max_fee values (gas_limit * gas_price)");
    println!("3. Even accounts with reasonable TRX balances appear insufficient");
    println!("4. Need to convert energy_price to appropriate gas_price units");
    
    Ok(())
}
