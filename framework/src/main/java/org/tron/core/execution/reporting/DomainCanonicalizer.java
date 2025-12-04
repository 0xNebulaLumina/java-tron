package org.tron.core.execution.reporting;

import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.runtime.vm.LogInfo;
import org.tron.common.utils.ByteArray;
import org.tron.core.execution.spi.ExecutionSPI.FreezeLedgerChange;
import org.tron.core.execution.spi.ExecutionSPI.GlobalResourceTotalsChange;
import org.tron.core.execution.spi.ExecutionSPI.LogEntry;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;
import org.tron.core.execution.spi.ExecutionSPI.Trc10Change;
import org.tron.core.execution.spi.ExecutionSPI.VoteChange;
import org.tron.core.execution.spi.ExecutionSPI.VoteEntry;

/**
 * Canonicalizer for domain-specific changes to produce deterministic JSON and SHA-256 digests.
 *
 * <p>This class provides deterministic ordering and serialization of domain changes
 * for each domain type: accounts, EVM storage, TRC-10 balances/issuance, votes,
 * freezes, global resources, account resource usage, and EVM logs.
 *
 * <p>Canonicalization rules:
 * <ul>
 *   <li>All hex values are lowercase, no 0x prefix
 *   <li>Numbers are decimal strings
 *   <li>Timestamps are epoch milliseconds as decimal strings
 *   <li>JSON object keys are sorted lexicographically at all depths
 *   <li>Arrays are sorted by domain-specific keys
 *   <li>Digest is SHA-256 over UTF-8 bytes of canonical JSON array string
 *   <li>Empty arrays use empty string for digest
 * </ul>
 */
public class DomainCanonicalizer {

  private static final Logger logger = LoggerFactory.getLogger(DomainCanonicalizer.class);

  /**
   * Result of canonicalization containing JSON, count, and digest.
   */
  public static class DomainResult {
    private final String json;
    private final int count;
    private final String digest;

    public DomainResult(String json, int count, String digest) {
      this.json = json;
      this.count = count;
      this.digest = digest;
    }

    public String getJson() {
      return json;
    }

    public int getCount() {
      return count;
    }

    public String getDigest() {
      return digest;
    }
  }

  /**
   * Account change delta for canonicalization.
   */
  public static class AccountDelta {
    private String addressHex;
    private String op; // create|update|delete
    private Long oldBalance;
    private Long newBalance;
    private Long oldNonce;
    private Long newNonce;
    private String oldCodeHashHex;
    private String newCodeHashHex;
    private Integer oldCodeLen;
    private Integer newCodeLen;

    public AccountDelta() {
    }

    public String getAddressHex() {
      return addressHex;
    }

    public void setAddressHex(String addressHex) {
      this.addressHex = addressHex;
    }

    public String getOp() {
      return op;
    }

    public void setOp(String op) {
      this.op = op;
    }

    public Long getOldBalance() {
      return oldBalance;
    }

    public void setOldBalance(Long oldBalance) {
      this.oldBalance = oldBalance;
    }

    public Long getNewBalance() {
      return newBalance;
    }

    public void setNewBalance(Long newBalance) {
      this.newBalance = newBalance;
    }

    public Long getOldNonce() {
      return oldNonce;
    }

    public void setOldNonce(Long oldNonce) {
      this.oldNonce = oldNonce;
    }

    public Long getNewNonce() {
      return newNonce;
    }

    public void setNewNonce(Long newNonce) {
      this.newNonce = newNonce;
    }

    public String getOldCodeHashHex() {
      return oldCodeHashHex;
    }

    public void setOldCodeHashHex(String oldCodeHashHex) {
      this.oldCodeHashHex = oldCodeHashHex;
    }

    public String getNewCodeHashHex() {
      return newCodeHashHex;
    }

    public void setNewCodeHashHex(String newCodeHashHex) {
      this.newCodeHashHex = newCodeHashHex;
    }

    public Integer getOldCodeLen() {
      return oldCodeLen;
    }

    public void setOldCodeLen(Integer oldCodeLen) {
      this.oldCodeLen = oldCodeLen;
    }

    public Integer getNewCodeLen() {
      return newCodeLen;
    }

    public void setNewCodeLen(Integer newCodeLen) {
      this.newCodeLen = newCodeLen;
    }
  }

  /**
   * EVM storage delta for canonicalization.
   */
  public static class EvmStorageDelta {
    private String contractAddressHex;
    private String slotKeyHex;
    private String op; // set|delete
    private String oldValueHex;
    private String newValueHex;

    public EvmStorageDelta() {
    }

    public String getContractAddressHex() {
      return contractAddressHex;
    }

    public void setContractAddressHex(String contractAddressHex) {
      this.contractAddressHex = contractAddressHex;
    }

    public String getSlotKeyHex() {
      return slotKeyHex;
    }

    public void setSlotKeyHex(String slotKeyHex) {
      this.slotKeyHex = slotKeyHex;
    }

    public String getOp() {
      return op;
    }

    public void setOp(String op) {
      this.op = op;
    }

    public String getOldValueHex() {
      return oldValueHex;
    }

    public void setOldValueHex(String oldValueHex) {
      this.oldValueHex = oldValueHex;
    }

    public String getNewValueHex() {
      return newValueHex;
    }

    public void setNewValueHex(String newValueHex) {
      this.newValueHex = newValueHex;
    }
  }

  /**
   * TRC-10 balance delta for canonicalization.
   */
  public static class Trc10BalanceDelta {
    private String tokenId;
    private String ownerAddressHex;
    private String op; // increase|decrease|set|delete
    private String oldBalance;
    private String newBalance;

    public Trc10BalanceDelta() {
    }

    public String getTokenId() {
      return tokenId;
    }

    public void setTokenId(String tokenId) {
      this.tokenId = tokenId;
    }

    public String getOwnerAddressHex() {
      return ownerAddressHex;
    }

    public void setOwnerAddressHex(String ownerAddressHex) {
      this.ownerAddressHex = ownerAddressHex;
    }

    public String getOp() {
      return op;
    }

    public void setOp(String op) {
      this.op = op;
    }

    public String getOldBalance() {
      return oldBalance;
    }

    public void setOldBalance(String oldBalance) {
      this.oldBalance = oldBalance;
    }

    public String getNewBalance() {
      return newBalance;
    }

    public void setNewBalance(String newBalance) {
      this.newBalance = newBalance;
    }
  }

  /**
   * TRC-10 issuance delta for canonicalization.
   */
  public static class Trc10IssuanceDelta {
    private String tokenId;
    private String field;
    private String op; // create|update|delete
    private String oldValue;
    private String newValue;

    public Trc10IssuanceDelta() {
    }

    public String getTokenId() {
      return tokenId;
    }

    public void setTokenId(String tokenId) {
      this.tokenId = tokenId;
    }

    public String getField() {
      return field;
    }

    public void setField(String field) {
      this.field = field;
    }

    public String getOp() {
      return op;
    }

    public void setOp(String op) {
      this.op = op;
    }

