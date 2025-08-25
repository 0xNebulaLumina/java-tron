package org.tron.core.execution.spi;

import java.util.Arrays;
import java.util.List;
import org.junit.After;
import org.junit.Assert;
import org.junit.Before;
import org.junit.Test;

/**
 * Test class for StateDigestJni. Note: These tests require the native library to be built and
 * available. If the native library is not available, tests will be skipped.
 */
public class StateDigestJniTest {

  private StateDigestJni stateDigest;
  private boolean nativeLibraryAvailable;

  @Before
  public void setUp() {
    try {
      stateDigest = new StateDigestJni();
      nativeLibraryAvailable = true;
    } catch (Exception e) {
      System.out.println("Native library not available, skipping tests: " + e.getMessage());
      nativeLibraryAvailable = false;
    }
  }

  @After
  public void tearDown() {
    if (stateDigest != null) {
      stateDigest.destroy();
    }
  }

  @Test
  public void testStateDigestCreation() {
    if (!nativeLibraryAvailable) {
      return; // Skip test
    }

    Assert.assertNotNull(stateDigest);
    Assert.assertTrue(stateDigest.isValid());
    Assert.assertEquals(0, stateDigest.getAccountCount());
  }

  @Test
  public void testAddAccount() {
    if (!nativeLibraryAvailable) {
      return; // Skip test
    }

    byte[] address = new byte[20];
    address[19] = 1; // Address: 0x...01

    byte[] balance = new byte[32];
    balance[31] = 100; // Balance: 100

    byte[] codeHash = new byte[32];
    codeHash[31] = 42; // Code hash: 0x...2a

    stateDigest.addAccount(address, balance, 5, codeHash);

    Assert.assertEquals(1, stateDigest.getAccountCount());
  }

  @Test
  public void testAddAccountWithStorage() {
    if (!nativeLibraryAvailable) {
      return; // Skip test
    }

    byte[] address = new byte[20];
    address[19] = 1;

    byte[] balance = new byte[32];
    balance[31] = 100;

    byte[] codeHash = new byte[32];
    codeHash[31] = 42;

    // Storage slots
    List<byte[]> keys = Arrays.asList(new byte[] {1, 2, 3}, new byte[] {4, 5, 6});
    List<byte[]> values = Arrays.asList(new byte[] {7, 8, 9}, new byte[] {10, 11, 12});

    stateDigest.addAccount(address, balance, 5, codeHash, keys, values);

    Assert.assertEquals(1, stateDigest.getAccountCount());
  }

  @Test
  public void testComputeHash() {
    if (!nativeLibraryAvailable) {
      return; // Skip test
    }

    byte[] address = new byte[20];
    address[19] = 1;

    byte[] balance = new byte[32];
    balance[31] = 100;

    byte[] codeHash = new byte[32];
    codeHash[31] = 42;

    stateDigest.addAccount(address, balance, 5, codeHash);

    byte[] hash = stateDigest.computeHash();
    Assert.assertNotNull(hash);
    Assert.assertEquals(32, hash.length); // Keccak256 produces 32 bytes
  }

  @Test
  public void testComputeHashHex() {
    if (!nativeLibraryAvailable) {
      return; // Skip test
    }

    byte[] address = new byte[20];
    address[19] = 1;

    byte[] balance = new byte[32];
    balance[31] = 100;

    byte[] codeHash = new byte[32];
    codeHash[31] = 42;

    stateDigest.addAccount(address, balance, 5, codeHash);

    String hashHex = stateDigest.computeHashHex();
    Assert.assertNotNull(hashHex);
    Assert.assertEquals(64, hashHex.length()); // 32 bytes = 64 hex characters
    Assert.assertTrue(hashHex.matches("[0-9a-f]+"));
  }

