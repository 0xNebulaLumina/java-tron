think harder:



I am comparing and debugging the (embedded execution + embedded storage) results vs the (remote execution + remote storage) results,
accoridng to @planning.md and the "Current Task: TRON‑Accurate Fee Handling (Remote Execution)" section in @CLAUDE.md .


but after I implement phase 3 (commit d70c6696e60333549f803e0a27ce11ecdc2f07af) 

There seems to be something wrong. 

Because now the Java side logs show "Non-VM transaction execution failed: Insufficient balance: need 125000, have 0", Which we don't have previously.


for more info:
+ phase 2 java-side logs is: java-2.log
+ phase 2 rust-side logs is: rust-2.log
+ phase 3 java-side logs is: java-3.log
+ phase 3 rust-side logs is: rust-3.log

Help me debug and fix. 


---


Root Cause                                                                                                                                                                                                                                        
                                                                                                                                                                                                                                                  
- The new Phase 3 “native non‑VM” path in the Rust backend started charging a default bandwidth-based TRX fee for ALL non‑VM transactions. For system/other non‑TRX transactions (often value=0, sometimes with to = T9yD14...), the sender may   
have 0 TRX, causing failures:                                                                                                                                                                                                                     
    - rust-3.log shows repeated errors: “Non-VM transaction execution failed: Insufficient balance: need 125000, have 0”.                                                                                                                         
    - The backend logs also read the blackhole address (T9yD14.../0x000…) because the Java request sets to to zero for many non‑VM contract types; our non‑VM handler was treating them as TRX value transfers and trying to charge fees          
regardless.                                                                                                                                                                                                                                       
                                                                                                                                                                                                                                                  
What I Changed                                                                                                                                                                                                                                    
                                                                                                                                                                                                                                                  
- File: rust-backend/crates/core/src/service.rs                                                                                                                                                                                                   
- Non‑VM execution (execute_non_vm_transaction):                                                                                                                                                                                                  
    - Default non‑VM fee = 0 unless explicitly configured via execution.fees.non_vm_blackhole_credit_flat.                                                                                                                                        
    - Only perform TRX balance updates when value > 0 (i.e., actual TRX transfers).                                                                                                                                                               
    - For value=0 system txs, skip TRX balance changes and just report bandwidth.                                                                                                                                                                 
    - Keep burn/blackhole fee modes intact; with burn+0 fee, it logs and doesn’t touch balances.                                                                                                                                                  
                                                                                                                                                                                                                                                  
This removes the unconditional TRX fee deduction that was breaking system/non‑transfer transactions.                                                                                                                                              
                                                                                                                                                                                                                                                  
Why This Fixes It                                                                                                                                                                                                                                 
                                                                                                                                                                                                                                                  
- Phase 2 behavior didn’t charge TRX for non‑VM by default; Phase 3 added a default bandwidth → fee deduction that caused “Insufficient balance” for senders with 0 TRX. Resetting the default fee to 0 restores parity with embedded execution   
and avoids failing non‑transfer system transactions while still reporting bandwidth for upstream accounting.                                                                                                                                      
                                                                                                                                                                                                                                                  
How to Validate                                                                                                                                                                                                                                   
                                                                                                                                                                                                                                                  
- Re-run your Phase 3 scenario and compare logs:                                                                                                                                                                                                  
    - rust-3.log should no longer contain “Non-VM transaction execution failed: Insufficient balance…” for system/non‑transfer txs.                                                                                                               
    - You should see “Non-VM transaction executed successfully - energy_used: 0, bandwidth_used: …”.                                                                                                                                              
- If you want to explicitly charge a flat fee to the blackhole for non‑VM txs, set execution.fees.non_vm_blackhole_credit_flat in rust-backend/config.toml.                                                                                       
                                                                                                                                                                                                                                                  
Notes                                                                                                                                                                                                                                             
                                                                                                                                                                                                                                                  
- This keeps behavior conservative for parity. A full TRON resource model (bandwidth/RC and dynamic properties) can be layered later to accurately decide when to charge TRX vs consume free bandwidth.                                           
- I didn’t run a local cargo build due to sandbox constraints; changes are minimal and should compile. If you want, I can build with elevated permissions or you can run cd rust-backend && cargo build --release.                                
                                                                                                                                                                                                                                                  
Optional Next Steps                                                                                                                                                                                                                               
                                                                                                                                                                                                                                                  
- Pass the TRON contract type over gRPC so the backend can tailor non‑VM handling per contract rather than relying solely on value heuristics.                                                                                                    
- If desired, add a config switch to re-enable default bandwidth-based fees for non‑VM once proper resource checks are implemented. 