    public String getOldValue() {
      return oldValue;
    }

    public void setOldValue(String oldValue) {
      this.oldValue = oldValue;
    }

    public String getNewValue() {
      return newValue;
    }

    public void setNewValue(String newValue) {
      this.newValue = newValue;
    }
  }

  /**
   * Vote delta for canonicalization.
   */
  public static class VoteDelta {
    private String voterAddressHex;
    private String witnessAddressHex;
    private String op; // increase|decrease|set|delete
    private String oldVotes;
    private String newVotes;

    public VoteDelta() {
    }

    public String getVoterAddressHex() {
      return voterAddressHex;
    }

    public void setVoterAddressHex(String voterAddressHex) {
      this.voterAddressHex = voterAddressHex;
    }

    public String getWitnessAddressHex() {
      return witnessAddressHex;
    }

    public void setWitnessAddressHex(String witnessAddressHex) {
      this.witnessAddressHex = witnessAddressHex;
    }

    public String getOp() {
      return op;
    }

    public void setOp(String op) {
      this.op = op;
    }

    public String getOldVotes() {
      return oldVotes;
    }

    public void setOldVotes(String oldVotes) {
      this.oldVotes = oldVotes;
    }

    public String getNewVotes() {
      return newVotes;
    }

    public void setNewVotes(String newVotes) {
      this.newVotes = newVotes;
    }
  }

  /**
   * Freeze delta for canonicalization.
   */
  public static class FreezeDelta {
    private String ownerAddressHex;
    private String resourceType; // ENERGY|BANDWIDTH|TRON_POWER
    private String recipientAddressHex; // null if self-freeze
    private String op; // freeze|unfreeze|update
    private String oldAmountSun;
    private String newAmountSun;
    private String oldExpireTimeMs;
    private String newExpireTimeMs;

    public FreezeDelta() {
    }

    public String getOwnerAddressHex() {
      return ownerAddressHex;
    }

    public void setOwnerAddressHex(String ownerAddressHex) {
      this.ownerAddressHex = ownerAddressHex;
    }

    public String getResourceType() {
      return resourceType;
    }

    public void setResourceType(String resourceType) {
      this.resourceType = resourceType;
    }

    public String getRecipientAddressHex() {
      return recipientAddressHex;
    }

    public void setRecipientAddressHex(String recipientAddressHex) {
      this.recipientAddressHex = recipientAddressHex;
    }

    public String getOp() {
      return op;
    }

    public void setOp(String op) {
      this.op = op;
    }

    public String getOldAmountSun() {
      return oldAmountSun;
    }

    public void setOldAmountSun(String oldAmountSun) {
      this.oldAmountSun = oldAmountSun;
    }

    public String getNewAmountSun() {
      return newAmountSun;
    }

    public void setNewAmountSun(String newAmountSun) {
      this.newAmountSun = newAmountSun;
    }

    public String getOldExpireTimeMs() {
      return oldExpireTimeMs;
    }

    public void setOldExpireTimeMs(String oldExpireTimeMs) {
      this.oldExpireTimeMs = oldExpireTimeMs;
    }

    public String getNewExpireTimeMs() {
      return newExpireTimeMs;
    }

    public void setNewExpireTimeMs(String newExpireTimeMs) {
      this.newExpireTimeMs = newExpireTimeMs;
    }
  }

  /**
   * Global resource delta for canonicalization.
   */
  public static class GlobalResourceDelta {
    private String field;
    private String op; // update
    private String oldValue;
    private String newValue;

    public GlobalResourceDelta() {
    }

    public String getField() {
      return field;
    }

    public void setField(String field) {
      this.field = field;
    }

    public String getOp() {
      return op;
    }

    public void setOp(String op) {
      this.op = op;
    }

    public String getOldValue() {
      return oldValue;
    }

    public void setOldValue(String oldValue) {
      this.oldValue = oldValue;
    }

    public String getNewValue() {
      return newValue;
    }

    public void setNewValue(String newValue) {
      this.newValue = newValue;
    }
  }

  /**
   * Account resource usage (AEXT) delta for canonicalization.
   */
  public static class AccountResourceUsageDelta {
    private String addressHex;
    private String op; // update
    private Long oldNetUsage;
    private Long newNetUsage;
    private Long oldEnergyUsage;
    private Long newEnergyUsage;
    private Long oldStorageUsage;
    private Long newStorageUsage;
    private Long oldNetLimit;
    private Long newNetLimit;
    private Long oldEnergyLimit;
    private Long newEnergyLimit;

    public AccountResourceUsageDelta() {
    }

    public String getAddressHex() {
      return addressHex;
    }

    public void setAddressHex(String addressHex) {
      this.addressHex = addressHex;
    }

    public String getOp() {
      return op;
    }

    public void setOp(String op) {
      this.op = op;
    }

    public Long getOldNetUsage() {
      return oldNetUsage;
    }

    public void setOldNetUsage(Long oldNetUsage) {
      this.oldNetUsage = oldNetUsage;
    }

    public Long getNewNetUsage() {
      return newNetUsage;
    }

    public void setNewNetUsage(Long newNetUsage) {
      this.newNetUsage = newNetUsage;
    }

    public Long getOldEnergyUsage() {
      return oldEnergyUsage;
    }

    public void setOldEnergyUsage(Long oldEnergyUsage) {
      this.oldEnergyUsage = oldEnergyUsage;
    }

    public Long getNewEnergyUsage() {
      return newEnergyUsage;
    }

    public void setNewEnergyUsage(Long newEnergyUsage) {
      this.newEnergyUsage = newEnergyUsage;
    }

    public Long getOldStorageUsage() {
      return oldStorageUsage;
    }

    public void setOldStorageUsage(Long oldStorageUsage) {
      this.oldStorageUsage = oldStorageUsage;
    }

    public Long getNewStorageUsage() {
      return newStorageUsage;
    }

    public void setNewStorageUsage(Long newStorageUsage) {
      this.newStorageUsage = newStorageUsage;
    }

    public Long getOldNetLimit() {
      return oldNetLimit;
    }

    public void setOldNetLimit(Long oldNetLimit) {
      this.oldNetLimit = oldNetLimit;
    }

    public Long getNewNetLimit() {
      return newNetLimit;
    }

    public void setNewNetLimit(Long newNetLimit) {
      this.newNetLimit = newNetLimit;
    }

    public Long getOldEnergyLimit() {
      return oldEnergyLimit;
    }

    public void setOldEnergyLimit(Long oldEnergyLimit) {
      this.oldEnergyLimit = oldEnergyLimit;
    }

    public Long getNewEnergyLimit() {
      return newEnergyLimit;
    }

    public void setNewEnergyLimit(Long newEnergyLimit) {
      this.newEnergyLimit = newEnergyLimit;
    }
  }

  /**
   * Log entry delta for canonicalization.
   */
  public static class LogEntryDelta {
    private String contractAddressHex;
    private int index;
    private List<String> topicsHex;
    private String dataHex;

    public LogEntryDelta() {
      this.topicsHex = new ArrayList<>();
    }

