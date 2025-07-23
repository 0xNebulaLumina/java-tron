package org.tron.core.execution.spi;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.List;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * JNI wrapper for the Rust StateDigest utility.
 *
 * <p>This class provides Java bindings for the StateDigest functionality implemented in Rust. It's
 * used for shadow execution verification to create deterministic hashes of modified accounts.
 */
public class StateDigestJni {
  private static final Logger logger = LoggerFactory.getLogger(StateDigestJni.class);

  private static boolean libraryLoaded = false;
  private static final Object loadLock = new Object();

  // Native library name (without platform-specific prefix/suffix)
  private static final String LIBRARY_NAME = "state_digest_jni";

  // Handle to the native StateDigest instance
  private long nativeHandle;

  static {
    loadNativeLibrary();
  }

  /** Load the native library. */
  private static void loadNativeLibrary() {
    synchronized (loadLock) {
      if (libraryLoaded) {
        return;
      }

      try {
        // Try to load from system library path first
        System.loadLibrary(LIBRARY_NAME);
        logger.info("Loaded StateDigest native library from system path");
        libraryLoaded = true;
        return;
      } catch (UnsatisfiedLinkError e) {
        logger.debug("Could not load StateDigest library from system path: {}", e.getMessage());
      }

      // Try to load from resources
      try {
        loadLibraryFromResources();
        logger.info("Loaded StateDigest native library from resources");
        libraryLoaded = true;
      } catch (Exception e) {
        logger.error("Failed to load StateDigest native library", e);
        throw new RuntimeException("Could not load StateDigest native library", e);
      }
    }
  }

  /** Load the native library from JAR resources. */
  private static void loadLibraryFromResources() throws IOException {
    String osName = System.getProperty("os.name").toLowerCase();
    String osArch = System.getProperty("os.arch").toLowerCase();

    // Normalize OS name
    String normalizedOsName;
    if (osName.contains("windows")) {
      normalizedOsName = "windows";
    } else if (osName.contains("mac")) {
      normalizedOsName = "mac";
    } else {
      normalizedOsName = "linux";
    }

    // Normalize architecture name to match build script output
    String normalizedArch;
    if (osArch.equals("amd64") || osArch.equals("x86_64")) {
      normalizedArch = "x86_64";
    } else if (osArch.equals("aarch64") || osArch.equals("arm64")) {
      normalizedArch = "aarch64";
    } else {
      normalizedArch = osArch;
    }

    String libraryFileName;
    if (normalizedOsName.equals("windows")) {
      libraryFileName = LIBRARY_NAME + ".dll";
    } else if (normalizedOsName.equals("mac")) {
      libraryFileName = "lib" + LIBRARY_NAME + ".dylib";
    } else {
      libraryFileName = "lib" + LIBRARY_NAME + ".so";
    }

    String resourcePath =
        "/native/" + normalizedOsName + "/" + normalizedArch + "/" + libraryFileName;

    try (InputStream inputStream = StateDigestJni.class.getResourceAsStream(resourcePath)) {
      if (inputStream == null) {
        throw new IOException("Native library not found in resources: " + resourcePath);
      }

      // Create temporary file
      Path tempFile =
          Files.createTempFile("state_digest_jni", osName.contains("windows") ? ".dll" : ".so");
      tempFile.toFile().deleteOnExit();

      // Copy library to temporary file
      Files.copy(inputStream, tempFile, StandardCopyOption.REPLACE_EXISTING);

      // Load the library
      System.load(tempFile.toAbsolutePath().toString());
    }
  }

  /** Create a new StateDigest instance. */
  public StateDigestJni() {
    this.nativeHandle = createStateDigest();
    if (this.nativeHandle == 0) {
      throw new RuntimeException("Failed to create native StateDigest instance");
    }
  }

