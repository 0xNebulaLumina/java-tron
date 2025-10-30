package org.tron.core.execution.reporting;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.assertNotNull;

import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.tron.core.capsule.AccountCapsule;
import org.tron.protos.Protocol.Account;
import com.google.protobuf.ByteString;

/**
 * Unit tests for AccountInfoCodec.
 * Verifies serialization correctness, AEXT tail handling, and configuration properties.
 */
public class AccountInfoCodecTest {

  @Before
  public void setUp() {
    // Set default configuration for tests
    System.setProperty(AccountInfoCodec.PROPERTY_INCLUDE_AEXT, "true");
  }

  @After
  public void tearDown() {
    // Clean up system properties
    System.clearProperty(AccountInfoCodec.PROPERTY_INCLUDE_AEXT);
  }

  @Test
  public void testSerializeEmptyAccount() {
    AccountCapsule account = createTestAccount(0L, 0L);
    byte[] serialized = AccountInfoCodec.serialize(account);

    assertNotNull("Serialized data should not be null", serialized);
    // Base size: 32 (balance) + 8 (nonce) + 32 (code_hash) + 4 (code_len) = 76
    // With AEXT tail (default): 76 + 76 = 152
    assertTrue("Serialized size should be at least base size",
        serialized.length >= 76);

    // Check balance field (first 32 bytes should be zeros for balance=0)
    byte[] balanceBytes = new byte[32];
    System.arraycopy(serialized, 0, balanceBytes, 0, 32);
    for (byte b : balanceBytes) {
      assertEquals("Balance bytes should be zero", 0, b);
    }
  }

  @Test
  public void testSerializeAccountWithBalance() {
    long testBalance = 1000000000L; // 1000 TRX in SUN
    AccountCapsule account = createTestAccount(testBalance, System.currentTimeMillis());
    byte[] serialized = AccountInfoCodec.serialize(account);

    assertNotNull("Serialized data should not be null", serialized);

    // Extract balance (first 32 bytes, big-endian, in last 8 bytes)
    long extractedBalance = extractBalanceFromSerialized(serialized);
    assertEquals("Balance should match", testBalance, extractedBalance);
  }

  @Test
  public void testBalanceEncodingBoundaries() {
    // Test various balance values
    long[] testBalances = {
        0L,
        1L,
        255L,
        256L,
        65535L,
        65536L,
        1000000000L,
        Long.MAX_VALUE
    };

    for (long testBalance : testBalances) {
      AccountCapsule account = createTestAccount(testBalance, 0L);
      byte[] serialized = AccountInfoCodec.serialize(account);

      long extractedBalance = extractBalanceFromSerialized(serialized);
      assertEquals("Balance should match for value: " + testBalance,
          testBalance, extractedBalance);
    }
  }

  @Test
  public void testSerializeWithAextEnabled() {
    System.setProperty(AccountInfoCodec.PROPERTY_INCLUDE_AEXT, "true");

    AccountCapsule account = createTestAccount(1000L, 0L);
    byte[] serialized = AccountInfoCodec.serialize(account);

    // Base size: 76 bytes, AEXT tail: 76 bytes, total: 152 bytes
    assertTrue("Should include AEXT tail when enabled",
        serialized.length >= 152);

    // Check for AEXT magic at offset 76
    if (serialized.length >= 80) {
      assertEquals("AEXT magic byte 0", 0x41, serialized[76] & 0xFF); // 'A'
      assertEquals("AEXT magic byte 1", 0x45, serialized[77] & 0xFF); // 'E'
      assertEquals("AEXT magic byte 2", 0x58, serialized[78] & 0xFF); // 'X'
      assertEquals("AEXT magic byte 3", 0x54, serialized[79] & 0xFF); // 'T'
    }
  }

  @Test
  public void testSerializeWithAextDisabled() {
    System.setProperty(AccountInfoCodec.PROPERTY_INCLUDE_AEXT, "false");

    AccountCapsule account = createTestAccount(1000L, 0L);
    byte[] serialized = AccountInfoCodec.serialize(account);

    // Base size only: 76 bytes
    assertEquals("Should not include AEXT tail when disabled", 76, serialized.length);
  }

  @Test
  public void testSerializeWithAextExplicitTrue() {
    AccountCapsule account = createTestAccount(1000L, 0L);
    byte[] serialized = AccountInfoCodec.serializeWithAext(account, true);

    assertTrue("Should include AEXT tail", serialized.length >= 152);
  }

