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
 * <p>Schema fields match the specification in CLAUDE.md and planning.md.
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
  
  // State changes
  private int stateChangeCount;
  private String stateChangesJson;
  private String stateDigestSha256;
  
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
          record.stateDigestSha256 = org.tron.core.execution.reporting.StateChangeCanonicalizer.computeStateDigest(stateChanges);
        }
      } else {
        record.stateChangeCount = 0;
        record.stateChangesJson = "";
        // Auto-compute empty digest if not already set
        if (record.stateDigestSha256 == null || record.stateDigestSha256.isEmpty()) {
          record.stateDigestSha256 = org.tron.core.execution.reporting.StateChangeCanonicalizer.computeEmptyStateDigest();
        }
      }
      return this;
    }
    
    public Builder stateDigestSha256(String stateDigestSha256) {
      record.stateDigestSha256 = stateDigestSha256 != null ? stateDigestSha256 : "";
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
    sb.append(stateChangeCount).append(",");
    sb.append(escapeForCsv(stateChangesJson)).append(",");
    sb.append(escapeForCsv(stateDigestSha256)).append(",");
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
    return "run_id,exec_mode,storage_mode,block_num,block_id_hex,is_witness_signed," +
           "block_timestamp,tx_index_in_block,tx_id_hex,owner_address_hex,contract_type," +
           "is_constant,fee_limit,is_success,result_code,energy_used,return_data_hex," +
           "return_data_len,runtime_error,state_change_count,state_changes_json," +
           "state_digest_sha256,ts_ms";
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
  
  public long getTsMs() { return tsMs; }
  public void setTsMs(long tsMs) { this.tsMs = tsMs; }
}