    public String getContractAddressHex() {
      return contractAddressHex;
    }

    public void setContractAddressHex(String contractAddressHex) {
      this.contractAddressHex = contractAddressHex;
    }

    public int getIndex() {
      return index;
    }

    public void setIndex(int index) {
      this.index = index;
    }

    public List<String> getTopicsHex() {
      return topicsHex;
    }

    public void setTopicsHex(List<String> topicsHex) {
      this.topicsHex = topicsHex;
    }

    public String getDataHex() {
      return dataHex;
    }

    public void setDataHex(String dataHex) {
      this.dataHex = dataHex;
    }
  }

  // ================================
  // Account Changes
  // ================================

  /**
   * Canonicalize account changes to JSON and compute digest.
   * Sort by: address_hex
   */
  public static DomainResult accountToJsonAndDigest(List<AccountDelta> deltas) {
    if (deltas == null || deltas.isEmpty()) {
      return emptyDomainResult();
    }

    // Sort by address_hex
    List<AccountDelta> sorted = new ArrayList<>(deltas);
    sorted.sort(Comparator.comparing(d -> d.getAddressHex().toLowerCase()));

    StringBuilder sb = new StringBuilder("[");
    for (int i = 0; i < sorted.size(); i++) {
      if (i > 0) {
        sb.append(",");
      }
      sb.append(accountDeltaToJson(sorted.get(i)));
    }
    sb.append("]");

    String json = sb.toString();
    return new DomainResult(json, sorted.size(), computeDigest(json));
  }

  private static String accountDeltaToJson(AccountDelta d) {
    StringBuilder sb = new StringBuilder("{");
    List<String> fields = new ArrayList<>();

    fields.add("\"address_hex\":\"" + d.getAddressHex().toLowerCase() + "\"");

    if (d.getNewBalance() != null || d.getOldBalance() != null) {
      fields.add("\"balance_sun\":{" + oldNewJson(
          d.getOldBalance() != null ? String.valueOf(d.getOldBalance()) : null,
          d.getNewBalance() != null ? String.valueOf(d.getNewBalance()) : null) + "}");
    }

    if (d.getNewCodeHashHex() != null || d.getOldCodeHashHex() != null) {
      fields.add("\"code_hash_hex\":{" + oldNewJson(
          d.getOldCodeHashHex() != null ? d.getOldCodeHashHex().toLowerCase() : null,
          d.getNewCodeHashHex() != null ? d.getNewCodeHashHex().toLowerCase() : null) + "}");
    }

    if (d.getNewCodeLen() != null || d.getOldCodeLen() != null) {
      fields.add("\"code_len_bytes\":{" + oldNewJson(
          d.getOldCodeLen() != null ? String.valueOf(d.getOldCodeLen()) : null,
          d.getNewCodeLen() != null ? String.valueOf(d.getNewCodeLen()) : null) + "}");
    }

    if (d.getNewNonce() != null || d.getOldNonce() != null) {
      fields.add("\"nonce\":{" + oldNewJson(
          d.getOldNonce() != null ? String.valueOf(d.getOldNonce()) : null,
          d.getNewNonce() != null ? String.valueOf(d.getNewNonce()) : null) + "}");
    }

    if (d.getOp() != null) {
      fields.add("\"op\":\"" + d.getOp() + "\"");
    }

    // Sort fields by key (already sorted lexicographically by name)
    Collections.sort(fields);
    sb.append(String.join(",", fields));
    sb.append("}");
    return sb.toString();
  }

  // ================================
  // EVM Storage Changes
  // ================================

  /**
   * Canonicalize EVM storage changes to JSON and compute digest.
   * Sort by: contract_address_hex, then slot_key_hex
   */
  public static DomainResult evmStorageToJsonAndDigest(List<EvmStorageDelta> deltas) {
    if (deltas == null || deltas.isEmpty()) {
      return emptyDomainResult();
    }

    // Sort by contract_address_hex, then slot_key_hex
    List<EvmStorageDelta> sorted = new ArrayList<>(deltas);
    sorted.sort((a, b) -> {
      int cmp = a.getContractAddressHex().toLowerCase()
          .compareTo(b.getContractAddressHex().toLowerCase());
      if (cmp != 0) {
        return cmp;
      }
      return a.getSlotKeyHex().toLowerCase().compareTo(b.getSlotKeyHex().toLowerCase());
    });

    StringBuilder sb = new StringBuilder("[");
    for (int i = 0; i < sorted.size(); i++) {
      if (i > 0) {
        sb.append(",");
      }
      sb.append(evmStorageDeltaToJson(sorted.get(i)));
    }
    sb.append("]");

    String json = sb.toString();
    return new DomainResult(json, sorted.size(), computeDigest(json));
  }

  private static String evmStorageDeltaToJson(EvmStorageDelta d) {
    StringBuilder sb = new StringBuilder("{");
    List<String> fields = new ArrayList<>();

    fields.add("\"contract_address_hex\":\"" + d.getContractAddressHex().toLowerCase() + "\"");

    if (d.getNewValueHex() != null) {
      fields.add("\"new_value_hex\":\"" + d.getNewValueHex().toLowerCase() + "\"");
    }

    if (d.getOldValueHex() != null) {
      fields.add("\"old_value_hex\":\"" + d.getOldValueHex().toLowerCase() + "\"");
    }

    if (d.getOp() != null) {
      fields.add("\"op\":\"" + d.getOp() + "\"");
    }

    fields.add("\"slot_key_hex\":\"" + d.getSlotKeyHex().toLowerCase() + "\"");

    Collections.sort(fields);
    sb.append(String.join(",", fields));
    sb.append("}");
    return sb.toString();
  }

  // ================================
  // TRC-10 Balance Changes
  // ================================

  /**
   * Canonicalize TRC-10 balance changes to JSON and compute digest.
   * Sort by: token_id (string comparison), then owner_address_hex
   */
  public static DomainResult trc10BalancesToJsonAndDigest(List<Trc10BalanceDelta> deltas) {
    if (deltas == null || deltas.isEmpty()) {
      return emptyDomainResult();
    }

    // Sort by token_id, then owner_address_hex
    List<Trc10BalanceDelta> sorted = new ArrayList<>(deltas);
    sorted.sort((a, b) -> {
      int cmp = a.getTokenId().compareTo(b.getTokenId());
      if (cmp != 0) {
        return cmp;
      }
      return a.getOwnerAddressHex().toLowerCase().compareTo(b.getOwnerAddressHex().toLowerCase());
    });

    StringBuilder sb = new StringBuilder("[");
    for (int i = 0; i < sorted.size(); i++) {
      if (i > 0) {
        sb.append(",");
      }
      sb.append(trc10BalanceDeltaToJson(sorted.get(i)));
    }
    sb.append("]");

    String json = sb.toString();
    return new DomainResult(json, sorted.size(), computeDigest(json));
  }

