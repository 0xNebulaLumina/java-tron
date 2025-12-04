package org.tron.core.execution.reporting;

import com.fasterxml.jackson.annotation.JsonIgnore;
import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import java.util.ArrayList;
import java.util.List;
import org.tron.common.utils.ByteArray;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;

/**
 * Data model for a single CSV record representing transaction execution details.
 *
 * <p>This record captures comprehensive execution information for both remote and embedded
 * execution modes, enabling offline comparison and analysis.
 *
 * <p>Schema includes base columns plus domain triplets for:
 * - State changes (legacy aggregate: account + EVM storage)
 * - Account changes (balance, nonce, code_hash, code_len)
 * - EVM storage changes
 * - TRC-10 balance changes
 * - TRC-10 issuance changes
 * - Vote changes
 * - Freeze changes
 * - Global resource changes
 * - Account resource usage (AEXT) changes
 * - Log entries
 */
public class ExecutionCsvRecord {

  // Run metadata
  private String runId;
  private String execMode;  // EMBEDDED|REMOTE
  private String storageMode;  // EMBEDDED|REMOTE

  // Block information
  private long blockNum;
  private String blockIdHex;
  private boolean isWitnessSigned;
  private long blockTimestamp;

  // Transaction information
  private int txIndexInBlock;
  private String txIdHex;
  private String ownerAddressHex;
  private String contractType;
  private boolean isConstant;
  private long feeLimit;

  // Execution results
  private boolean isSuccess;
  private String resultCode;
  private long energyUsed;
  private String returnDataHex;
  private int returnDataLen;
  private String runtimeError;

  // Legacy state changes (aggregate of account + EVM storage)
  private int stateChangeCount;
  private String stateChangesJson;
  private String stateDigestSha256;

  // Domain: Account changes
  private String accountChangesJson;
  private int accountChangeCount;
  private String accountDigestSha256;

  // Domain: EVM storage changes
  private String evmStorageChangesJson;
  private int evmStorageChangeCount;
  private String evmStorageDigestSha256;

  // Domain: TRC-10 balance changes
  private String trc10BalanceChangesJson;
  private int trc10BalanceChangeCount;
  private String trc10BalanceDigestSha256;

  // Domain: TRC-10 issuance changes
  private String trc10IssuanceChangesJson;
  private int trc10IssuanceChangeCount;
  private String trc10IssuanceDigestSha256;

  // Domain: Vote changes
  private String voteChangesJson;
  private int voteChangeCount;
  private String voteDigestSha256;

  // Domain: Freeze changes
  private String freezeChangesJson;
  private int freezeChangeCount;
  private String freezeDigestSha256;

  // Domain: Global resource changes
  private String globalResourceChangesJson;
  private int globalResourceChangeCount;
  private String globalResourceDigestSha256;

  // Domain: Account resource usage (AEXT) changes
  private String accountResourceUsageChangesJson;
  private int accountResourceUsageChangeCount;
  private String accountResourceUsageDigestSha256;

  // Domain: Log entries
  private String logEntriesJson;
  private int logEntryCount;
  private String logEntriesDigestSha256;

  // Metadata
  private long tsMs;

  @JsonIgnore
  private static final ObjectMapper objectMapper = new ObjectMapper();

  /**
   * Default constructor.
   */
  public ExecutionCsvRecord() {
    this.stateChangesJson = "";
    this.runtimeError = "";
    this.accountChangesJson = "[]";
    this.evmStorageChangesJson = "[]";
    this.trc10BalanceChangesJson = "[]";
    this.trc10IssuanceChangesJson = "[]";
    this.voteChangesJson = "[]";
    this.freezeChangesJson = "[]";
    this.globalResourceChangesJson = "[]";
    this.accountResourceUsageChangesJson = "[]";
    this.logEntriesJson = "[]";
    this.accountDigestSha256 = "";
    this.evmStorageDigestSha256 = "";
    this.trc10BalanceDigestSha256 = "";
    this.trc10IssuanceDigestSha256 = "";
    this.voteDigestSha256 = "";
    this.freezeDigestSha256 = "";
    this.globalResourceDigestSha256 = "";
    this.accountResourceUsageDigestSha256 = "";
    this.logEntriesDigestSha256 = "";
    this.tsMs = System.currentTimeMillis();
  }