  @Test
  public void testSerializeWithAextExplicitFalse() {
    AccountCapsule account = createTestAccount(1000L, 0L);
    byte[] serialized = AccountInfoCodec.serializeWithAext(account, false);

    assertEquals("Should not include AEXT tail", 76, serialized.length);
  }

  @Test
  public void testSerializeNullAccount() {
    byte[] serialized = AccountInfoCodec.serialize(null);
    assertNotNull("Should return non-null array for null account", serialized);
    assertEquals("Should return empty array for null account", 0, serialized.length);
  }

  @Test
  public void testNonceFieldIsZero() {
    AccountCapsule account = createTestAccount(1000L, 0L);
    byte[] serialized = AccountInfoCodec.serialize(account);

    // Nonce is at offset 32, 8 bytes
    byte[] nonceBytes = new byte[8];
    System.arraycopy(serialized, 32, nonceBytes, 0, 8);

    for (byte b : nonceBytes) {
      assertEquals("Nonce bytes should be zero (TRON doesn't use nonces)", 0, b);
    }
  }

  @Test
  public void testCodeHashForAccountWithoutCode() {
    AccountCapsule account = createTestAccount(1000L, 0L);
    byte[] serialized = AccountInfoCodec.serialize(account);

    // Code hash is at offset 40, 32 bytes
    byte[] codeHashBytes = new byte[32];
    System.arraycopy(serialized, 40, codeHashBytes, 0, 32);

    // Should be Keccak-256 of empty byte array
    byte[] expectedHash = AccountInfoCodec.keccak256(new byte[0]);
    for (int i = 0; i < 32; i++) {
      assertEquals("Code hash should match Keccak-256 of empty array at byte " + i,
          expectedHash[i], codeHashBytes[i]);
    }
  }

  @Test
  public void testCodeLengthField() {
    AccountCapsule account = createTestAccount(1000L, 0L);
    byte[] serialized = AccountInfoCodec.serialize(account);

    // Code length is at offset 72, 4 bytes, big-endian
    int codeLength = ((serialized[72] & 0xFF) << 24)
        | ((serialized[73] & 0xFF) << 16)
        | ((serialized[74] & 0xFF) << 8)
        | (serialized[75] & 0xFF);

    assertEquals("Code length should be 0 for account without code", 0, codeLength);
  }

  @Test
  public void testAccountWithContract() {
    // Create account with contract code
    byte[] contractCode = new byte[]{0x60, 0x60, 0x60, 0x40}; // Sample bytecode
    Account.Builder accountBuilder = Account.newBuilder();
    accountBuilder.setBalance(1000L);
    accountBuilder.setCreateTime(0L);
    accountBuilder.setCode(ByteString.copyFrom(contractCode));

    AccountCapsule account = new AccountCapsule(accountBuilder.build());
    byte[] serialized = AccountInfoCodec.serializeWithAext(account, false);

    // Base size + code length: 76 + 4 = 80
    assertEquals("Serialized size should include contract code", 80, serialized.length);

    // Check code length field
    int codeLength = ((serialized[72] & 0xFF) << 24)
        | ((serialized[73] & 0xFF) << 16)
        | ((serialized[74] & 0xFF) << 8)
        | (serialized[75] & 0xFF);

    assertEquals("Code length should match", contractCode.length, codeLength);

    // Check actual code bytes
    for (int i = 0; i < contractCode.length; i++) {
      assertEquals("Code byte " + i + " should match",
          contractCode[i], serialized[76 + i]);
    }
  }

  @Test
  public void testBalanceRoundTrip() {
    // Test that changing only balance results in expected delta in serialization
    long balance1 = 1000000L;
    long balance2 = 2000000L;

    AccountCapsule account1 = createTestAccount(balance1, 0L);
    AccountCapsule account2 = createTestAccount(balance2, 0L);

    byte[] serialized1 = AccountInfoCodec.serializeWithAext(account1, false);
    byte[] serialized2 = AccountInfoCodec.serializeWithAext(account2, false);

    // Extract balances
    long extracted1 = extractBalanceFromSerialized(serialized1);
    long extracted2 = extractBalanceFromSerialized(serialized2);

    assertEquals("Balance 1 should match", balance1, extracted1);
    assertEquals("Balance 2 should match", balance2, extracted2);

    // Only the first 32 bytes should differ (balance field)
    // bytes 32-76 should be identical (nonce, code_hash, code_len all same for empty accounts)
    for (int i = 32; i < 76; i++) {
      assertEquals("Non-balance bytes should match at offset " + i,
          serialized1[i], serialized2[i]);
    }
  }