  private static String trc10BalanceDeltaToJson(Trc10BalanceDelta d) {
    StringBuilder sb = new StringBuilder("{");
    List<String> fields = new ArrayList<>();

    if (d.getNewBalance() != null) {
      fields.add("\"new_balance\":\"" + d.getNewBalance() + "\"");
    }

    if (d.getOldBalance() != null) {
      fields.add("\"old_balance\":\"" + d.getOldBalance() + "\"");
    }

    if (d.getOp() != null) {
      fields.add("\"op\":\"" + d.getOp() + "\"");
    }

    fields.add("\"owner_address_hex\":\"" + d.getOwnerAddressHex().toLowerCase() + "\"");
    fields.add("\"token_id\":\"" + d.getTokenId() + "\"");

    Collections.sort(fields);
    sb.append(String.join(",", fields));
    sb.append("}");
    return sb.toString();
  }

  // ================================
  // TRC-10 Issuance Changes
  // ================================

  /**
   * Canonicalize TRC-10 issuance changes to JSON and compute digest.
   * Sort by: token_id, then field
   */
  public static DomainResult trc10IssuanceToJsonAndDigest(List<Trc10IssuanceDelta> deltas) {
    if (deltas == null || deltas.isEmpty()) {
      return emptyDomainResult();
    }

    // Sort by token_id, then field
    List<Trc10IssuanceDelta> sorted = new ArrayList<>(deltas);
    sorted.sort((a, b) -> {
      int cmp = a.getTokenId().compareTo(b.getTokenId());
      if (cmp != 0) {
        return cmp;
      }
      return a.getField().compareTo(b.getField());
    });

    StringBuilder sb = new StringBuilder("[");
    for (int i = 0; i < sorted.size(); i++) {
      if (i > 0) {
        sb.append(",");
      }
      sb.append(trc10IssuanceDeltaToJson(sorted.get(i)));
    }
    sb.append("]");

    String json = sb.toString();
    return new DomainResult(json, sorted.size(), computeDigest(json));
  }

  private static String trc10IssuanceDeltaToJson(Trc10IssuanceDelta d) {
    StringBuilder sb = new StringBuilder("{");
    List<String> fields = new ArrayList<>();

    fields.add("\"field\":\"" + d.getField() + "\"");

    if (d.getNewValue() != null) {
      fields.add("\"new\":\"" + escapeJsonString(d.getNewValue()) + "\"");
    }

    if (d.getOldValue() != null) {
      fields.add("\"old\":\"" + escapeJsonString(d.getOldValue()) + "\"");
    }

    if (d.getOp() != null) {
      fields.add("\"op\":\"" + d.getOp() + "\"");
    }

    fields.add("\"token_id\":\"" + d.getTokenId() + "\"");

    Collections.sort(fields);
    sb.append(String.join(",", fields));
    sb.append("}");
    return sb.toString();
  }

  // ================================
  // Vote Changes
  // ================================

  /**
   * Canonicalize vote changes to JSON and compute digest.
   * Sort by: voter_address_hex, then witness_address_hex
   */
  public static DomainResult votesToJsonAndDigest(List<VoteDelta> deltas) {
    if (deltas == null || deltas.isEmpty()) {
      return emptyDomainResult();
    }

    // Sort by voter_address_hex, then witness_address_hex
    List<VoteDelta> sorted = new ArrayList<>(deltas);
    sorted.sort((a, b) -> {
      int cmp = a.getVoterAddressHex().toLowerCase()
          .compareTo(b.getVoterAddressHex().toLowerCase());
      if (cmp != 0) {
        return cmp;
      }
      return a.getWitnessAddressHex().toLowerCase()
          .compareTo(b.getWitnessAddressHex().toLowerCase());
    });

    StringBuilder sb = new StringBuilder("[");
    for (int i = 0; i < sorted.size(); i++) {
      if (i > 0) {
        sb.append(",");
      }
      sb.append(voteDeltaToJson(sorted.get(i)));
    }
    sb.append("]");

    String json = sb.toString();
    return new DomainResult(json, sorted.size(), computeDigest(json));
  }

  private static String voteDeltaToJson(VoteDelta d) {
    StringBuilder sb = new StringBuilder("{");
    List<String> fields = new ArrayList<>();

    if (d.getNewVotes() != null) {
      fields.add("\"new_votes\":\"" + d.getNewVotes() + "\"");
    }

    if (d.getOldVotes() != null) {
      fields.add("\"old_votes\":\"" + d.getOldVotes() + "\"");
    }

    if (d.getOp() != null) {
      fields.add("\"op\":\"" + d.getOp() + "\"");
    }

    fields.add("\"voter_address_hex\":\"" + d.getVoterAddressHex().toLowerCase() + "\"");
    fields.add("\"witness_address_hex\":\"" + d.getWitnessAddressHex().toLowerCase() + "\"");

    Collections.sort(fields);
    sb.append(String.join(",", fields));
    sb.append("}");
    return sb.toString();
  }

  // ================================
  // Freeze Changes
  // ================================

  /**
   * Canonicalize freeze changes to JSON and compute digest.
   * Sort by: owner_address_hex, resource_type, then recipient_address_hex
   */
  public static DomainResult freezesToJsonAndDigest(List<FreezeDelta> deltas) {
    if (deltas == null || deltas.isEmpty()) {
      return emptyDomainResult();
    }

    // Sort by owner_address_hex, resource_type, then recipient_address_hex
    List<FreezeDelta> sorted = new ArrayList<>(deltas);
    sorted.sort((a, b) -> {
      int cmp = a.getOwnerAddressHex().toLowerCase()
          .compareTo(b.getOwnerAddressHex().toLowerCase());
      if (cmp != 0) {
        return cmp;
      }
      cmp = a.getResourceType().compareTo(b.getResourceType());
      if (cmp != 0) {
        return cmp;
      }
      String recA = a.getRecipientAddressHex() != null
          ? a.getRecipientAddressHex().toLowerCase() : "";
      String recB = b.getRecipientAddressHex() != null
          ? b.getRecipientAddressHex().toLowerCase() : "";
      return recA.compareTo(recB);
    });

    StringBuilder sb = new StringBuilder("[");
    for (int i = 0; i < sorted.size(); i++) {
      if (i > 0) {
        sb.append(",");
      }
      sb.append(freezeDeltaToJson(sorted.get(i)));
    }
    sb.append("]");

    String json = sb.toString();
    return new DomainResult(json, sorted.size(), computeDigest(json));
  }