  /**
   * Builder pattern for constructing records.
   */
  public static Builder builder() {
    return new Builder();
  }

  /**
   * Builder class for ExecutionCsvRecord.
   */
  public static class Builder {
    private ExecutionCsvRecord record = new ExecutionCsvRecord();

    public Builder runId(String runId) {
      record.runId = runId;
      return this;
    }

    public Builder execMode(String execMode) {
      record.execMode = execMode;
      return this;
    }

    public Builder storageMode(String storageMode) {
      record.storageMode = storageMode;
      return this;
    }

    public Builder blockNum(long blockNum) {
      record.blockNum = blockNum;
      return this;
    }

    public Builder blockIdHex(String blockIdHex) {
      record.blockIdHex = blockIdHex;
      return this;
    }

    public Builder blockIdHex(byte[] blockId) {
      record.blockIdHex = blockId != null ? ByteArray.toHexString(blockId) : "";
      return this;
    }

    public Builder isWitnessSigned(boolean isWitnessSigned) {
      record.isWitnessSigned = isWitnessSigned;
      return this;
    }

    public Builder blockTimestamp(long blockTimestamp) {
      record.blockTimestamp = blockTimestamp;
      return this;
    }

    public Builder txIndexInBlock(int txIndexInBlock) {
      record.txIndexInBlock = txIndexInBlock;
      return this;
    }

    public Builder txIdHex(String txIdHex) {
      record.txIdHex = txIdHex;
      return this;
    }

    public Builder txIdHex(byte[] txId) {
      record.txIdHex = txId != null ? ByteArray.toHexString(txId) : "";
      return this;
    }

    public Builder ownerAddressHex(String ownerAddressHex) {
      record.ownerAddressHex = ownerAddressHex;
      return this;
    }

    public Builder ownerAddressHex(byte[] ownerAddress) {
      record.ownerAddressHex = ownerAddress != null ? ByteArray.toHexString(ownerAddress) : "";
      return this;
    }

    public Builder contractType(String contractType) {
      record.contractType = contractType;
      return this;
    }

    public Builder isConstant(boolean isConstant) {
      record.isConstant = isConstant;
      return this;
    }

    public Builder feeLimit(long feeLimit) {
      record.feeLimit = feeLimit;
      return this;
    }

    public Builder isSuccess(boolean isSuccess) {
      record.isSuccess = isSuccess;
      return this;
    }

    public Builder resultCode(String resultCode) {
      record.resultCode = resultCode;
      return this;
    }

    public Builder energyUsed(long energyUsed) {
      record.energyUsed = energyUsed;
      return this;
    }

    public Builder returnDataHex(String returnDataHex) {
      record.returnDataHex = returnDataHex != null ? returnDataHex : "";
      return this;
    }

    public Builder returnDataHex(byte[] returnData) {
      if (returnData != null) {
        record.returnDataHex = ByteArray.toHexString(returnData);
        record.returnDataLen = returnData.length;
      } else {
        record.returnDataHex = "";
        record.returnDataLen = 0;
      }
      return this;
    }

    public Builder returnDataLen(int returnDataLen) {
      record.returnDataLen = returnDataLen;
      return this;
    }

    public Builder runtimeError(String runtimeError) {
      record.runtimeError = runtimeError != null ? sanitizeForCsv(runtimeError) : "";
      return this;
    }

    public Builder stateChangeCount(int stateChangeCount) {
      record.stateChangeCount = stateChangeCount;
      return this;
    }

    public Builder stateChanges(List<StateChange> stateChanges) {
      if (stateChanges != null && !stateChanges.isEmpty()) {
        record.stateChangeCount = stateChanges.size();
        record.stateChangesJson = convertStateChangesToJson(stateChanges);
        // Auto-compute digest if not already set
        if (record.stateDigestSha256 == null || record.stateDigestSha256.isEmpty()) {
          record.stateDigestSha256 = StateChangeCanonicalizer.computeStateDigest(stateChanges);
        }
      } else {
        record.stateChangeCount = 0;
        record.stateChangesJson = "";
        // Auto-compute empty digest if not already set
        if (record.stateDigestSha256 == null || record.stateDigestSha256.isEmpty()) {
          record.stateDigestSha256 = StateChangeCanonicalizer.computeEmptyStateDigest();
        }
      }
      return this;
    }

