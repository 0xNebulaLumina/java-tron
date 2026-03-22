Think harder.                                                                                            
                                                                                                                                                                                                                  
I want to compare the (embedded execution + embedded storage) results vs the (remote execution + remote storage) results,                                            
                                                                                                                                                                                                                  
The result csv are                                                                                                                                                                                                
+ output-directory/execution-csv/20260128-131248-9835b834-embedded-embedded.csv                          
+ output-directory/execution-csv/20260319-122200-ef7cfbce-remote-remote.csv                            
respectively.                                                                                            
                                                                                                         
First mismatch at row 9887 (1-based), block 120399, tx e8a5bc5d90155b978ab4ca54a03faedfd66577c279e7ffe11be0856292d1d8e0                                                                                           
  - state_changes_json:                                                                                  
    embedded: [{"address":"414a193c92cd631c1911b99ca964da8fd342f4cddd","key":"","oldValue":"00000000000000000000000000000000000000000000000000000000006ae6080000000000000000c5d2460186f7233c927e7db2dcc703c0e500b6
53ca82273b7bfad8045d85a470000000004145585400010044000000000000162a00000000000001920000000000000000000000001e6744b6000000001e657c1300000000000000000000000000007080000000000000708000000000","newValue":"0000000000
000000000000000000000000000000000000000000000e9ccd3ea90000000000000000c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470000000004145585400010044000000000000162a000000000000019200000000000000000000
00001e6744b6000000001e657c1300000000000000000000000000007080000000000000708000000000"}]         
    remote  : [{"address":"414a193c92cd631c1911b99ca964da8fd342f4cddd","key":"","oldValue":"00000000000000000000000000000000000000000000000000000000006ae6080000000000000000c5d2460186f7233c927e7db2dcc703c0e500b6
53ca82273b7bfad8045d85a470000000004145585400010044000000000000162a00000000000001920000000000000000000000001e6744b6000000001e657c1300000000000000000000000000007080000000000000708000000000","newValue":"0000000000
000000000000000000000000000000000000000000000e9ae4f6a90000000000000000c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470000000004145585400010044000000000000162a000000000000019200000000000000000000
00001e6744b6000000001e657c1300000000000000000000000000007080000000000000708000000000"}]    
  - state_digest_sha256:                                                                                                                                                                                          
    embedded: 16cdd84625dc74150379959759683fc7257124fd9ad41db3253b5baf5a1f89e6                                                                                                                                    
    remote  : e973f4c4286aa0c7ca4b1d97d0a89fd97aa46caa72cd8f3275247ed91b9cb7af                                                                                                                                    
  - account_changes_json:                                                                                
    embedded: [{"address_hex":"414a193c92cd631c1911b99ca964da8fd342f4cddd","balance_sun":{"new":"62760238761","old":"7005704"},"code_hash_hex":{"new":"c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d8
5a470","old":"c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"},"code_len_bytes":{"new":"0","old":"0"},"nonce":{"new":"0","old":"0"},"op":"update"}]
    remote  : [{"address_hex":"414a193c92cd631c1911b99ca964da8fd342f4cddd","balance_sun":{"new":"62728238761","old":"7005704"},"code_hash_hex":{"new":"c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d8
5a470","old":"c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"},"code_len_bytes":{"new":"0","old":"0"},"nonce":{"new":"0","old":"0"},"op":"update"}]
  - account_digest_sha256:                                                                               
    embedded: 58b8cd49a30b608ed671fd76210757e808c85a7ef0db40cb7fe09bcb027130d8                         
    remote  : eed51601152bfe9f51f3cc8b898a3ace79a9f1b7e23ad44c12207cd239cac15f
                                                                                                         
Logs:                                                                                                                                                                                                             
(embedded execution + embedded storage) java log: 621c89c.embedded-java.log
(remote execution + remote storage) java log: remote-java.04e59b7.log                    
(remote execution + remote storage) rust log: remote-rust.04e59b7.log                      
                                                                                                                                                                                                                  
You will help me debug and figure out why there are mismatches.

---

• Current Read

- The first real divergence is a stale Account.allowance for TGj1Ej1qRzL9feLTLhjwgxXF4Ct6GTWg2U, not a bad withdraw implementation. At block 120399, Rust withdrew 62721233057, embedded withdrew 62753233057,
and the gap is exactly 32000000 SUN.
- That 32000000 is WITNESS_PAY_PER_BLOCK from framework/src/main/java/org/tron/core/db/Manager.java:2005. In no-delegation mode, Java adds witness rewards directly to Account.allowance there, but that path
does not eagerly mirror the updated account to remote storage.
- Rust is behaving consistently with the state it saw. The rust log for tx e8a5bc... shows it loaded balance 7005704 and withdrew exactly the stale allowance it had, so the bug is upstream of contract
execution.

What I ruled out

- The duplicate ResourceSyncContext classes are not the runtime cause. FullNode.jar contains two entries, but javap against the built jar resolves the framework implementation, so chainbase
recordAccountDirty(...) calls are not dead.
- ResourceSyncService should be live in this run: Spring scans org.tron, storage mode comes from main_net_config_remote.conf, and pre-exec sync is called in framework/src/main/java/org/tron/core/db/
Manager.java:1554.
- buildAccountBatch() is not obviously reading the wrong layer. Its accountStore.getUnchecked(...) still goes through the revoking head, not directly to remote root.

Most Likely Root Cause

- The mismatch comes from Java reward state reaching only the local revoking head, while Rust execution reads the lagging remote store.
- The intended repair path is pre-exec resource sync before trace.exec(), but at block 120399 that repair did not make TGj1...’s updated account visible to Rust.
- The strongest concrete bug in that repair path is framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java:208: batchWrite() ignores BatchWriteResponse.success. If Rust returned
success=false, Java would proceed as if the sync had succeeded.
- A second design weakness is that flushPreExec() is non-fatal and quiet, so a skipped/ineffective sync can produce exactly this kind of deterministic parity bug.

The downstream TransferContract revert at block 122719 is just fallout from the same missing 32000000.

Next useful step is targeted instrumentation, not contract changes: log whether TGj1... is present in the pre-exec dirty account batch, log the serialized allowance Java is sending, and make batchWrite() fail
hard on success=false. I can implement that next.

---

• Implemented the missing reward-sync path for delegation mode.

The mismatch at block 120399 / tx e8a5bc5d90155b978ab4ca54a03faedfd66577c279e7ffe11be0856292d1d8e0 is consistent with Java paying post-block witness rewards into DelegationStore while Rust withdraw computes
reward from the remote delegation DB. The earlier fix only mirrored the legacy direct-allowance branch, so Rust kept missing the 32,000,000 SUN reward.

Changes are in framework/src/main/java/org/tron/core/db/Manager.java:2008 and framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java:147:

- Manager.payReward(...) now collects post-block dirty witness accounts plus current-cycle delegation reward keys (<cycle>-<witnessHex>-reward) for the block witness, standby witnesses, and tx-fee reward,
  then flushes them immediately.
- ResourceSyncService now supports flushing delegation DB keys, includes them in debug logging/confirmation reads, and uses the same fail-closed behavior as the existing account/property sync.

Verification:

- ./gradlew :framework:compileJava -x test --dependency-verification=off passed.
- ./gradlew :framework:checkstyleMain --dependency-verification=off passed with the repo’s existing warning-heavy baseline.
- I did not rerun the full parity job.

Next step is to rerun the remote scenario with this build. If the diagnosis is right, the first mismatch at row 9887 should disappear. If it does not, the next place to instrument is Rust-side delegation
batch-write readback, not WithdrawBalanceContract itself.