package org.tron.core.storage.spi;

import java.util.Map;
import java.util.concurrent.CompletableFuture;

/** Iterator interface for storage operations. */
public interface StorageIterator extends AutoCloseable {
  CompletableFuture<Boolean> hasNext();

  CompletableFuture<Map.Entry<byte[], byte[]>> next();

  CompletableFuture<Void> seek(byte[] key);

  CompletableFuture<Void> seekToFirst();

  CompletableFuture<Void> seekToLast();

  @Override
  void close();
}