    public Builder stateDigestSha256(String stateDigestSha256) {
      record.stateDigestSha256 = stateDigestSha256 != null ? stateDigestSha256 : "";
      return this;
    }

    // Domain: Account changes
    public Builder accountChangesJson(String json) {
      record.accountChangesJson = json != null ? json : "[]";
      return this;
    }

    public Builder accountChangeCount(int count) {
      record.accountChangeCount = count;
      return this;
    }

    public Builder accountDigestSha256(String digest) {
      record.accountDigestSha256 = digest != null ? digest : "";
      return this;
    }

    public Builder accountDomain(DomainCanonicalizer.DomainResult result) {
      record.accountChangesJson = result.getJson();
      record.accountChangeCount = result.getCount();
      record.accountDigestSha256 = result.getDigest();
      return this;
    }

    // Domain: EVM storage changes
    public Builder evmStorageChangesJson(String json) {
      record.evmStorageChangesJson = json != null ? json : "[]";
      return this;
    }

    public Builder evmStorageChangeCount(int count) {
      record.evmStorageChangeCount = count;
      return this;
    }

    public Builder evmStorageDigestSha256(String digest) {
      record.evmStorageDigestSha256 = digest != null ? digest : "";
      return this;
    }

    public Builder evmStorageDomain(DomainCanonicalizer.DomainResult result) {
      record.evmStorageChangesJson = result.getJson();
      record.evmStorageChangeCount = result.getCount();
      record.evmStorageDigestSha256 = result.getDigest();
      return this;
    }

    // Domain: TRC-10 balance changes
    public Builder trc10BalanceChangesJson(String json) {
      record.trc10BalanceChangesJson = json != null ? json : "[]";
      return this;
    }

    public Builder trc10BalanceChangeCount(int count) {
      record.trc10BalanceChangeCount = count;
      return this;
    }

    public Builder trc10BalanceDigestSha256(String digest) {
      record.trc10BalanceDigestSha256 = digest != null ? digest : "";
      return this;
    }

    public Builder trc10BalanceDomain(DomainCanonicalizer.DomainResult result) {
      record.trc10BalanceChangesJson = result.getJson();
      record.trc10BalanceChangeCount = result.getCount();
      record.trc10BalanceDigestSha256 = result.getDigest();
      return this;
    }

    // Domain: TRC-10 issuance changes
    public Builder trc10IssuanceChangesJson(String json) {
      record.trc10IssuanceChangesJson = json != null ? json : "[]";
      return this;
    }

    public Builder trc10IssuanceChangeCount(int count) {
      record.trc10IssuanceChangeCount = count;
      return this;
    }

    public Builder trc10IssuanceDigestSha256(String digest) {
      record.trc10IssuanceDigestSha256 = digest != null ? digest : "";
      return this;
    }

    public Builder trc10IssuanceDomain(DomainCanonicalizer.DomainResult result) {
      record.trc10IssuanceChangesJson = result.getJson();
      record.trc10IssuanceChangeCount = result.getCount();
      record.trc10IssuanceDigestSha256 = result.getDigest();
      return this;
    }

    // Domain: Vote changes
    public Builder voteChangesJson(String json) {
      record.voteChangesJson = json != null ? json : "[]";
      return this;
    }

    public Builder voteChangeCount(int count) {
      record.voteChangeCount = count;
      return this;
    }

    public Builder voteDigestSha256(String digest) {
      record.voteDigestSha256 = digest != null ? digest : "";
      return this;
    }

    public Builder voteDomain(DomainCanonicalizer.DomainResult result) {
      record.voteChangesJson = result.getJson();
      record.voteChangeCount = result.getCount();
      record.voteDigestSha256 = result.getDigest();
      return this;
    }

    // Domain: Freeze changes
    public Builder freezeChangesJson(String json) {
      record.freezeChangesJson = json != null ? json : "[]";
      return this;
    }

    public Builder freezeChangeCount(int count) {
      record.freezeChangeCount = count;
      return this;
    }

    public Builder freezeDigestSha256(String digest) {
      record.freezeDigestSha256 = digest != null ? digest : "";
      return this;
    }