  @Test
  public void testAextTailStructure() {
    AccountCapsule account = createTestAccount(1000L, 0L);
    byte[] aextTail = AccountInfoCodec.serializeAextTail(account);

    assertNotNull("AEXT tail should not be null", aextTail);
    assertEquals("AEXT tail should be 76 bytes", 76, aextTail.length);

    // Check magic
    assertEquals("AEXT magic byte 0", 0x41, aextTail[0] & 0xFF);
    assertEquals("AEXT magic byte 1", 0x45, aextTail[1] & 0xFF);
    assertEquals("AEXT magic byte 2", 0x58, aextTail[2] & 0xFF);
    assertEquals("AEXT magic byte 3", 0x54, aextTail[3] & 0xFF);

    // Check version (u16 big-endian, should be 1)
    int version = ((aextTail[4] & 0xFF) << 8) | (aextTail[5] & 0xFF);
    assertEquals("AEXT version should be 1", 1, version);

    // Check length (u16 big-endian, should be 68)
    int payloadLength = ((aextTail[6] & 0xFF) << 8) | (aextTail[7] & 0xFF);
    assertEquals("AEXT payload length should be 68", 68, payloadLength);

    // Payload starts at offset 8 and is 68 bytes
    // Total: 4 (magic) + 2 (version) + 2 (length) + 68 (payload) = 76
  }

  @Test
  public void testLongToBytes32() {
    long value = 0x123456789ABCDEFL;
    byte[] bytes = AccountInfoCodec.longToBytes32(value);

    assertEquals("Should be 32 bytes", 32, bytes.length);

    // First 24 bytes should be 0
    for (int i = 0; i < 24; i++) {
      assertEquals("Leading bytes should be zero", 0, bytes[i]);
    }

    // Last 8 bytes should contain the value in big-endian
    long extracted = 0;
    for (int i = 0; i < 8; i++) {
      extracted = (extracted << 8) | (bytes[24 + i] & 0xFF);
    }
    assertEquals("Extracted value should match", value, extracted);
  }

  @Test
  public void testLongToBytes8() {
    long value = 0x123456789ABCDEFL;
    byte[] bytes = AccountInfoCodec.longToBytes8(value);

    assertEquals("Should be 8 bytes", 8, bytes.length);

    // Extract value in big-endian
    long extracted = 0;
    for (int i = 0; i < 8; i++) {
      extracted = (extracted << 8) | (bytes[i] & 0xFF);
    }
    assertEquals("Extracted value should match", value, extracted);
  }

  @Test
  public void testKeccak256EmptyInput() {
    byte[] hash = AccountInfoCodec.keccak256(new byte[0]);
    assertNotNull("Hash should not be null", hash);
    assertEquals("Hash should be 32 bytes", 32, hash.length);

    // Keccak-256 of empty string is known:
    // c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470
    byte[] expectedHash = hexToBytes("c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470");
    for (int i = 0; i < 32; i++) {
      assertEquals("Keccak-256 hash byte " + i + " should match expected",
          expectedHash[i], hash[i]);
    }
  }

  /**
   * Helper method to create test AccountCapsule.
   */
  private AccountCapsule createTestAccount(long balance, long createTime) {
    Account.Builder accountBuilder = Account.newBuilder();
    accountBuilder.setBalance(balance);
    accountBuilder.setCreateTime(createTime);
    return new AccountCapsule(accountBuilder.build());
  }

  /**
   * Helper to extract balance from serialized account (first 32 bytes, big-endian).
   */
  private long extractBalanceFromSerialized(byte[] serialized) {
    // Balance is in first 32 bytes, big-endian, value in last 8 bytes
    long balance = 0;
    for (int i = 0; i < 8; i++) {
      balance = (balance << 8) | (serialized[24 + i] & 0xFF);
    }
    return balance;
  }

  /**
   * Helper method to convert hex string to byte array.
   */
  private byte[] hexToBytes(String hex) {
    if (hex == null || hex.isEmpty()) {
      return new byte[0];
    }
    if (hex.startsWith("0x")) {
      hex = hex.substring(2);
    }
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
