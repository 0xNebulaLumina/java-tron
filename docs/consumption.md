下面把 **TRON（TRX）中关于 Bandwidth / Energy / 消耗（以及 burn/blackhole）** 的消耗方式做一个尽量全面、可复现的逐项清单与计算公式，并给出常用的查询/估算方法和实务建议。因为网络参数（**单位能量/带宽单价、免费带宽上限** 等）会随链上参数调整而变动，我会把**关键公式 + 如何实时查询** 一并给出。关键事实都标注了官方/权威来源，便于你复核或直接用 API 自动化。

---

# 各类交易类型及其资源消耗表

| 交易类型            | 消耗 TRX                 | 消耗 能量 | 消耗 带宽 | 资源抵扣后豁免                      | 消耗规则／示例                                                             |
| --------------- | ---------------------- | ----- | ----- | ---------------------------- | ------------------------------------------------------------------- |
| **普通 TRX 转账**   | 否（仅算带宽，带宽不足时燃烧TRX）     | 否     | 是     | 带宽足够时免TRX；可用冻结+免费带宽抵扣        | 带宽=交易字节数；若超免费+冻结带宽，上缴TRX=字节数×0.001 TRX。                             |
| **TRC-10 代币转账** | 否（同上，只算带宽）             | 否     | 是     | 同上                           | 同上（TRC-10不走TVM，仅消耗带宽）。                                              |
| **TRC-20 合约调用** | 否（消耗带宽和能量，资源不足时燃烧TRX）  | 是     | 是     | 账号冻结的能量和带宽先用；资源不足时按费用限额燃烧TRX | 带宽=字节数；能量≈执行智能合约所需TVM指令量。若能量不够，每能量收费0.0001 TRX；带宽不足，每字节收费0.001 TRX。 |
| **智能合约部署**      | 否（消耗带宽和大量能量，资源不足时烧TRX） | 是     | 是     | 同上                           | 合约部署相当于合约调用，消耗高额能量和带宽；例如简单合约可能耗几十万能量，若资源不足同样烧TRX。                   |
| **新账户创建**       | 是（固定1 TRX）             | 否     | 是     | 无豁免（固定费）                     | 激活新地址需付1 TRX创建费；此外若带宽不足需额外烧0.1 TRX。使用专门合约方式创建可免1 TRX，但需多消耗约25000能量。 |
| **TRC-10 代币发行** | 是（固定1024 TRX）          | 否     | 是     | 无豁免                          | 发行TRC-10代币需一次性支付1024 TRX（无能量消耗，仅带宽）。发行完成后，代币转账仅耗带宽。                 |

---

# 核心概念（快速回顾）

* **Bandwidth（带宽点）**：用来支付交易在链上的数据存储/传输，任何外发交易都会消耗（TRX 转账、TRC10 转账、合约触发都含带宽消耗）。带宽消耗 = 交易 protobuf 序列化后的字节数 × 带宽率（通常 =1）。每个外部账户每天有免费带宽（默认 600 点，链可调整）。([Tron Developer Network][1])
* **Energy（能量）**：用于 TVM（智能合约）的计算资源。只有触发/部署合约（如 TRC20 转账、TriggerSmartContract、Deploy）才消耗 Energy。Energy 一般通过冻结（stake）TRX 获得（优先消耗冻结得到的能量），不足时会**烧毁（burn）TRX**以换取能量。([Tron Developer Network][2])
* **燃烧 / 黑洞（burn / blackhole）**：当资源不足且未冻结/租赁时，链会按当前单价从账户扣 TRX（即“燃烧”或记录为消耗），历史上是把这些 TRX 转入黑洞地址，API 可查询烧毁总量（链参数/提案影响方式可能变化）。([Tron Developer Network][3])

---

# 统一的计费公式（必须掌握）

> **带宽部分**（如果可用免费带宽不足）：
> Burned\_TRX\_for\_bandwidth = max(0, bandwidth\_used - free\_bandwidth\_available) × bandwidth\_unit\_price
> （注意：链上单位通常用 `sun`，1 TRX = 1,000,000 sun） 。带宽\_used = 交易的 protobuf 字节长度（可通过创建 tx 并查看 raw\_data\_hex / 除 2 得到 bytes）。详见 docs。([Tron Developer Network][4], [Stack Overflow][5])

> **能量部分（合约执行）**：
> Burned\_TRX\_for\_energy = max(0, energy\_used - energy\_from\_stake) × energy\_unit\_price
> energy\_used 由 TVM 指令逐条计算；开发者可以通过本地/节点模拟或从节点返回的 txResult 查看实际消耗。合约交易还有 `fee_limit` 上限控制。([Tron Developer Network][2])