  private static String freezeDeltaToJson(FreezeDelta d) {
    StringBuilder sb = new StringBuilder("{");
    List<String> fields = new ArrayList<>();

    if (d.getNewAmountSun() != null) {
      fields.add("\"new_amount_sun\":\"" + d.getNewAmountSun() + "\"");
    }

    if (d.getNewExpireTimeMs() != null) {
      fields.add("\"new_expire_time_ms\":\"" + d.getNewExpireTimeMs() + "\"");
    }

    if (d.getOldAmountSun() != null) {
      fields.add("\"old_amount_sun\":\"" + d.getOldAmountSun() + "\"");
    }

    if (d.getOldExpireTimeMs() != null) {
      fields.add("\"old_expire_time_ms\":\"" + d.getOldExpireTimeMs() + "\"");
    }

    if (d.getOp() != null) {
      fields.add("\"op\":\"" + d.getOp() + "\"");
    }

    fields.add("\"owner_address_hex\":\"" + d.getOwnerAddressHex().toLowerCase() + "\"");

    if (d.getRecipientAddressHex() != null && !d.getRecipientAddressHex().isEmpty()) {
      fields.add("\"recipient_address_hex\":\"" + d.getRecipientAddressHex().toLowerCase() + "\"");
    }

    fields.add("\"resource_type\":\"" + d.getResourceType() + "\"");

    Collections.sort(fields);
    sb.append(String.join(",", fields));
    sb.append("}");
    return sb.toString();
  }

  // ================================
  // Global Resource Changes
  // ================================

  /**
   * Canonicalize global resource changes to JSON and compute digest.
   * Sort by: field
   */
  public static DomainResult globalsToJsonAndDigest(List<GlobalResourceDelta> deltas) {
    if (deltas == null || deltas.isEmpty()) {
      return emptyDomainResult();
    }

    // Sort by field
    List<GlobalResourceDelta> sorted = new ArrayList<>(deltas);
    sorted.sort(Comparator.comparing(GlobalResourceDelta::getField));

    StringBuilder sb = new StringBuilder("[");
    for (int i = 0; i < sorted.size(); i++) {
      if (i > 0) {
        sb.append(",");
      }
      sb.append(globalResourceDeltaToJson(sorted.get(i)));
    }
    sb.append("]");

    String json = sb.toString();
    return new DomainResult(json, sorted.size(), computeDigest(json));
  }

  private static String globalResourceDeltaToJson(GlobalResourceDelta d) {
    StringBuilder sb = new StringBuilder("{");
    List<String> fields = new ArrayList<>();

    fields.add("\"field\":\"" + d.getField() + "\"");

    if (d.getNewValue() != null) {
      fields.add("\"new\":\"" + d.getNewValue() + "\"");
    }

    if (d.getOldValue() != null) {
      fields.add("\"old\":\"" + d.getOldValue() + "\"");
    }

    if (d.getOp() != null) {
      fields.add("\"op\":\"" + d.getOp() + "\"");
    }

    Collections.sort(fields);
    sb.append(String.join(",", fields));
    sb.append("}");
    return sb.toString();
  }

  // ================================
  // Account Resource Usage (AEXT) Changes
  // ================================

  /**
   * Canonicalize account resource usage changes to JSON and compute digest.
   * Sort by: address_hex
   */
  public static DomainResult accountAextToJsonAndDigest(List<AccountResourceUsageDelta> deltas) {
    if (deltas == null || deltas.isEmpty()) {
      return emptyDomainResult();
    }

    // Sort by address_hex
    List<AccountResourceUsageDelta> sorted = new ArrayList<>(deltas);
    sorted.sort(Comparator.comparing(d -> d.getAddressHex().toLowerCase()));

    StringBuilder sb = new StringBuilder("[");
    for (int i = 0; i < sorted.size(); i++) {
      if (i > 0) {
        sb.append(",");
      }
      sb.append(accountAextDeltaToJson(sorted.get(i)));
    }
    sb.append("]");

    String json = sb.toString();
    return new DomainResult(json, sorted.size(), computeDigest(json));
  }

  private static String accountAextDeltaToJson(AccountResourceUsageDelta d) {
    StringBuilder sb = new StringBuilder("{");
    List<String> fields = new ArrayList<>();

    fields.add("\"address_hex\":\"" + d.getAddressHex().toLowerCase() + "\"");

    if (d.getNewEnergyLimit() != null || d.getOldEnergyLimit() != null) {
      fields.add("\"energy_limit\":{" + oldNewJson(
          d.getOldEnergyLimit() != null ? String.valueOf(d.getOldEnergyLimit()) : null,
          d.getNewEnergyLimit() != null ? String.valueOf(d.getNewEnergyLimit()) : null) + "}");
    }

    if (d.getNewEnergyUsage() != null || d.getOldEnergyUsage() != null) {
      fields.add("\"energy_usage\":{" + oldNewJson(
          d.getOldEnergyUsage() != null ? String.valueOf(d.getOldEnergyUsage()) : null,
          d.getNewEnergyUsage() != null ? String.valueOf(d.getNewEnergyUsage()) : null) + "}");
    }

    if (d.getNewNetLimit() != null || d.getOldNetLimit() != null) {
      fields.add("\"net_limit\":{" + oldNewJson(
          d.getOldNetLimit() != null ? String.valueOf(d.getOldNetLimit()) : null,
          d.getNewNetLimit() != null ? String.valueOf(d.getNewNetLimit()) : null) + "}");
    }

    if (d.getNewNetUsage() != null || d.getOldNetUsage() != null) {
      fields.add("\"net_usage\":{" + oldNewJson(
          d.getOldNetUsage() != null ? String.valueOf(d.getOldNetUsage()) : null,
          d.getNewNetUsage() != null ? String.valueOf(d.getNewNetUsage()) : null) + "}");
    }

    if (d.getOp() != null) {
      fields.add("\"op\":\"" + d.getOp() + "\"");
    }

    if (d.getNewStorageUsage() != null || d.getOldStorageUsage() != null) {
      fields.add("\"storage_usage\":{" + oldNewJson(
          d.getOldStorageUsage() != null ? String.valueOf(d.getOldStorageUsage()) : null,
          d.getNewStorageUsage() != null ? String.valueOf(d.getNewStorageUsage()) : null) + "}");
    }

    Collections.sort(fields);
    sb.append(String.join(",", fields));
    sb.append("}");
    return sb.toString();
  }

  // ================================
  // Log Entries
  // ================================

  /**
   * Canonicalize log entries to JSON and compute digest.
   * Sort by: contract_address_hex, then index
   */
  public static DomainResult logsToJsonAndDigest(List<LogEntryDelta> deltas) {
    if (deltas == null || deltas.isEmpty()) {
      return emptyDomainResult();
    }

    // Sort by contract_address_hex, then index
    List<LogEntryDelta> sorted = new ArrayList<>(deltas);
    sorted.sort((a, b) -> {
      int cmp = a.getContractAddressHex().toLowerCase()
          .compareTo(b.getContractAddressHex().toLowerCase());
      if (cmp != 0) {
        return cmp;
      }
      return Integer.compare(a.getIndex(), b.getIndex());
    });

    StringBuilder sb = new StringBuilder("[");
    for (int i = 0; i < sorted.size(); i++) {
      if (i > 0) {
        sb.append(",");
      }
      sb.append(logEntryDeltaToJson(sorted.get(i)));
    }
    sb.append("]");

    String json = sb.toString();
    return new DomainResult(json, sorted.size(), computeDigest(json));
  }