  @Test
  public void testHashDeterministic() {
    if (!nativeLibraryAvailable) {
      return; // Skip test
    }

    // Create two identical state digests
    StateDigestJni digest1 = new StateDigestJni();
    StateDigestJni digest2 = new StateDigestJni();

    try {
      byte[] address = new byte[20];
      address[19] = 1;

      byte[] balance = new byte[32];
      balance[31] = 100;

      byte[] codeHash = new byte[32];
      codeHash[31] = 42;

      // Add same account to both digests
      digest1.addAccount(address, balance, 5, codeHash);
      digest2.addAccount(address, balance, 5, codeHash);

      // Hashes should be identical
      String hash1 = digest1.computeHashHex();
      String hash2 = digest2.computeHashHex();

      Assert.assertEquals(hash1, hash2);
    } finally {
      digest1.destroy();
      digest2.destroy();
    }
  }

  @Test
  public void testClear() {
    if (!nativeLibraryAvailable) {
      return; // Skip test
    }

    byte[] address = new byte[20];
    byte[] balance = new byte[32];
    byte[] codeHash = new byte[32];

    stateDigest.addAccount(address, balance, 0, codeHash);
    Assert.assertEquals(1, stateDigest.getAccountCount());

    stateDigest.clear();
    Assert.assertEquals(0, stateDigest.getAccountCount());
  }

  @Test
  public void testMultipleAccounts() {
    if (!nativeLibraryAvailable) {
      return; // Skip test
    }

    // Add multiple accounts
    for (int i = 0; i < 5; i++) {
      byte[] address = new byte[20];
      address[19] = (byte) i;

      byte[] balance = new byte[32];
      balance[31] = (byte) (i * 10);

      byte[] codeHash = new byte[32];
      codeHash[31] = (byte) (i * 2);

      stateDigest.addAccount(address, balance, i, codeHash);
    }

    Assert.assertEquals(5, stateDigest.getAccountCount());

    // Hash should be deterministic
    String hash1 = stateDigest.computeHashHex();
    String hash2 = stateDigest.computeHashHex();
    Assert.assertEquals(hash1, hash2);
  }

  @Test
  public void testInvalidArguments() {
    if (!nativeLibraryAvailable) {
      return; // Skip test
    }

    try {
      stateDigest.addAccount(null, new byte[32], 0, new byte[32]);
      Assert.fail("Should have thrown IllegalArgumentException");
    } catch (IllegalArgumentException e) {
      Assert.assertTrue(e.getMessage().contains("cannot be null"));
    }

    try {
      stateDigest.addAccount(new byte[20], null, 0, new byte[32]);
      Assert.fail("Should have thrown IllegalArgumentException");
    } catch (IllegalArgumentException e) {
      Assert.assertTrue(e.getMessage().contains("cannot be null"));
    }

    try {
      stateDigest.addAccount(new byte[20], new byte[32], 0, null);
      Assert.fail("Should have thrown IllegalArgumentException");
    } catch (IllegalArgumentException e) {
      Assert.assertTrue(e.getMessage().contains("cannot be null"));
    }
  }

  @Test
  public void testStorageMismatch() {
    if (!nativeLibraryAvailable) {
      return; // Skip test
    }

    byte[] address = new byte[20];
    byte[] balance = new byte[32];
    byte[] codeHash = new byte[32];

    List<byte[]> keys = Arrays.asList(new byte[] {1}, new byte[] {2});
    List<byte[]> values = Arrays.asList(new byte[] {3}); // Mismatched length

    try {
      stateDigest.addAccount(address, balance, 0, codeHash, keys, values);
      Assert.fail("Should have thrown IllegalArgumentException");
    } catch (IllegalArgumentException e) {
      Assert.assertTrue(e.getMessage().contains("same length"));
    }
  }

  @Test
  public void testDestroyedInstance() {
    if (!nativeLibraryAvailable) {
      return; // Skip test
    }

    stateDigest.destroy();
    Assert.assertFalse(stateDigest.isValid());

    try {
      stateDigest.addAccount(new byte[20], new byte[32], 0, new byte[32]);
      Assert.fail("Should have thrown IllegalStateException");
    } catch (IllegalStateException e) {
      Assert.assertTrue(e.getMessage().contains("destroyed"));
    }

    try {
      stateDigest.computeHash();
      Assert.fail("Should have thrown IllegalStateException");
    } catch (IllegalStateException e) {
      Assert.assertTrue(e.getMessage().contains("destroyed"));
    }
  }
}