    public Builder freezeDomain(DomainCanonicalizer.DomainResult result) {
      record.freezeChangesJson = result.getJson();
      record.freezeChangeCount = result.getCount();
      record.freezeDigestSha256 = result.getDigest();
      return this;
    }

    // Domain: Global resource changes
    public Builder globalResourceChangesJson(String json) {
      record.globalResourceChangesJson = json != null ? json : "[]";
      return this;
    }

    public Builder globalResourceChangeCount(int count) {
      record.globalResourceChangeCount = count;
      return this;
    }

    public Builder globalResourceDigestSha256(String digest) {
      record.globalResourceDigestSha256 = digest != null ? digest : "";
      return this;
    }

    public Builder globalResourceDomain(DomainCanonicalizer.DomainResult result) {
      record.globalResourceChangesJson = result.getJson();
      record.globalResourceChangeCount = result.getCount();
      record.globalResourceDigestSha256 = result.getDigest();
      return this;
    }

    // Domain: Account resource usage (AEXT) changes
    public Builder accountResourceUsageChangesJson(String json) {
      record.accountResourceUsageChangesJson = json != null ? json : "[]";
      return this;
    }

    public Builder accountResourceUsageChangeCount(int count) {
      record.accountResourceUsageChangeCount = count;
      return this;
    }

    public Builder accountResourceUsageDigestSha256(String digest) {
      record.accountResourceUsageDigestSha256 = digest != null ? digest : "";
      return this;
    }

    public Builder accountResourceUsageDomain(DomainCanonicalizer.DomainResult result) {
      record.accountResourceUsageChangesJson = result.getJson();
      record.accountResourceUsageChangeCount = result.getCount();
      record.accountResourceUsageDigestSha256 = result.getDigest();
      return this;
    }

    // Domain: Log entries
    public Builder logEntriesJson(String json) {
      record.logEntriesJson = json != null ? json : "[]";
      return this;
    }

    public Builder logEntryCount(int count) {
      record.logEntryCount = count;
      return this;
    }

    public Builder logEntriesDigestSha256(String digest) {
      record.logEntriesDigestSha256 = digest != null ? digest : "";
      return this;
    }

    public Builder logsDomain(DomainCanonicalizer.DomainResult result) {
      record.logEntriesJson = result.getJson();
      record.logEntryCount = result.getCount();
      record.logEntriesDigestSha256 = result.getDigest();
      return this;
    }

    public Builder tsMs(long tsMs) {
      record.tsMs = tsMs;
      return this;
    }

    public ExecutionCsvRecord build() {
      return record;
    }

    /**
     * Convert state changes to JSON string for CSV storage.
     *
     * @param stateChanges List of state changes
     * @return JSON string representation
     */
    private String convertStateChangesToJson(List<StateChange> stateChanges) {
      try {
        List<StateChangeEntry> entries = new ArrayList<>();
        for (StateChange change : stateChanges) {
          StateChangeEntry entry = new StateChangeEntry();
          entry.address = change.getAddress() != null ? ByteArray.toHexString(change.getAddress()) : "";
          entry.key = change.getKey() != null ? ByteArray.toHexString(change.getKey()) : "";
          entry.oldValue = change.getOldValue() != null ? ByteArray.toHexString(change.getOldValue()) : "";
          entry.newValue = change.getNewValue() != null ? ByteArray.toHexString(change.getNewValue()) : "";
          entries.add(entry);
        }
        // Sort state changes by address for deterministic ordering
        entries.sort((a, b) -> a.address.compareToIgnoreCase(b.address));

        return objectMapper.writeValueAsString(entries);
      } catch (JsonProcessingException e) {
        // Fallback to simple string representation with sorting
        List<StateChange> sortedChanges = new ArrayList<>(stateChanges);
        sortedChanges.sort((a, b) -> {
          String addrA = a.getAddress() != null ? ByteArray.toHexString(a.getAddress()) : "";
          String addrB = b.getAddress() != null ? ByteArray.toHexString(b.getAddress()) : "";
          return addrA.compareToIgnoreCase(addrB);
        });

        StringBuilder sb = new StringBuilder("[");
        for (int i = 0; i < sortedChanges.size(); i++) {
          if (i > 0) sb.append(",");
          StateChange change = sortedChanges.get(i);
          sb.append("{\"address\":\"")
              .append(change.getAddress() != null ? ByteArray.toHexString(change.getAddress()) : "")
              .append("\",\"key\":\"")
              .append(change.getKey() != null ? ByteArray.toHexString(change.getKey()) : "")
              .append("\",\"oldValue\":\"")
              .append(change.getOldValue() != null ? ByteArray.toHexString(change.getOldValue()) : "")
              .append("\",\"newValue\":\"")
              .append(change.getNewValue() != null ? ByteArray.toHexString(change.getNewValue()) : "")
              .append("\"}");
        }
        sb.append("]");
        return sb.toString();
      }
    }