  private static String logEntryDeltaToJson(LogEntryDelta d) {
    StringBuilder sb = new StringBuilder("{");
    List<String> fields = new ArrayList<>();

    fields.add("\"contract_address_hex\":\"" + d.getContractAddressHex().toLowerCase() + "\"");

    if (d.getDataHex() != null) {
      fields.add("\"data_hex\":\"" + d.getDataHex().toLowerCase() + "\"");
    }

    fields.add("\"index\":\"" + d.getIndex() + "\"");

    if (d.getTopicsHex() != null && !d.getTopicsHex().isEmpty()) {
      StringBuilder topicsSb = new StringBuilder("[");
      for (int i = 0; i < d.getTopicsHex().size(); i++) {
        if (i > 0) {
          topicsSb.append(",");
        }
        topicsSb.append("\"").append(d.getTopicsHex().get(i).toLowerCase()).append("\"");
      }
      topicsSb.append("]");
      fields.add("\"topics_hex\":" + topicsSb.toString());
    }

    Collections.sort(fields);
    sb.append(String.join(",", fields));
    sb.append("}");
    return sb.toString();
  }

  // ================================
  // Converters from ExecutionSPI types
  // ================================

  /**
   * Convert LogEntry list to LogEntryDelta list.
   */
  public static List<LogEntryDelta> convertLogEntries(List<LogEntry> logs) {
    if (logs == null || logs.isEmpty()) {
      return new ArrayList<>();
    }

    List<LogEntryDelta> deltas = new ArrayList<>();
    int index = 0;
    for (LogEntry log : logs) {
      LogEntryDelta delta = new LogEntryDelta();
      delta.setContractAddressHex(log.getAddress() != null
          ? ByteArray.toHexString(log.getAddress()) : "");
      delta.setIndex(index++);
      delta.setDataHex(log.getData() != null ? ByteArray.toHexString(log.getData()) : "");

      if (log.getTopics() != null) {
        List<String> topicsHex = new ArrayList<>();
        for (byte[] topic : log.getTopics()) {
          topicsHex.add(topic != null ? ByteArray.toHexString(topic) : "");
        }
        delta.setTopicsHex(topicsHex);
      }

      deltas.add(delta);
    }
    return deltas;
  }

  /**
   * Convert LogInfo list to LogEntryDelta list.
   */
  public static List<LogEntryDelta> convertLogInfos(List<LogInfo> logs) {
    if (logs == null || logs.isEmpty()) {
      return new ArrayList<>();
    }

    List<LogEntryDelta> deltas = new ArrayList<>();
    int index = 0;
    for (LogInfo log : logs) {
      LogEntryDelta delta = new LogEntryDelta();
      delta.setContractAddressHex(log.getAddress() != null
          ? ByteArray.toHexString(log.getAddress()) : "");
      delta.setIndex(index++);
      delta.setDataHex(log.getData() != null ? ByteArray.toHexString(log.getData()) : "");

      if (log.getTopics() != null) {
        List<String> topicsHex = new ArrayList<>();
        for (org.tron.common.runtime.vm.DataWord topic : log.getTopics()) {
          topicsHex.add(topic != null ? ByteArray.toHexString(topic.getData()) : "");
        }
        delta.setTopicsHex(topicsHex);
      }

      deltas.add(delta);
    }
    return deltas;
  }

  /**
   * Convert VoteChange list to VoteDelta list.
   * Uses old_votes = 0 for new votes, tracks per-witness changes.
   */
  public static List<VoteDelta> convertVoteChanges(List<VoteChange> voteChanges) {
    if (voteChanges == null || voteChanges.isEmpty()) {
      return new ArrayList<>();
    }

    List<VoteDelta> deltas = new ArrayList<>();
    for (VoteChange vc : voteChanges) {
      String voterHex = vc.getOwnerAddress() != null
          ? ByteArray.toHexString(vc.getOwnerAddress()) : "";

      if (vc.getVotes() != null) {
        for (VoteEntry ve : vc.getVotes()) {
          VoteDelta delta = new VoteDelta();
          delta.setVoterAddressHex(voterHex);
          delta.setWitnessAddressHex(ve.getVoteAddress() != null
              ? ByteArray.toHexString(ve.getVoteAddress()) : "");
          delta.setOp("set");
          delta.setOldVotes("0"); // Assume old is 0 for now (can be enhanced with pre-state)
          delta.setNewVotes(String.valueOf(ve.getVoteCount()));
          deltas.add(delta);
        }
      }
    }
    return deltas;
  }

  /**
   * Convert FreezeLedgerChange list to FreezeDelta list.
   */
  public static List<FreezeDelta> convertFreezeChanges(List<FreezeLedgerChange> freezeChanges) {
    if (freezeChanges == null || freezeChanges.isEmpty()) {
      return new ArrayList<>();
    }

    List<FreezeDelta> deltas = new ArrayList<>();
    for (FreezeLedgerChange fc : freezeChanges) {
      FreezeDelta delta = new FreezeDelta();
      delta.setOwnerAddressHex(fc.getOwnerAddress() != null
          ? ByteArray.toHexString(fc.getOwnerAddress()) : "");
      delta.setResourceType(fc.getResource().name());
      delta.setOp(fc.getAmount() > 0 ? "freeze" : "unfreeze");
      delta.setOldAmountSun("0");
      delta.setNewAmountSun(String.valueOf(fc.getAmount()));
      delta.setOldExpireTimeMs("0");
      delta.setNewExpireTimeMs(String.valueOf(fc.getExpirationMs()));
      deltas.add(delta);
    }
    return deltas;
  }

  /**
   * Convert GlobalResourceTotalsChange list to GlobalResourceDelta list.
   */
  public static List<GlobalResourceDelta> convertGlobalResourceChanges(
      List<GlobalResourceTotalsChange> globalChanges) {
    if (globalChanges == null || globalChanges.isEmpty()) {
      return new ArrayList<>();
    }

    List<GlobalResourceDelta> deltas = new ArrayList<>();
    for (GlobalResourceTotalsChange gc : globalChanges) {
      // Each GlobalResourceTotalsChange contains multiple fields
      // We emit separate delta entries for each field

      GlobalResourceDelta netWeight = new GlobalResourceDelta();
      netWeight.setField("total_net_weight");
      netWeight.setOp("update");
      netWeight.setOldValue("0"); // Placeholder - needs pre-state
      netWeight.setNewValue(String.valueOf(gc.getTotalNetWeight()));
      deltas.add(netWeight);

      GlobalResourceDelta netLimit = new GlobalResourceDelta();
      netLimit.setField("total_net_limit");
      netLimit.setOp("update");
      netLimit.setOldValue("0");
      netLimit.setNewValue(String.valueOf(gc.getTotalNetLimit()));
      deltas.add(netLimit);

      GlobalResourceDelta energyWeight = new GlobalResourceDelta();
      energyWeight.setField("total_energy_weight");
      energyWeight.setOp("update");
      energyWeight.setOldValue("0");
      energyWeight.setNewValue(String.valueOf(gc.getTotalEnergyWeight()));
      deltas.add(energyWeight);

      GlobalResourceDelta energyLimit = new GlobalResourceDelta();
      energyLimit.setField("total_energy_limit");
      energyLimit.setOp("update");
      energyLimit.setOldValue("0");
      energyLimit.setNewValue(String.valueOf(gc.getTotalEnergyLimit()));
      deltas.add(energyLimit);
    }
    return deltas;
  }