> **总体（发送方最终付费）**：
> 最终被烧 TRX ≈ Burned\_TRX\_for\_bandwidth + Burned\_TRX\_for\_energy + 可能的“新账户激活费/系统费用”
> （参数名例如 `getCreateAccountFee` / `getTransactionFee` 可通过 `/wallet/getchainparameters` 查询实时值）。([Tron Developer Network][6])

---

# 各类交易详列（逐项说明：消耗哪些资源 + 额外注意点 + 如何估算）

## 1) 普通 TRX 转账（TransferContract）

* **消耗**：**只消耗 Bandwidth**（交易字节数）。发送方消耗其账户的免费带宽（每天重置 600），不足时会按单价烧 TRX。([Tron Developer Network][1])
* **额外**：若接收方此前未在主网上“激活”（即从未有过交易/账户还不存在），发送给该地址可能触发“账户创建/激活费”（链参数可反映，不同版本参数可能为 0.1TRX 或 1TRX 等，需实时查询 `/wallet/getchainparameters`）。([Tron Developer Network][6], [Stack Overflow][7])
* **估算方法**：先用 `tron.createTransaction`（或相应 API 构造但不广播）得到 `raw_data_hex`，计算字节数（len/2，加上 protobuf overhead），即为带宽点数。再乘以当前带宽单价（从 `getChainParameters` 或 `getBandwidthPrices` 查询）。（如何用代码算 tx 大小可见 StackOverflow / docs）。([Stack Overflow][5], [Tron Developer Network][8])

## 2) TRC-10 转账（TransferAssetContract）

* **消耗**：**只消耗 Bandwidth**（TRC10 是系统级资产，不需 TVM 合约执行），因此不会消耗 Energy（除非在合约内触发 TRC10，少见）。发行 TRC10 本身会有一次性系统费用（发布费，典型为 1024 TRX）。([Tron Developer Network][9])
* **注意**：TRC10 更“轻量”，通常比 TRC20 便宜；交易仍按 tx 字节数计带宽。若发送到未激活账户，同样可能触发账户创建费。([support.tronscan.org][10])

## 3) TRC-20（或任意智能合约）转账 / 调用（TriggerSmartContract）

* **消耗**：同时消耗 **Bandwidth + Energy**。带宽覆盖交易数据，Energy 覆盖合约执行的计算（TVM 指令）。先消耗 stake 得到的 Energy，若不足会烧 TRX。([Tron Developer Network][2])
* **常见示例（USDT/TRC20）**：USDT 一类 TRC20 转账通常消耗大量 Energy（公开资料示例：第一次向空地址转 USDT 可能要 \~130,000 energy；非空地址后续约 65,000 energy 的区间），因此如果没有冻结能量，可能被烧掉数十 TRX（示例按能量单价换算）。**这些数字会随合约实现/链参数变化，务必用节点模拟或能量估算器验证**。([tronex.energy][11], [Netts][12])
* **如何预估**：

  1. 在本地或通过节点 `triggerconstantcontract` / 模拟接口先估算 `energy_used`（或用工具/fee-calculator）。
  2. 带宽同样用 raw tx bytes 估算。
  3. 用上面的公式换算成要烧 TRX（或判断是否用冻结能量覆盖）。([Tron Developer Network][8], [tr.energy][13])

## 4) 部署合约（CreateSmartContract）

* **消耗**：大量 **Energy + Bandwidth**。部署合约要指定 `fee_limit`（上限控制），合约代码复杂度直接决定 energy 用量；若 energy 不够会触发烧 TRX 或失败。([Tron Developer Network][14])
* **建议**：部署前在测试网充分模拟（测能量），并设置合理 `fee_limit` 防止意外“大烧”。([Tron Developer Network][15])

## 5) 系统交易（冻结/解冻、投票、资产发行、合约注册等）

* **冻结（freeze）/解冻（unfreeze）**：通常是普通交易（带宽），但 freeze 会“锁仓”以产生 Bandwidth / Energy（取决于你选择冻结的资源类型）；该操作本身会产生带宽消耗（若带宽不足则烧 TRX）。并且冻结能得到 TRON Power（投票权）。([Tron Developer Network][16])
* **发行 TRC10（AssetIssue）**：一次性系统费用（典型值 1024 TRX），并会写入链上数据（带宽）。([Tron Developer Network][9])

## 6) 查询操作 / constant call（调用 view / constant 方法）

* **消耗**：**不消耗 Bandwidth/Energy**（因为不是上链交易，是 RPC 查询或 constant 调用）。可以用来估算合约执行结果/预估 gas。([Tron Developer Network][4])

---

# 关于单价（bandwidth\_unit\_price / energy\_unit\_price）与实时查询