    /**
     * Sanitize string for CSV by removing/escaping problematic characters.
     */
    private String sanitizeForCsv(String str) {
      if (str == null || str.isEmpty()) {
        return "";
      }
      // Remove newlines and carriage returns, escape quotes
      return str.replaceAll("[\r\n]", " ").replace("\"", "\"\"");
    }
  }

  /**
   * State change entry for JSON serialization.
   */
  public static class StateChangeEntry {
    public String address;
    public String key;
    public String oldValue;
    public String newValue;
  }

  /**
   * Convert this record to a CSV row string.
   *
   * @return CSV-formatted string
   */
  public String toCsvRow() {
    StringBuilder sb = new StringBuilder();

    // Base columns
    sb.append(escapeForCsv(runId)).append(",");
    sb.append(escapeForCsv(execMode)).append(",");
    sb.append(escapeForCsv(storageMode)).append(",");
    sb.append(blockNum).append(",");
    sb.append(escapeForCsv(blockIdHex)).append(",");
    sb.append(isWitnessSigned).append(",");
    sb.append(blockTimestamp).append(",");
    sb.append(txIndexInBlock).append(",");
    sb.append(escapeForCsv(txIdHex)).append(",");
    sb.append(escapeForCsv(ownerAddressHex)).append(",");
    sb.append(escapeForCsv(contractType)).append(",");
    sb.append(isConstant).append(",");
    sb.append(feeLimit).append(",");
    sb.append(isSuccess).append(",");
    sb.append(escapeForCsv(resultCode)).append(",");
    sb.append(energyUsed).append(",");
    sb.append(escapeForCsv(returnDataHex)).append(",");
    sb.append(returnDataLen).append(",");
    sb.append(escapeForCsv(runtimeError)).append(",");

    // Legacy state changes (aggregate)
    sb.append(escapeForCsv(stateChangesJson)).append(",");
    sb.append(stateChangeCount).append(",");
    sb.append(escapeForCsv(stateDigestSha256)).append(",");

    // Domain: Account changes
    sb.append(escapeForCsv(accountChangesJson)).append(",");
    sb.append(accountChangeCount).append(",");
    sb.append(escapeForCsv(accountDigestSha256)).append(",");

    // Domain: EVM storage changes
    sb.append(escapeForCsv(evmStorageChangesJson)).append(",");
    sb.append(evmStorageChangeCount).append(",");
    sb.append(escapeForCsv(evmStorageDigestSha256)).append(",");

    // Domain: TRC-10 balance changes
    sb.append(escapeForCsv(trc10BalanceChangesJson)).append(",");
    sb.append(trc10BalanceChangeCount).append(",");
    sb.append(escapeForCsv(trc10BalanceDigestSha256)).append(",");

    // Domain: TRC-10 issuance changes
    sb.append(escapeForCsv(trc10IssuanceChangesJson)).append(",");
    sb.append(trc10IssuanceChangeCount).append(",");
    sb.append(escapeForCsv(trc10IssuanceDigestSha256)).append(",");

    // Domain: Vote changes
    sb.append(escapeForCsv(voteChangesJson)).append(",");
    sb.append(voteChangeCount).append(",");
    sb.append(escapeForCsv(voteDigestSha256)).append(",");

    // Domain: Freeze changes
    sb.append(escapeForCsv(freezeChangesJson)).append(",");
    sb.append(freezeChangeCount).append(",");
    sb.append(escapeForCsv(freezeDigestSha256)).append(",");

    // Domain: Global resource changes
    sb.append(escapeForCsv(globalResourceChangesJson)).append(",");
    sb.append(globalResourceChangeCount).append(",");
    sb.append(escapeForCsv(globalResourceDigestSha256)).append(",");

    // Domain: Account resource usage (AEXT) changes
    sb.append(escapeForCsv(accountResourceUsageChangesJson)).append(",");
    sb.append(accountResourceUsageChangeCount).append(",");
    sb.append(escapeForCsv(accountResourceUsageDigestSha256)).append(",");

    // Domain: Log entries
    sb.append(escapeForCsv(logEntriesJson)).append(",");
    sb.append(logEntryCount).append(",");
    sb.append(escapeForCsv(logEntriesDigestSha256)).append(",");

    // Trailing metadata
    sb.append(tsMs);

    return sb.toString();
  }

