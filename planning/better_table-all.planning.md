think harder.

currently the csv only track account_changes (balance, nonce, code_hash, code_len)

but not other changes: 
+ EVM storage
+ chainbase tables (TRC‑10 Balances, TRC‑10 Issuance, votes, freeze records)
+ Global Resource
+ account resources usage
+ Logs
+ Contract lifecycle: creations (address, code_hash, creator), selfdestructs (beneficiary, balance moved), code updates/clear ABI.
+ Contract settings: consume_user_resource_percent, origin_energy_limit, permission_id defaults, is_disabled.
+ Internal calls and value transfers: CALL/DELEGATECALL/STATICCALL/CREATE trees and TRX value edges (from→to, reason, depth).
+ TRC‑20/721/1155 decoded events: transfers, approvals, mints/burns, batch transfers (derived from logs with known ABIs).
+ Account permissions: Owner/Active/Witness permission updates, threshold changes, key additions/removals, permission_id changes.
+ Governance proposals: create/approve/delete, per‑proposal approvals, status transitions, and parameter deltas activated.
+ Governance parameters: concrete DynamicProperties deltas caused by proposals (min/max fees, prices, feature flags).
+ Witness/SR set: register/update/resign, URL/name changes, total vote tally deltas, rank changes, schedule/producer set switches (when maintenance triggers).
+ Rewards and payouts: SR reward accruals and WithdrawBalance transfers, community pool movements.
+ TRC‑10 ownership and supply ops: asset ownership transfers, precision/description/url changes, freeze_supply/unfreeze_supply.
+ Resource delegation: DelegateResource/UnDelegateResource records separate from self‑freeze (who→who, ENERGY/BANDWIDTH).
+ Maintenance-cycle effects: maintenance boundary activation, next_maintenance_time roll, scheduled housekeeping applied this tx.
+ Exchange/market (built‑in DEX): create/inject/withdraw/exchange trades; per‑exchange reserves and price parameters.
+ Shielded (zk) state: note commitments appended, nullifiers set, anchors/merkle roots updates, shielded value in/out (if TronZ enabled).
+ Account name/id: SetAccountId mappings and collisions/resolutions.
+ Fee/supply flows: fee breakdown deltas (energy_fee, net_fee, storage_fee, multi_sig_fee), burned_trx, minted/tranche movements (if any).
+ Storage summary: per‑contract slots_touched, bytes_added/removed; useful for quick diff stats and audits.
+ Precompile usage: which precompiles called and how many times (e.g., ecrecover, sha256), for performance/security forensics.
+ Blacklist/whitelist and sanctions state: account/contract lists or toggles if your build enables these governance features.
+ Cross‑chain/bridge state: lock/mint/burn records and pending cross‑chain ops (if sidechains/bridges are enabled in your config).
+ ...

no need to care about backward compability, help me redesign a new csv table, keep the original
`exec_mode,storage_mode,block_num,block_id_hex,is_witness_signed,block_timestamp,tx_index_in_block,tx_id_hex,owner_address_hex,contract_type,is_constant,fee_limit,is_success,result_code,energy_used,return_data_hex,return_data_len,runtime_error`

keep using 1 csv, no need to record different changes (EVM storage, chainbase tables, etc) in different csv.


btw when tracking these other changes, also track _changes_json, _change_count, _digest_sha256 like we did for state_changes.