  /**
   * Split state changes into account changes and EVM storage changes.
   * Account changes have empty key; EVM storage changes have non-empty key.
   */
  public static SplitStateChanges splitStateChanges(List<StateChange> stateChanges) {
    SplitStateChanges result = new SplitStateChanges();
    if (stateChanges == null || stateChanges.isEmpty()) {
      return result;
    }

    for (StateChange sc : stateChanges) {
      if (sc.getKey() == null || sc.getKey().length == 0) {
        // Account change
        AccountDelta delta = parseAccountChange(sc);
        if (delta != null) {
          result.accountChanges.add(delta);
        }
      } else {
        // EVM storage change
        EvmStorageDelta delta = new EvmStorageDelta();
        delta.setContractAddressHex(sc.getAddress() != null
            ? ByteArray.toHexString(sc.getAddress()) : "");
        delta.setSlotKeyHex(ByteArray.toHexString(sc.getKey()));
        delta.setOldValueHex(sc.getOldValue() != null
            ? ByteArray.toHexString(sc.getOldValue()) : "");
        delta.setNewValueHex(sc.getNewValue() != null
            ? ByteArray.toHexString(sc.getNewValue()) : "");
        delta.setOp(delta.getNewValueHex().isEmpty() || isAllZeros(sc.getNewValue())
            ? "delete" : "set");
        result.evmStorageChanges.add(delta);
      }
    }

    return result;
  }

  /**
   * Result of splitting state changes into account and EVM storage.
   */
  public static class SplitStateChanges {
    public List<AccountDelta> accountChanges = new ArrayList<>();
    public List<EvmStorageDelta> evmStorageChanges = new ArrayList<>();
  }

  /**
   * Parse account change from StateChange.
   * Format: [balance(32)][nonce(8)][codeHash(32)][codeLen(4)][code...]
   */
  private static AccountDelta parseAccountChange(StateChange sc) {
    AccountDelta delta = new AccountDelta();
    delta.setAddressHex(sc.getAddress() != null
        ? ByteArray.toHexString(sc.getAddress()) : "");

    // Determine operation
    boolean hasOld = sc.getOldValue() != null && sc.getOldValue().length > 0;
    boolean hasNew = sc.getNewValue() != null && sc.getNewValue().length > 0;

    if (!hasOld && hasNew) {
      delta.setOp("create");
    } else if (hasOld && !hasNew) {
      delta.setOp("delete");
    } else {
      delta.setOp("update");
    }

    // Parse old value if present
    if (hasOld) {
      parseAccountInfoInto(sc.getOldValue(), delta, true);
    }

    // Parse new value if present
    if (hasNew) {
      parseAccountInfoInto(sc.getNewValue(), delta, false);
    }

    return delta;
  }

  /**
   * Parse account info bytes into delta.
   * Format: [balance(32)][nonce(8)][codeHash(32)][codeLen(4)][code...]
   */
  private static void parseAccountInfoInto(byte[] data, AccountDelta delta, boolean isOld) {
    if (data == null || data.length < 76) {
      return; // Minimum: 32 + 8 + 32 + 4 = 76 bytes
    }

    try {
      // Balance: first 32 bytes (BigInteger)
      byte[] balanceBytes = new byte[32];
      System.arraycopy(data, 0, balanceBytes, 0, 32);
      java.math.BigInteger balance = new java.math.BigInteger(1, balanceBytes);

      // Nonce: next 8 bytes
      long nonce = 0;
      for (int i = 0; i < 8; i++) {
        nonce = (nonce << 8) | (data[32 + i] & 0xFF);
      }

      // Code hash: next 32 bytes
      byte[] codeHash = new byte[32];
      System.arraycopy(data, 40, codeHash, 0, 32);

      // Code length: next 4 bytes
      int codeLen = 0;
      for (int i = 0; i < 4; i++) {
        codeLen = (codeLen << 8) | (data[72 + i] & 0xFF);
      }

      if (isOld) {
        delta.setOldBalance(balance.longValue());
        delta.setOldNonce(nonce);
        delta.setOldCodeHashHex(ByteArray.toHexString(codeHash));
        delta.setOldCodeLen(codeLen);
      } else {
        delta.setNewBalance(balance.longValue());
        delta.setNewNonce(nonce);
        delta.setNewCodeHashHex(ByteArray.toHexString(codeHash));
        delta.setNewCodeLen(codeLen);
      }
    } catch (Exception e) {
      logger.warn("Failed to parse account info: {}", e.getMessage());
    }
  }

  /**
   * Parsed AEXT (Account EXTension) data structure.
   */
  public static class ParsedAext {
    public long netUsage;
    public long freeNetUsage;
    public long energyUsage;
    public long latestConsumeTime;
    public long latestConsumeFreeTime;
    public long latestConsumeTimeForEnergy;
    public long netWindowSize;
    public long energyWindowSize;
    public boolean netWindowOptimized;
    public boolean energyWindowOptimized;

    public boolean isEmpty() {
      return netUsage == 0 && freeNetUsage == 0 && energyUsage == 0
          && netWindowSize == 0 && energyWindowSize == 0;
    }
  }

  /**
   * Parse AEXT from account bytes if present.
   * AEXT format: magic("AEXT") + version(2) + length(2) + payload(68)
   * Returns null if no AEXT found or parsing fails.
   */
  public static ParsedAext parseAext(byte[] data) {
    if (data == null) {
      return null;
    }

    // Minimum account bytes: 76 (base) + 0 (code) = 76
    // AEXT header starts after: 76 + codeLen
    if (data.length < 76) {
      return null;
    }

    try {
      // Get code length from offset 72 (4 bytes big-endian)
      int codeLen = 0;
      for (int i = 0; i < 4; i++) {
        codeLen = (codeLen << 8) | (data[72 + i] & 0xFF);
      }

      // Calculate AEXT start offset
      int aextStart = 76 + codeLen;

      // Need at least 8 bytes for AEXT header and 68 bytes for payload
      if (data.length < aextStart + 8 + 68) {
        return null;
      }

      // Check AEXT magic: "AEXT" (0x41 0x45 0x58 0x54)
      if (data[aextStart] != 0x41 || data[aextStart + 1] != 0x45
          || data[aextStart + 2] != 0x58 || data[aextStart + 3] != 0x54) {
        return null;
      }

      // Check version (expecting 1)
      int version = ((data[aextStart + 4] & 0xFF) << 8) | (data[aextStart + 5] & 0xFF);
      if (version != 1) {
        logger.warn("Unsupported AEXT version: {}", version);
        return null;
      }

      // Check length (expecting 68)
      int payloadLen = ((data[aextStart + 6] & 0xFF) << 8) | (data[aextStart + 7] & 0xFF);
      if (payloadLen != 68) {
        logger.warn("Unexpected AEXT payload length: {}", payloadLen);
        return null;
      }

      // Parse payload (starting at aextStart + 8)
      int offset = aextStart + 8;
      ParsedAext aext = new ParsedAext();

      aext.netUsage = readI64BigEndian(data, offset);
      offset += 8;
      aext.freeNetUsage = readI64BigEndian(data, offset);
      offset += 8;
      aext.energyUsage = readI64BigEndian(data, offset);
      offset += 8;
      aext.latestConsumeTime = readI64BigEndian(data, offset);
      offset += 8;
      aext.latestConsumeFreeTime = readI64BigEndian(data, offset);
      offset += 8;
      aext.latestConsumeTimeForEnergy = readI64BigEndian(data, offset);
      offset += 8;
      aext.netWindowSize = readI64BigEndian(data, offset);
      offset += 8;
      aext.energyWindowSize = readI64BigEndian(data, offset);
      offset += 8;
      aext.netWindowOptimized = data[offset++] != 0;
      aext.energyWindowOptimized = data[offset] != 0;

      return aext;
    } catch (Exception e) {
      logger.debug("Failed to parse AEXT: {}", e.getMessage());
      return null;
    }
  }