  /**
   * Escape string value for CSV format.
   *
   * @param str String to escape
   * @return Escaped CSV value
   */
  private String escapeForCsv(String str) {
    if (str == null || str.isEmpty()) {
      return "\"\"";
    }
    // Always quote and escape quotes
    return "\"" + str.replace("\"", "\"\"") + "\"";
  }

  /**
   * Get the CSV header row.
   *
   * @return CSV header string
   */
  public static String getCsvHeader() {
    return "run_id,exec_mode,storage_mode,block_num,block_id_hex,is_witness_signed,"
        + "block_timestamp,tx_index_in_block,tx_id_hex,owner_address_hex,contract_type,"
        + "is_constant,fee_limit,is_success,result_code,energy_used,return_data_hex,"
        + "return_data_len,runtime_error,"
        // Legacy state changes
        + "state_changes_json,state_change_count,state_digest_sha256,"
        // Domain: Account
        + "account_changes_json,account_change_count,account_digest_sha256,"
        // Domain: EVM storage
        + "evm_storage_changes_json,evm_storage_change_count,evm_storage_digest_sha256,"
        // Domain: TRC-10 balances
        + "trc10_balance_changes_json,trc10_balance_change_count,trc10_balance_digest_sha256,"
        // Domain: TRC-10 issuance
        + "trc10_issuance_changes_json,trc10_issuance_change_count,trc10_issuance_digest_sha256,"
        // Domain: Votes
        + "vote_changes_json,vote_change_count,vote_digest_sha256,"
        // Domain: Freezes
        + "freeze_changes_json,freeze_change_count,freeze_digest_sha256,"
        // Domain: Global resources
        + "global_resource_changes_json,global_resource_change_count,global_resource_digest_sha256,"
        // Domain: Account resource usage (AEXT)
        + "account_resource_usage_changes_json,account_resource_usage_change_count,"
        + "account_resource_usage_digest_sha256,"
        // Domain: Logs
        + "log_entries_json,log_entry_count,log_entries_digest_sha256,"
        // Metadata
        + "ts_ms";
  }

  /**
   * Get the number of columns in the CSV.
   */
  public static int getColumnCount() {
    // Base: 19 columns
    // Legacy state changes: 3 columns
    // Account: 3 columns
    // EVM storage: 3 columns
    // TRC-10 balance: 3 columns
    // TRC-10 issuance: 3 columns
    // Votes: 3 columns
    // Freezes: 3 columns
    // Global resources: 3 columns
    // Account resource usage: 3 columns
    // Logs: 3 columns
    // ts_ms: 1 column
    return 19 + 3 + 3 + 3 + 3 + 3 + 3 + 3 + 3 + 3 + 3 + 1; // = 50
  }

  // Getters and setters for all fields

  public String getRunId() { return runId; }
  public void setRunId(String runId) { this.runId = runId; }

  public String getExecMode() { return execMode; }
  public void setExecMode(String execMode) { this.execMode = execMode; }

  public String getStorageMode() { return storageMode; }
  public void setStorageMode(String storageMode) { this.storageMode = storageMode; }

  public long getBlockNum() { return blockNum; }
  public void setBlockNum(long blockNum) { this.blockNum = blockNum; }

  public String getBlockIdHex() { return blockIdHex; }
  public void setBlockIdHex(String blockIdHex) { this.blockIdHex = blockIdHex; }

  public boolean isWitnessSigned() { return isWitnessSigned; }
  public void setWitnessSigned(boolean witnessSigned) { isWitnessSigned = witnessSigned; }

  public long getBlockTimestamp() { return blockTimestamp; }
  public void setBlockTimestamp(long blockTimestamp) { this.blockTimestamp = blockTimestamp; }

