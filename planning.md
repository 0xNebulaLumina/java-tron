think harder:



I am comparing and debugging the (embedded execution + embedded storage) results vs the (remote execution + remote storage) results,
accoridng to planning.md and the "Current Task: TRON‑Accurate Fee Handling (Remote Execution)" section in CLAUDE.md .


but is the phase 3 implementation (commit d70c6696e60333549f803e0a27ce11ecdc2f07af) really correct? or is there any other reason?


why I ask is because:


the embbed result is output-directory/execution-csv/20250830-121317-cfc1a8c1-embedded-embedded.csv,

phase 1 and phase 2 remote result is output-directory/execution-csv/20250901-063135-9f694cc7-remote-remote.csv

but phase 3 remote result (output-directory/execution-csv/20250901-064114-89131436-remote-remote.csv) get stuck at some tx at block_num 2040


for more info:
+ phase 2 java-side logs is: java-2.log
+ phase 2 rust-side logs is: rust-2.log
+ phase 3 java-side logs is: java-3.log
+ phase 3 rust-side logs is: rust-3.log


---



Here’s what I found by correlating the CSVs, your phase 2/3 logs, and the Phase 3 diff.

What happened

- Stoppage point: The Phase 3 CSV stops in block 2040 after tx index 3. Highest recorded entries are block
2040, tx_index 0–3, then no 4th row.
- Rust errors: In rust-3.log there are hard failures before that window: “Non-VM transaction execution
failed: Insufficient balance: need 125000, have 0” (06:42:33). That’s consistent with enforcing a TRX fee
for a non-VM tx where the sender had 0 TRX but still had free bandwidth on TRON.
- Phase 2 vs 3 contrast: Phase 2 had no such “Insufficient balance” errors, and the CSV progressed (1284
rows). Phase 3 stopped early (241 rows).

Why this happens (Phase 3 deviation)

- Always-charging non‑VM fees: In rust-backend/crates/core/src/service.rs, the new non‑VM path computes
fee_amount = bandwidth_used * context.bandwidth_price (default 1000) when non_vm_blackhole_credit_flat is
None, and then requires sender balance >= (value + fee), or it returns Err("Insufficient balance").
- Missing TRON free bandwidth semantics: TRON first consumes free bandwidth/staked resources for non‑VM
transactions. Only when free bandwidth is insufficient does it burn TRX. The Phase 3 code ignores this and
charges TRX unconditionally, causing legitimate non‑VM transfers to fail with “Insufficient balance”.
- Wrong nonce semantics for non‑VM: The Phase 3 non‑VM path increments the sender “nonce” (Ethereum
concept). TRON non‑VM transfers do not increment an EVM nonce. This can create parity mismatches even when
it doesn’t halt.
- Incomplete contract-type handling: Java sends only a TxKind (NON_VM/VM). For TransferAssetContract
(TRC‑10), the current path treats it like a TRX transfer (uses value field to move TRX), which is
incorrect. Even if the stuck case here is a TransferContract, this is another correctness hole.
- Context defaults are risky: Fallbacks like block_number = 0 and block_timestamp = now can skew
resource/fee logic if a null block context ever leaks through (less likely here, but should be fixed for
determinism).

Concrete evidence

- CSV: 20250901-064114-89131436-remote-remote.csv shows 2040, tx_index 0–3, then stops.
- Rust logs (phase 3): multiple “Non-VM transaction execution failed: Insufficient balance: need 125000,
have 0”.
- Phase 2 logs: no such errors (same backend config), so Phase 3 regression is in the new non‑VM code
path.
- Phase 3 diff: service.rs implements execute_non_vm_transaction with fee deduction and nonce++ for
NON_VM; RemoteExecutionSPI classifies TransferContract/TransferAsset as NON_VM and sends only a generic
TronTransaction with a single value field.

Is Phase 3 “correct” per plan?

- No — it deviates from the plan in CLAUDE.md and planning.md:
    - The plan explicitly said not to attempt full TRON resource settlement in Phase 3 unless you can read
dynamic properties; and to keep non‑VM conservative (no enforced TRX charge unless explicitly configured).
    - The plan prioritized “burn mode emits no extra account change,” energy_used = 0, and parity — not
mandatory fee deduction from sender TRX for every non‑VM tx.

Root cause summary

