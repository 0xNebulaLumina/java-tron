package org.tron.core.execution.reporting;

import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.utils.ByteArray;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;

/**
 * Canonicalizer for state changes to produce deterministic SHA-256 digests.
 * 
 * <p>This class provides deterministic ordering and serialization of state changes
 * to enable consistent digest computation across different execution modes.
 * 
 * <p>Canonicalization rules:
 * <ul>
 *   <li>Convert all byte arrays to lowercase hex strings
 *   <li>Build tuples as: address|key|oldValue|newValue
 *   <li>Sort tuples lexicographically
 *   <li>Join with newlines and compute SHA-256
 *   <li>Return digest as lowercase hex string
 * </ul>
 */
public class StateChangeCanonicalizer {
  
  private static final Logger logger = LoggerFactory.getLogger(StateChangeCanonicalizer.class);
  
  /**
   * Compute SHA-256 digest of canonical state changes.
   * 
   * @param stateChanges List of state changes to canonicalize
   * @return SHA-256 digest as lowercase hex string
   */
  public static String computeStateDigest(List<StateChange> stateChanges) {
    if (stateChanges == null || stateChanges.isEmpty()) {
      return computeEmptyStateDigest();
    }
    
    try {
      List<String> canonicalTuples = buildCanonicalTuples(stateChanges);
      String canonicalString = String.join("\n", canonicalTuples);
      
      MessageDigest digest = MessageDigest.getInstance("SHA-256");
      byte[] hashBytes = digest.digest(canonicalString.getBytes(StandardCharsets.UTF_8));
      
      return ByteArray.toHexString(hashBytes).toLowerCase();
    } catch (NoSuchAlgorithmException e) {
      logger.error("SHA-256 algorithm not available", e);
      return ""; // Fallback to empty string
    }
  }
  
  /**
   * Compute SHA-256 digest for empty state changes list.
   * 
   * @return SHA-256 digest of empty string as lowercase hex
   */
  public static String computeEmptyStateDigest() {
    try {
      MessageDigest digest = MessageDigest.getInstance("SHA-256");
      byte[] hashBytes = digest.digest("".getBytes(StandardCharsets.UTF_8));
      return ByteArray.toHexString(hashBytes).toLowerCase();
    } catch (NoSuchAlgorithmException e) {
      logger.error("SHA-256 algorithm not available", e);
      return "";
    }
  }
  
  /**
   * Build canonical tuples from state changes.
   * 
   * @param stateChanges List of state changes
   * @return Sorted list of canonical tuple strings
   */
  private static List<String> buildCanonicalTuples(List<StateChange> stateChanges) {
    List<String> tuples = new ArrayList<>();
    
    for (StateChange change : stateChanges) {
      String tuple = buildCanonicalTuple(change);
      tuples.add(tuple);
    }
    
    // Sort lexicographically for deterministic ordering
    Collections.sort(tuples);
    
    return tuples;
  }
  
  /**
   * Build canonical tuple string for a single state change.
   * 
   * @param change State change to canonicalize
   * @return Canonical tuple string: address|key|oldValue|newValue
   */
  private static String buildCanonicalTuple(StateChange change) {
    String address = change.getAddress() != null 
        ? ByteArray.toHexString(change.getAddress()).toLowerCase() 
        : "";
    String key = change.getKey() != null 
        ? ByteArray.toHexString(change.getKey()).toLowerCase() 
        : "";
    String oldValue = change.getOldValue() != null 
        ? ByteArray.toHexString(change.getOldValue()).toLowerCase() 
        : "";
    String newValue = change.getNewValue() != null 
        ? ByteArray.toHexString(change.getNewValue()).toLowerCase() 
        : "";
    
    return address + "|" + key + "|" + oldValue + "|" + newValue;
  }
  
  /**
   * Validate state digest format.
   * 
   * @param digest Digest string to validate
   * @return true if valid SHA-256 hex string (64 lowercase hex chars)
   */
  public static boolean isValidStateDigest(String digest) {
    if (digest == null) {
      return false;
    }
    // SHA-256 produces 32 bytes = 64 hex characters
    if (digest.length() != 64) {
      return false;
    }
    // Check if all characters are lowercase hex
    return digest.matches("^[0-9a-f]{64}$");
  }
  
  /**
   * Create canonical JSON representation for CSV storage.
   * This is separate from digest computation and focuses on readability.
   * 
   * @param stateChanges List of state changes
   * @return JSON array string with hex-encoded values
   */
  public static String createCanonicalJson(List<StateChange> stateChanges) {
    if (stateChanges == null || stateChanges.isEmpty()) {
      return "[]";
    }
    
    StringBuilder sb = new StringBuilder("[");
    
    for (int i = 0; i < stateChanges.size(); i++) {
      if (i > 0) {
        sb.append(",");
      }
      
      StateChange change = stateChanges.get(i);
      sb.append("{");
      sb.append("\"address\":\"").append(toHexOrEmpty(change.getAddress())).append("\",");
      sb.append("\"key\":\"").append(toHexOrEmpty(change.getKey())).append("\",");
      sb.append("\"oldValue\":\"").append(toHexOrEmpty(change.getOldValue())).append("\",");
      sb.append("\"newValue\":\"").append(toHexOrEmpty(change.getNewValue())).append("\"");
      sb.append("}");
    }
    
    sb.append("]");
    return sb.toString();
  }
  
  /**
   * Helper to convert byte array to hex or empty string.
   * 
   * @param bytes Byte array to convert
   * @return Hex string or empty string if null
   */
  private static String toHexOrEmpty(byte[] bytes) {
    return bytes != null ? ByteArray.toHexString(bytes) : "";
  }
  
  /**
   * Compare two state digests for equality.
   * 
   * @param digest1 First digest
   * @param digest2 Second digest
   * @return true if digests are equal (case-insensitive)
   */
  public static boolean digestsEqual(String digest1, String digest2) {
    if (digest1 == null && digest2 == null) {
      return true;
    }
    if (digest1 == null || digest2 == null) {
      return false;
    }
    return digest1.toLowerCase().equals(digest2.toLowerCase());
  }
}