  public int getTxIndexInBlock() { return txIndexInBlock; }
  public void setTxIndexInBlock(int txIndexInBlock) { this.txIndexInBlock = txIndexInBlock; }

  public String getTxIdHex() { return txIdHex; }
  public void setTxIdHex(String txIdHex) { this.txIdHex = txIdHex; }

  public String getOwnerAddressHex() { return ownerAddressHex; }
  public void setOwnerAddressHex(String ownerAddressHex) { this.ownerAddressHex = ownerAddressHex; }

  public String getContractType() { return contractType; }
  public void setContractType(String contractType) { this.contractType = contractType; }

  public boolean isConstant() { return isConstant; }
  public void setConstant(boolean constant) { isConstant = constant; }

  public long getFeeLimit() { return feeLimit; }
  public void setFeeLimit(long feeLimit) { this.feeLimit = feeLimit; }

  public boolean isSuccess() { return isSuccess; }
  public void setSuccess(boolean success) { isSuccess = success; }

  public String getResultCode() { return resultCode; }
  public void setResultCode(String resultCode) { this.resultCode = resultCode; }

  public long getEnergyUsed() { return energyUsed; }
  public void setEnergyUsed(long energyUsed) { this.energyUsed = energyUsed; }

  public String getReturnDataHex() { return returnDataHex; }
  public void setReturnDataHex(String returnDataHex) { this.returnDataHex = returnDataHex; }

  public int getReturnDataLen() { return returnDataLen; }
  public void setReturnDataLen(int returnDataLen) { this.returnDataLen = returnDataLen; }

  public String getRuntimeError() { return runtimeError; }
  public void setRuntimeError(String runtimeError) { this.runtimeError = runtimeError; }

  public int getStateChangeCount() { return stateChangeCount; }
  public void setStateChangeCount(int stateChangeCount) { this.stateChangeCount = stateChangeCount; }

  public String getStateChangesJson() { return stateChangesJson; }
  public void setStateChangesJson(String stateChangesJson) { this.stateChangesJson = stateChangesJson; }

  public String getStateDigestSha256() { return stateDigestSha256; }
  public void setStateDigestSha256(String stateDigestSha256) { this.stateDigestSha256 = stateDigestSha256; }

  // Domain: Account
  public String getAccountChangesJson() { return accountChangesJson; }
  public void setAccountChangesJson(String accountChangesJson) { this.accountChangesJson = accountChangesJson; }

  public int getAccountChangeCount() { return accountChangeCount; }
  public void setAccountChangeCount(int accountChangeCount) { this.accountChangeCount = accountChangeCount; }

  public String getAccountDigestSha256() { return accountDigestSha256; }
  public void setAccountDigestSha256(String accountDigestSha256) { this.accountDigestSha256 = accountDigestSha256; }

  // Domain: EVM storage
  public String getEvmStorageChangesJson() { return evmStorageChangesJson; }
  public void setEvmStorageChangesJson(String evmStorageChangesJson) { this.evmStorageChangesJson = evmStorageChangesJson; }

  public int getEvmStorageChangeCount() { return evmStorageChangeCount; }
  public void setEvmStorageChangeCount(int evmStorageChangeCount) { this.evmStorageChangeCount = evmStorageChangeCount; }

  public String getEvmStorageDigestSha256() { return evmStorageDigestSha256; }
  public void setEvmStorageDigestSha256(String evmStorageDigestSha256) { this.evmStorageDigestSha256 = evmStorageDigestSha256; }

  // Domain: TRC-10 balance
  public String getTrc10BalanceChangesJson() { return trc10BalanceChangesJson; }
  public void setTrc10BalanceChangesJson(String trc10BalanceChangesJson) { this.trc10BalanceChangesJson = trc10BalanceChangesJson; }

  public int getTrc10BalanceChangeCount() { return trc10BalanceChangeCount; }
  public void setTrc10BalanceChangeCount(int trc10BalanceChangeCount) { this.trc10BalanceChangeCount = trc10BalanceChangeCount; }

  public String getTrc10BalanceDigestSha256() { return trc10BalanceDigestSha256; }
  public void setTrc10BalanceDigestSha256(String trc10BalanceDigestSha256) { this.trc10BalanceDigestSha256 = trc10BalanceDigestSha256; }

