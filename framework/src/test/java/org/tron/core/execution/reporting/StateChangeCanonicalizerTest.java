package org.tron.core.execution.reporting;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotEquals;
import static org.junit.Assert.assertTrue;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import org.junit.Test;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;

/**
 * Unit tests for StateChangeCanonicalizer.
 */
public class StateChangeCanonicalizerTest {

  @Test
  public void testEmptyStateDigest() {
    String digest1 = StateChangeCanonicalizer.computeEmptyStateDigest();
    String digest2 = StateChangeCanonicalizer.computeStateDigest(null);
    String digest3 = StateChangeCanonicalizer.computeStateDigest(new ArrayList<>());
    
    // All should produce the same digest
    assertEquals(digest1, digest2);
    assertEquals(digest1, digest3);
    
    // Should be valid SHA-256 digest format
    assertTrue(StateChangeCanonicalizer.isValidStateDigest(digest1));
    assertEquals(64, digest1.length());
  }
  
  @Test
  public void testSingleStateChangeDigest() {
    StateChange change = createStateChange(
        "0x1234567890abcdef",
        "0xabcdef1234567890", 
        "0x01de",
        "0x0eda"
    );
    
    List<StateChange> changes = Arrays.asList(change);
    String digest = StateChangeCanonicalizer.computeStateDigest(changes);
    
    assertTrue(StateChangeCanonicalizer.isValidStateDigest(digest));
    assertNotEquals(StateChangeCanonicalizer.computeEmptyStateDigest(), digest);
  }
  
  @Test
  public void testDeterministicOrdering() {
    StateChange change1 = createStateChange("0xaaa", "0x111", "0x01de", "0x0eda");
    StateChange change2 = createStateChange("0xbbb", "0x222", "0x02df", "0x0edb");
    
    // Same changes in different order should produce same digest
    List<StateChange> order1 = Arrays.asList(change1, change2);
    List<StateChange> order2 = Arrays.asList(change2, change1);
    
    String digest1 = StateChangeCanonicalizer.computeStateDigest(order1);
    String digest2 = StateChangeCanonicalizer.computeStateDigest(order2);
    
    assertEquals("Digest should be deterministic regardless of order", digest1, digest2);
  }
  
  @Test
  public void testCanonicalJsonCreation() {
    StateChange change = createStateChange(
        "0x1234567890abcdef",
        "0xabcdef1234567890", 
        "0x01de",
        "0x0eda"
    );
    
    List<StateChange> changes = Arrays.asList(change);
    String json = StateChangeCanonicalizer.createCanonicalJson(changes);
    
    // Should be valid JSON array format
    assertTrue("JSON should start with [", json.startsWith("["));
    assertTrue("JSON should end with ]", json.endsWith("]"));
    assertTrue("JSON should contain address field", json.contains("\"address\""));
    assertTrue("JSON should contain key field", json.contains("\"key\""));
    assertTrue("JSON should contain oldValue field", json.contains("\"oldValue\""));
    assertTrue("JSON should contain newValue field", json.contains("\"newValue\""));
  }
  
  @Test
  public void testEmptyJsonCreation() {
    String json1 = StateChangeCanonicalizer.createCanonicalJson(null);
    String json2 = StateChangeCanonicalizer.createCanonicalJson(new ArrayList<>());
    
    assertEquals("[]", json1);
    assertEquals("[]", json2);
  }
  
  @Test
  public void testDigestValidation() {
    // Valid SHA-256 digest (64 hex chars)
    assertTrue(StateChangeCanonicalizer.isValidStateDigest(
        "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"));
    
    // Invalid cases
    assertFalse("Null should be invalid", 
        StateChangeCanonicalizer.isValidStateDigest(null));
    assertFalse("Too short should be invalid", 
        StateChangeCanonicalizer.isValidStateDigest("abcdef"));
    assertFalse("Too long should be invalid", 
        StateChangeCanonicalizer.isValidStateDigest(
            "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcd"));
    assertFalse("Uppercase should be invalid", 
        StateChangeCanonicalizer.isValidStateDigest(
            "ABCDEF1234567890ABCDEF1234567890ABCDEF1234567890ABCDEF1234567890"));
    assertFalse("Non-hex characters should be invalid", 
        StateChangeCanonicalizer.isValidStateDigest(
            "ghijkl1234567890abcdef1234567890abcdef1234567890abcdef1234567890"));
  }
  
  @Test
  public void testDigestEquality() {
    String digest1 = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
    String digest2 = "ABCDEF1234567890ABCDEF1234567890ABCDEF1234567890ABCDEF1234567890";
    String digest3 = "fedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321";
    
    // Case-insensitive equality
    assertTrue(StateChangeCanonicalizer.digestsEqual(digest1, digest2));
    assertFalse(StateChangeCanonicalizer.digestsEqual(digest1, digest3));
    
    // Null handling
    assertTrue(StateChangeCanonicalizer.digestsEqual(null, null));
    assertFalse(StateChangeCanonicalizer.digestsEqual(digest1, null));
    assertFalse(StateChangeCanonicalizer.digestsEqual(null, digest1));
  }
  
  /**
   * Helper method to create a StateChange for testing.
   */
  private StateChange createStateChange(String address, String key, String oldValue, String newValue) {
    return new StateChange(
        hexToBytes(address),
        hexToBytes(key),
        hexToBytes(oldValue),
        hexToBytes(newValue)
    );
  }
  
  /**
   * Helper method to convert hex string to byte array.
   */
  private byte[] hexToBytes(String hex) {
    if (hex == null || hex.isEmpty()) {
      return new byte[0];
    }
    // Remove 0x prefix if present
    if (hex.startsWith("0x")) {
      hex = hex.substring(2);
    }
    // Ensure even length
    if (hex.length() % 2 != 0) {
      hex = "0" + hex;
    }
    
    byte[] bytes = new byte[hex.length() / 2];
    for (int i = 0; i < bytes.length; i++) {
      bytes[i] = (byte) Integer.parseInt(hex.substring(2 * i, 2 * i + 2), 16);
    }
    return bytes;
  }
}