* 链上这些参数**会变**（例如 `getTransactionFee`、`getEnergyFee`、免费带宽阈值等），因此**必须**通过节点接口 `/wallet/getchainparameters` 或 `GetBandwidthPrices` 查询当前数值再做最终换算。开发/产品化时把这些 API 调用放到流水线里即可。示例 API 文档与参数说明：`getchainparameters`（可查到 getTransactionFee、getCreateAccountFee、getEnergyFee、getFreeNetLimit 等）。([Tron Developer Network][6])

* 官方示例历史/典型数值（仅作说明，实际以链上查询为准）：resource model 文档示例曾列出带宽单价约 **1000 sun (0.001 TRX)**、能量单价约 **280 sun (0.00028 TRX)**（历史上也出现过不同数值/提案），因此**不要把这些当成固定常量**——务必实时查询。([Tron Developer Network][17])

---

# burn / blackhole 的额外说明（你提到的 burn/blackhole）

1. **自动燃烧（因资源不足）**：节点会按上面的公式直接从账户扣 TRX（记录为“burn”/费用），历史上这些被转入“黑洞地址”或被节点内部记账；TRON 提供 API 查询过去/当前燃烧量（如 `getBurnTRX`）。注意：链上治理（委员会/提案）会影响燃烧的具体处理方式（是否仍转入黑洞地址等）。([Tron Developer Network][3], [GitHub][18])
2. **手动/事件性燃烧（项目方、稳定币回购销毁等）**：一些项目会把代币发到所谓黑洞地址 `0x000...`（不可控地址）以做销毁；TRON 团队/项目也会做定期烧币活动（新闻可见）。这些不是“资源费”，而是项目层面的供应管理。([trx.tokenview.io][19], [The Currency analytics][20])

---

# 编程/自动化：如何在代码里预估/避免“被烧 TRX”

1. **查询链参数（实时）**：`/wallet/getchainparameters`（获取带宽/能量单价、创建账号费、免费带宽上限等）。([Tron Developer Network][6])
2. **构造但不广播 tx，读取 raw\_data\_hex**，计算字节数（/2）得到带宽消耗估算；对于合约调用，可以先 `triggerconstantcontract` 或本地 vm-run 来估算 energy。([Stack Overflow][5], [Tron Developer Network][8])
3. **如果预计 energy 会很高**：优先使用冻结（freeze）得到 energy 或通过 `delegateResource`（Stake2.0）/第三方租赁（energy rental 服务如 TronRent、TronNRG 等）来避免高额燃烧。示例 API：`delegateresource`。([Tron Developer Network][21], [support.tronnrg.com][22])
4. **在构造 tx 时设置 `fee_limit`**：避免无限制消耗导致意外大额烧币（fee\_limit 是防火墙）。([Tron Developer Network][15])

---

# 常见数字示例（仅做示范，**不要直接用于结算**，请先用 `/wallet/getchainparameters` 验证）

* 免费带宽：**600** 点/天（默认，链可变）。([Tron Developer Network][6])
* 带宽单价（历史示例/常见值）：**1000 sun = 0.001 TRX / 带宽点**（链可变）。([Tron Developer Network][17])
* 能量单价（历史示例）：**280 sun = 0.00028 TRX / energy**（链可变；其他快照也见 0.0001TRX/energy 等，请以 getchainparameters 为准）。([Tron Developer Network][2])
* TRC20（如 USDT）转账示例：**\~65,000—130,000 energy**（具体取决于合约实现/是否首次向空账户等），换算成 TRX 会是若干 TRX（取决于当前 energy 单价）。这些示例可从多个 fee-calculator/博客与工具得到近似值（tr.energy、tronsave、gasfeesnow 等）。([tronex.energy][11], [Netts][12])

---

# 快速检查表（部署到产品/自动化前）

* [ ] 在你的节点/服务中定期拉 `getchainparameters`、`getBandwidthPrices`，缓存短时有效值。([Tron Developer Network][23])
* [ ] 所有关键转账/合约调用先用模拟接口估算 `bandwidth_used`（raw\_data\_hex）和 `energy_used`（模拟/triggerConstant）。([Stack Overflow][5], [Tron Developer Network][8])
* [ ] 如果预估会消耗大量 energy：优先 freeze（stake）或租赁 energy / delegateResource；并在 tx 中设置 `fee_limit`。([Tron Developer Network][24])
* [ ] 记录 /wallet/getburntrx（或监控黑洞地址）以审计链上烧币开销/对账。([Tron Developer Network][3])

---

# 参考（关键文档 / 工具）