  /**
   * Add an account to the state digest.
   *
   * @param address Account address (20 bytes)
   * @param balance Account balance (32 bytes, big-endian)
   * @param nonce Account nonce
   * @param codeHash Account code hash (32 bytes)
   * @param storageKeys List of storage keys
   * @param storageValues List of storage values (must match keys length)
   */
  public void addAccount(
      byte[] address,
      byte[] balance,
      long nonce,
      byte[] codeHash,
      List<byte[]> storageKeys,
      List<byte[]> storageValues) {
    if (nativeHandle == 0) {
      throw new IllegalStateException("StateDigest instance has been destroyed");
    }

    if (address == null || balance == null || codeHash == null) {
      throw new IllegalArgumentException("Address, balance, and codeHash cannot be null");
    }

    if (storageKeys != null
        && storageValues != null
        && storageKeys.size() != storageValues.size()) {
      throw new IllegalArgumentException("Storage keys and values must have the same length");
    }

    // Convert lists to arrays for JNI
    byte[][] keyArray = storageKeys != null ? storageKeys.toArray(new byte[0][]) : new byte[0][];
    byte[][] valueArray =
        storageValues != null ? storageValues.toArray(new byte[0][]) : new byte[0][];

    addAccount(nativeHandle, address, balance, nonce, codeHash, keyArray, valueArray);
  }

  /**
   * Add an account with empty storage.
   *
   * @param address Account address (20 bytes)
   * @param balance Account balance (32 bytes, big-endian)
   * @param nonce Account nonce
   * @param codeHash Account code hash (32 bytes)
   */
  public void addAccount(byte[] address, byte[] balance, long nonce, byte[] codeHash) {
    addAccount(address, balance, nonce, codeHash, null, null);
  }

  /**
   * Compute the deterministic hash of all modified accounts.
   *
   * @return 32-byte Keccak256 hash
   */
  public byte[] computeHash() {
    if (nativeHandle == 0) {
      throw new IllegalStateException("StateDigest instance has been destroyed");
    }
    return computeHash(nativeHandle);
  }

  /**
   * Compute the deterministic hash as hex string.
   *
   * @return 64-character hex string
   */
  public String computeHashHex() {
    if (nativeHandle == 0) {
      throw new IllegalStateException("StateDigest instance has been destroyed");
    }
    return computeHashHex(nativeHandle);
  }

  /**
   * Get the number of accounts in the digest.
   *
   * @return Number of accounts
   */
  public long getAccountCount() {
    if (nativeHandle == 0) {
      throw new IllegalStateException("StateDigest instance has been destroyed");
    }
    return getAccountCount(nativeHandle);
  }

  /** Clear all accounts from the digest. */
  public void clear() {
    if (nativeHandle == 0) {
      throw new IllegalStateException("StateDigest instance has been destroyed");
    }
    clear(nativeHandle);
  }

  /**
   * Check if the StateDigest instance is valid.
   *
   * @return true if valid, false if destroyed
   */
  public boolean isValid() {
    return nativeHandle != 0;
  }

  /**
   * Destroy the native StateDigest instance and free memory. This method is automatically called by
   * the finalizer, but can be called explicitly for immediate cleanup.
   */
  public void destroy() {
    if (nativeHandle != 0) {
      destroyStateDigest(nativeHandle);
      nativeHandle = 0;
    }
  }

  @Override
  protected void finalize() throws Throwable {
    try {
      destroy();
    } finally {
      super.finalize();
    }
  }

  // Native method declarations

  /**
   * Create a new native StateDigest instance.
   *
   * @return Handle to the native instance
   */
  private static native long createStateDigest();

  /**
   * Destroy a native StateDigest instance.
   *
   * @param handle Handle to the native instance
   */
  private static native void destroyStateDigest(long handle);

  /**
   * Add an account to the native StateDigest.
   *
   * @param handle Handle to the native instance
   * @param address Account address
   * @param balance Account balance
   * @param nonce Account nonce
   * @param codeHash Account code hash
   * @param storageKeys Storage keys array
   * @param storageValues Storage values array
   */
  private static native void addAccount(
      long handle,
      byte[] address,
      byte[] balance,
      long nonce,
      byte[] codeHash,
      byte[][] storageKeys,
      byte[][] storageValues);

  /**
   * Compute hash from native StateDigest.
   *
   * @param handle Handle to the native instance
   * @return Hash bytes
   */
  private static native byte[] computeHash(long handle);

  /**
   * Compute hash hex from native StateDigest.
   *
   * @param handle Handle to the native instance
   * @return Hash hex string
   */
  private static native String computeHashHex(long handle);

  /**
   * Get account count from native StateDigest.
   *
   * @param handle Handle to the native instance
   * @return Number of accounts
   */
  private static native long getAccountCount(long handle);

  /**
   * Clear accounts from native StateDigest.
   *
   * @param handle Handle to the native instance
   */
  private static native void clear(long handle);
}