- Phase 3 introduced a strict TRX fee deduction for non‑VM txs (using a fixed bandwidth price), which
is not TRON‑accurate and causes failures when the sender has 0 TRX but free bandwidth should cover the
transaction. That’s why the Phase 3 export halts in block 2040: the next tx in that block triggers this
error on the new non‑VM branch.

What to change (minimal, safe path)

- Disable forced TRX deduction for non‑VM unless explicitly configured:
    - If fees.mode = "burn" and non_vm_blackhole_credit_flat is None, set fee_amount = 0 and skip balance
check for the fee (keep debit only for the TRX transfer amount).
    - Do not increment nonce for non‑VM.
- Keep blackhole credit optional behind config:
    - Only credit blackhole when fees.mode = "blackhole" and a flat amount is configured
(non_vm_blackhole_credit_flat).
- Don’t treat TRC‑10 as TRX:
    - Either pass contract type (or asset id) in the proto so the backend can branch, or leave
TransferAsset on the VM/legacy path for now to avoid incorrect TRX debits.
- Ensure context is deterministic:
    - Always populate block_number, block_timestamp, coinbase from the real BlockCapsule; avoid 0/now
fallbacks during replay.

If you want, I can:

- Patch service.rs to make non‑VM burns free by default (no TRX fee) and remove nonce++ for NON_VM,
keeping optional blackhole credit only when configured.
- Gate any fee deduction behind non_vm_blackhole_credit_flat to match the plan.
- Leave TRC‑10 on VM/legacy path or add a proto field to disambiguate before enabling non‑VM handling
for it.

This should let the Phase 3 run progress past block 2040 and keep state_change_count parity for non‑VM
while avoiding the incorrect “Insufficient balance” errors.

---

think harder:

> Either pass contract type (or asset id) in the proto so the backend can branch, or leave TransferAsset  
on the VM/legacy path for now to avoid incorrect TRX debits.


What's your recommendation? based on the fact that TRC-10 tokens on the TRON blockchain are not executed  
using the TRON EVM or TVM (TRON Virtual Machine).  

---

Recommendation: carry explicit TRON contract type (and TRC‑10 metadata) in the proto, and handle TRC‑10 as non‑VM in the Rust backend — but keep it on the Java actuator path until the Rust storage/execution can actually update TRC‑10 ledgers.

Why

- TRC‑10 is non‑VM: It never runs on TVM/EVM. Modeling it as VM is incorrect and will silently miss asset balance updates.
- Backend readiness: Your Rust logs show missing TRC‑10 stores (“Database asset-issue-v2 not found”). Without these DBs wired, the backend cannot apply TRC‑10 state changes.
- CSV parity: Java actuators already produce the correct TRC‑10 deltas; forcing it through VM or a
half‑baked NON_VM path will break state_change_count and digests.

Short‑term (stabilize parity)

- Don’t route TransferAssetContract to Rust: leave it on Java actuators and remote storage (Java writes
via RemoteStorageSPI), so CSV reflects correct TRC‑10 balance changes.
- Keep TransferContract (TRX) on the Rust NON_VM path, but remove enforced TRX fee deduction by default
(no nonce++, no flat fee unless explicitly configured), to avoid the “Insufficient balance” halts you hit.

Medium‑term (correct architecture)

- Proto: add a contract_type (or a TRON contract_type enum mirroring Java) and include TRC‑10 asset_id
for TransferAssetContract.
- Rust: implement a TRC‑10 non‑VM processor:
    - Read/write TRC‑10 balances (account-asset, asset-issue-v2), handle account creation, bandwidth fee
semantics (burn vs blackhole credit) from config/dynamic properties.
    - Emit deterministic state changes matching Java’s journaling.
- Storage: expose the TRC‑10 databases in the storage engine and through the adapter.

What not to do

- Don’t send TRC‑10 down the VM path as a fallback — it misrepresents TRON semantics and won’t produce the
required asset balance deltas.


---

Think harder.

Now don't start the implementation, let's document all these detailed plan, with detailed todo lists, in CLAUDE.md

(
not only the "Don’t treat TRC‑10 as TRX" part

but also other parts including but not limited to:
- Disable forced TRX deduction for non‑VM unless explicitly configured
- Keep blackhole credit optional behind config
- Ensure context is deterministic
)