* TRON Developer Hub — Resource Model / Energy Consumption / Frozen Energy & fee\_limit（说明资源如何计算、优先消耗顺序、单位价等）。([Tron Developer Network][2])
* TRON Developer Hub — getChainParameters / getBandwidthPrices（用于实时查询带宽/能量单价与系统参数）。([Tron Developer Network][6])
* TRC-10 / TRC-20 标准与合约文档（说明 TRC10 为系统级、TRC20 属合约级，消耗差异）。([Tron Developer Network][9])
* Tronscan / Tronsave / 多个 fee-calculator 博客（示例：USDT/TRC20 能量区间估算与实际案例）。([tronex.energy][11], [Netts][12])
* getBurnTRX / blackhole 相关 API（查询链上已被销毁 TRX 的接口与说明）。([Tron Developer Network][3])

---

[1]: https://developers.tron.network/v4.4.0/docs/resource-model?utm_source=chatgpt.com "Resource Model - TRON Developer Hub"
[2]: https://developers.tron.network/v4.4.2/docs/resource-model?utm_source=chatgpt.com "Resource Model - TRON Developer Hub"
[3]: https://developers.tron.network/reference/getburntrx-1?utm_source=chatgpt.com "GetBurnTRX - TRON Developer Hub"
[4]: https://developers.tron.network/docs/tron-protocol-transaction?utm_source=chatgpt.com "Transactions - TRON Developer Hub"
[5]: https://stackoverflow.com/questions/75554365/tron-how-to-calculate-transaction-bandwidth-before-broadcast-transaction-to-the?utm_source=chatgpt.com "tron: how to calculate transaction bandwidth before broadcast ..."
[6]: https://developers.tron.network/reference/wallet-getchainparameters?utm_source=chatgpt.com "GetChainParameters - TRON Developer Hub"
[7]: https://stackoverflow.com/questions/76747393/tron-api-get-the-number-of-burned-trx-before-making-a-transaction?utm_source=chatgpt.com "Tron API get the number of burned TRX before making a transaction"
[8]: https://developers.tron.network/docs/faq?utm_source=chatgpt.com "FAQ - TRON Developer Hub"
[9]: https://developers.tron.network/docs/trc10?utm_source=chatgpt.com "TRC-10 - TRON Developer Hub"
[10]: https://support.tronscan.org/hc/en-us/articles/360027103751-What-are-the-differences-between-TRC10-and-TRC20-Tokens?utm_source=chatgpt.com "What are the differences between TRC10 and TRC20 Tokens?"
[11]: https://tronex.energy/blog/understanding-trc20-fees-how-much-does-it-cost-to-send-usdt-on-the-tron-network?utm_source=chatgpt.com "Understanding TRC20 fees: how much does it cost to send USDT on the ..."
[12]: https://netts.io/tools/converter/?utm_source=chatgpt.com "USDT Energy Calculator - Calculate Energy Requirements | Netts.io"
[13]: https://tr.energy/en/tron-energy-calculator/?utm_source=chatgpt.com "Tron Energy and Fee Calculator – Accurate Calculation for USDT ..."
[14]: https://developers.tron.network/v4.4.0/docs/trc-20-contracts?utm_source=chatgpt.com "TRC-20 Contracts - developers.tron.network"
[15]: https://developers.tron.network/v4.4.0/docs/frozen-energy-and-fee-limit-model?utm_source=chatgpt.com "Stake TRX for Energy and OUT_OF_ENERGY - TRON Developer Hub"
[16]: https://developers.tron.network/v4.0.0/docs/resource-model?utm_source=chatgpt.com "Resource Model - TRON Developer Hub"
[17]: https://developers.tron.network/docs/resource-model?utm_source=chatgpt.com "Resource Model - TRON Developer Hub"
[18]: https://github.com/tronprotocol/tips/issues/234?utm_source=chatgpt.com "Proposal: Optimize the black hole accounts to increase the ... - GitHub"
[19]: https://trx.tokenview.io/en/address/0000000000000000000000000000000000?utm_source=chatgpt.com "Black hole: Address (0) without private key, often used for token burn ..."
[20]: https://thecurrencyanalytics.com/altcoins/tron-burns-10-million-tokens-is-this-the-catalyst-trx-needs-for-a-price-surge-141316?utm_source=chatgpt.com "Tron Burns 10 Million Tokens: Is This the Catalyst TRX Needs for a ..."
[21]: https://developers.tron.network/reference/delegateresource-1?utm_source=chatgpt.com "DelegateResource - TRON Developer Hub"
[22]: https://support.tronnrg.com/developer-docs/rent-tron-energy-tronweb?utm_source=chatgpt.com "Rent Tron Energy - TronWeb | NRG"
[23]: https://developers.tron.network/reference/getbandwidthprices?utm_source=chatgpt.com "GetBandwidthPrices - developers.tron.network"
[24]: https://developers.tron.network/v4.7.0/reference/delegateresource?utm_source=chatgpt.com "delegateResource - developers.tron.network"