  // Domain: TRC-10 issuance
  public String getTrc10IssuanceChangesJson() { return trc10IssuanceChangesJson; }
  public void setTrc10IssuanceChangesJson(String trc10IssuanceChangesJson) { this.trc10IssuanceChangesJson = trc10IssuanceChangesJson; }

  public int getTrc10IssuanceChangeCount() { return trc10IssuanceChangeCount; }
  public void setTrc10IssuanceChangeCount(int trc10IssuanceChangeCount) { this.trc10IssuanceChangeCount = trc10IssuanceChangeCount; }

  public String getTrc10IssuanceDigestSha256() { return trc10IssuanceDigestSha256; }
  public void setTrc10IssuanceDigestSha256(String trc10IssuanceDigestSha256) { this.trc10IssuanceDigestSha256 = trc10IssuanceDigestSha256; }

  // Domain: Votes
  public String getVoteChangesJson() { return voteChangesJson; }
  public void setVoteChangesJson(String voteChangesJson) { this.voteChangesJson = voteChangesJson; }

  public int getVoteChangeCount() { return voteChangeCount; }
  public void setVoteChangeCount(int voteChangeCount) { this.voteChangeCount = voteChangeCount; }

  public String getVoteDigestSha256() { return voteDigestSha256; }
  public void setVoteDigestSha256(String voteDigestSha256) { this.voteDigestSha256 = voteDigestSha256; }

  // Domain: Freezes
  public String getFreezeChangesJson() { return freezeChangesJson; }
  public void setFreezeChangesJson(String freezeChangesJson) { this.freezeChangesJson = freezeChangesJson; }

  public int getFreezeChangeCount() { return freezeChangeCount; }
  public void setFreezeChangeCount(int freezeChangeCount) { this.freezeChangeCount = freezeChangeCount; }

  public String getFreezeDigestSha256() { return freezeDigestSha256; }
  public void setFreezeDigestSha256(String freezeDigestSha256) { this.freezeDigestSha256 = freezeDigestSha256; }

  // Domain: Global resources
  public String getGlobalResourceChangesJson() { return globalResourceChangesJson; }
  public void setGlobalResourceChangesJson(String globalResourceChangesJson) { this.globalResourceChangesJson = globalResourceChangesJson; }

  public int getGlobalResourceChangeCount() { return globalResourceChangeCount; }
  public void setGlobalResourceChangeCount(int globalResourceChangeCount) { this.globalResourceChangeCount = globalResourceChangeCount; }

  public String getGlobalResourceDigestSha256() { return globalResourceDigestSha256; }
  public void setGlobalResourceDigestSha256(String globalResourceDigestSha256) { this.globalResourceDigestSha256 = globalResourceDigestSha256; }

  // Domain: Account resource usage (AEXT)
  public String getAccountResourceUsageChangesJson() { return accountResourceUsageChangesJson; }
  public void setAccountResourceUsageChangesJson(String accountResourceUsageChangesJson) { this.accountResourceUsageChangesJson = accountResourceUsageChangesJson; }

  public int getAccountResourceUsageChangeCount() { return accountResourceUsageChangeCount; }
  public void setAccountResourceUsageChangeCount(int accountResourceUsageChangeCount) { this.accountResourceUsageChangeCount = accountResourceUsageChangeCount; }

  public String getAccountResourceUsageDigestSha256() { return accountResourceUsageDigestSha256; }
  public void setAccountResourceUsageDigestSha256(String accountResourceUsageDigestSha256) { this.accountResourceUsageDigestSha256 = accountResourceUsageDigestSha256; }

  // Domain: Log entries
  public String getLogEntriesJson() { return logEntriesJson; }
  public void setLogEntriesJson(String logEntriesJson) { this.logEntriesJson = logEntriesJson; }

  public int getLogEntryCount() { return logEntryCount; }
  public void setLogEntryCount(int logEntryCount) { this.logEntryCount = logEntryCount; }

  public String getLogEntriesDigestSha256() { return logEntriesDigestSha256; }
  public void setLogEntriesDigestSha256(String logEntriesDigestSha256) { this.logEntriesDigestSha256 = logEntriesDigestSha256; }

  public long getTsMs() { return tsMs; }
  public void setTsMs(long tsMs) { this.tsMs = tsMs; }
}