  /**
   * Read i64 value in big-endian format from byte array.
   */
  private static long readI64BigEndian(byte[] data, int offset) {
    long value = 0;
    for (int i = 0; i < 8; i++) {
      value = (value << 8) | (data[offset + i] & 0xFF);
    }
    return value;
  }

  /**
   * Extract AccountResourceUsageDelta list from state changes.
   * Parses AEXT from account bytes (empty key state changes).
   */
  public static List<AccountResourceUsageDelta> extractAccountResourceUsage(
      List<StateChange> stateChanges) {

    List<AccountResourceUsageDelta> deltas = new ArrayList<>();
    if (stateChanges == null || stateChanges.isEmpty()) {
      return deltas;
    }

    for (StateChange sc : stateChanges) {
      // Only process account changes (empty key)
      if (sc.getKey() != null && sc.getKey().length > 0) {
        continue;
      }

      ParsedAext oldAext = parseAext(sc.getOldValue());
      ParsedAext newAext = parseAext(sc.getNewValue());

      // Skip if no AEXT data on either side
      if (oldAext == null && newAext == null) {
        continue;
      }

      // Create delta if there's any change
      AccountResourceUsageDelta delta = new AccountResourceUsageDelta();
      delta.setAddressHex(sc.getAddress() != null
          ? ByteArray.toHexString(sc.getAddress()).toLowerCase() : "");
      delta.setOp("update");

      // Set old values (default to 0 if no old AEXT)
      long oldNetUsage = oldAext != null ? oldAext.netUsage : 0;
      long oldEnergyUsage = oldAext != null ? oldAext.energyUsage : 0;
      long oldNetWindowSize = oldAext != null ? oldAext.netWindowSize : 0;
      long oldEnergyWindowSize = oldAext != null ? oldAext.energyWindowSize : 0;

      // Set new values (default to 0 if no new AEXT)
      long newNetUsage = newAext != null ? newAext.netUsage : 0;
      long newEnergyUsage = newAext != null ? newAext.energyUsage : 0;
      long newNetWindowSize = newAext != null ? newAext.netWindowSize : 0;
      long newEnergyWindowSize = newAext != null ? newAext.energyWindowSize : 0;

      // Only include if something actually changed
      boolean hasChange = oldNetUsage != newNetUsage
          || oldEnergyUsage != newEnergyUsage
          || oldNetWindowSize != newNetWindowSize
          || oldEnergyWindowSize != newEnergyWindowSize;

      if (!hasChange) {
        continue;
      }

      // Set values only if they changed
      if (oldNetUsage != newNetUsage || oldAext != null || newAext != null) {
        delta.setOldNetUsage(oldNetUsage);
        delta.setNewNetUsage(newNetUsage);
      }
      if (oldEnergyUsage != newEnergyUsage || oldAext != null || newAext != null) {
        delta.setOldEnergyUsage(oldEnergyUsage);
        delta.setNewEnergyUsage(newEnergyUsage);
      }
      // Using window size as approximate storage usage for now
      if (oldNetWindowSize != newNetWindowSize || oldEnergyWindowSize != newEnergyWindowSize) {
        delta.setOldStorageUsage(oldNetWindowSize);
        delta.setNewStorageUsage(newNetWindowSize);
      }
      // Net/energy limits are not in AEXT; set null to omit from JSON
      delta.setOldNetLimit(null);
      delta.setNewNetLimit(null);
      delta.setOldEnergyLimit(null);
      delta.setNewEnergyLimit(null);

      deltas.add(delta);
    }

    return deltas;
  }

  // ================================
  // Helper Methods
  // ================================

  /**
   * Create empty domain result.
   */
  public static DomainResult emptyDomainResult() {
    return new DomainResult("[]", 0, "");
  }

  /**
   * Compute SHA-256 digest of string.
   */
  private static String computeDigest(String data) {
    if (data == null || data.isEmpty() || data.equals("[]")) {
      return "";
    }

    try {
      MessageDigest digest = MessageDigest.getInstance("SHA-256");
      byte[] hashBytes = digest.digest(data.getBytes(StandardCharsets.UTF_8));
      return ByteArray.toHexString(hashBytes).toLowerCase();
    } catch (NoSuchAlgorithmException e) {
      logger.error("SHA-256 algorithm not available", e);
      return "";
    }
  }

  /**
   * Build old/new JSON object content.
   */
  private static String oldNewJson(String oldVal, String newVal) {
    List<String> parts = new ArrayList<>();
    if (newVal != null) {
      parts.add("\"new\":\"" + newVal + "\"");
    }
    if (oldVal != null) {
      parts.add("\"old\":\"" + oldVal + "\"");
    }
    Collections.sort(parts);
    return String.join(",", parts);
  }

  /**
   * Escape special characters in JSON string.
   */
  private static String escapeJsonString(String s) {
    if (s == null) {
      return "";
    }
    StringBuilder sb = new StringBuilder();
    for (char c : s.toCharArray()) {
      switch (c) {
        case '"':
          sb.append("\\\"");
          break;
        case '\\':
          sb.append("\\\\");
          break;
        case '\b':
          sb.append("\\b");
          break;
        case '\f':
          sb.append("\\f");
          break;
        case '\n':
          sb.append("\\n");
          break;
        case '\r':
          sb.append("\\r");
          break;
        case '\t':
          sb.append("\\t");
          break;
        default:
          if (c < ' ') {
            sb.append(String.format("\\u%04x", (int) c));
          } else {
            sb.append(c);
          }
      }
    }
    return sb.toString();
  }

  /**
   * Check if byte array is all zeros.
   */
  private static boolean isAllZeros(byte[] data) {
    if (data == null) {
      return true;
    }
    for (byte b : data) {
      if (b != 0) {
        return false;
      }
    }
    return true;
  }
}
