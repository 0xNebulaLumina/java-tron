package org.tron.core.db2;

import com.google.common.collect.Sets;
import java.io.File;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Set;
import java.util.stream.Collectors;
import lombok.AllArgsConstructor;
import lombok.EqualsAndHashCode;
import lombok.NoArgsConstructor;
import org.junit.After;
import org.junit.Assert;
import org.junit.Before;
import org.junit.Test;
import org.springframework.util.CollectionUtils;
import org.tron.common.application.Application;
import org.tron.common.application.ApplicationFactory;
import org.tron.common.application.TronApplicationContext;
import org.tron.common.cache.CacheStrategies;
import org.tron.common.utils.FileUtil;
import org.tron.common.utils.SessionOptional;
import org.tron.core.Constant;
import org.tron.core.capsule.ProtoCapsule;
import org.tron.core.config.DefaultConfig;
import org.tron.core.config.args.Args;
import org.tron.core.db2.common.DB;
import org.tron.core.db2.common.Flusher;
import org.tron.core.db2.common.WrappedByteArray;
import org.tron.core.db2.RevokingDbWithCacheNewValueTest.TestRevokingTronStore;
import org.tron.core.db2.core.Chainbase;
import org.tron.core.db2.core.Snapshot;
import org.tron.core.db2.core.SnapshotManager;
import org.tron.core.db2.core.SnapshotRoot;
import org.tron.core.exception.ItemNotFoundException;

public class SnapshotRootTest {

  private TestRevokingTronStore tronDatabase;
  private TronApplicationContext context;
  private Application appT;
  private SnapshotManager revokingDatabase;
  private final Set<String> noSecondCacheDBs = Sets.newHashSet(Arrays.asList("trans-cache",
          "exchange-v2","nullifier","accountTrie","transactionRetStore","accountid-index",
          "market_account","market_pair_to_price","recent-transaction","block-index","block",
          "market_pair_price_to_order","proposal","tree-block-index","IncrementalMerkleTree",
          "asset-issue","balance-trace","transactionHistoryStore","account-index","section-bloom",
          "exchange","market_order","account-trace","contract-state","trans"));
  private Set<String> allDBNames;
  private Set<String> allRevokingDBNames;


  @Before
  public void init() {
    Args.setParam(new String[]{"-d", "output_revokingStore_test"}, Constant.TEST_CONF);
    context = new TronApplicationContext(DefaultConfig.class);
    appT = ApplicationFactory.create(context);
  }

  @After
  public void removeDb() {
    Args.clearParam();
    context.destroy();
    FileUtil.deleteDir(new File("output_revokingStore_test"));
  }

  @Test
  public synchronized void testRemove() {
    ProtoCapsuleTest testProtoCapsule = new ProtoCapsuleTest("test".getBytes());
    tronDatabase = new TestRevokingTronStore("testSnapshotRoot-testRemove");
    tronDatabase.put("test".getBytes(), testProtoCapsule);
    Assert.assertEquals(testProtoCapsule, tronDatabase.get("test".getBytes()));

    tronDatabase.delete("test".getBytes());
    Assert.assertEquals(null, tronDatabase.get("test".getBytes()));
    tronDatabase.close();
  }

  @Test
  public synchronized void testMerge() {
    tronDatabase = new TestRevokingTronStore("testSnapshotRoot-testMerge");
    revokingDatabase = context.getBean(SnapshotManager.class);
    revokingDatabase.enable();
    revokingDatabase.add(tronDatabase.getRevokingDB());

    SessionOptional dialog = SessionOptional.instance().setValue(revokingDatabase.buildSession());
    ProtoCapsuleTest testProtoCapsule = new ProtoCapsuleTest("merge".getBytes());
    tronDatabase.put(testProtoCapsule.getData(), testProtoCapsule);
    revokingDatabase.getDbs().forEach(db -> db.getHead().getRoot().merge(db.getHead()));
    dialog.reset();
    Assert.assertEquals(tronDatabase.get(testProtoCapsule.getData()), testProtoCapsule);

    tronDatabase.close();
  }

  @Test
  public synchronized void testMergeList() {
    tronDatabase = new TestRevokingTronStore("testSnapshotRoot-testMergeList");
    revokingDatabase = context.getBean(SnapshotManager.class);
    revokingDatabase.enable();
    revokingDatabase.add(tronDatabase.getRevokingDB());

    SessionOptional.instance().setValue(revokingDatabase.buildSession());
    ProtoCapsuleTest testProtoCapsule = new ProtoCapsuleTest("test".getBytes());
    tronDatabase.put("merge".getBytes(), testProtoCapsule);
    for (int i = 1; i < 11; i++) {
      ProtoCapsuleTest tmpProtoCapsule = new ProtoCapsuleTest(("mergeList" + i).getBytes());
      try (ISession tmpSession = revokingDatabase.buildSession()) {
        tronDatabase.put(tmpProtoCapsule.getData(), tmpProtoCapsule);
        tmpSession.commit();
      }
    }
    revokingDatabase.getDbs().forEach(db -> {
      List<Snapshot> snapshots = new ArrayList<>();
      SnapshotRoot root = (SnapshotRoot) db.getHead().getRoot();
      Snapshot next = root;
      for (int i = 0; i < 11; ++i) {
        next = next.getNext();
        snapshots.add(next);
      }
      root.merge(snapshots);
      root.resetSolidity();

      for (int i = 1; i < 11; i++) {
        ProtoCapsuleTest tmpProtoCapsule = new ProtoCapsuleTest(("mergeList" + i).getBytes());
        Assert.assertEquals(tmpProtoCapsule, tronDatabase.get(tmpProtoCapsule.getData()));
      }

    });
    revokingDatabase.updateSolidity(10);
    tronDatabase.close();
  }

  @Test
  public void testMergeWithHeadDeleteInvalidatesCache() {
    // "votes" DB is configured for second-level caching (CacheStrategies.CACHE_DBS),
    // and REMOTE mode uses head-based merge. This test ensures deletes invalidate the cache.
    InMemoryFlusherDb db = new InMemoryFlusherDb("votes");
    SnapshotRoot root = new SnapshotRoot(db);
    Chainbase chainbase = new Chainbase(root);

    byte[] key = "test-key".getBytes();
    byte[] value = "test-value".getBytes();

    // 1) Write via a snapshot, then mergeWithHead to populate root+cache.
    Snapshot snapshot1 = chainbase.getHead().advance();
    chainbase.setHead(snapshot1);
    chainbase.put(key, value);
    root.mergeWithHead(snapshot1, Arrays.asList(snapshot1));
    chainbase.setHead(root);
    Assert.assertArrayEquals(value, chainbase.getUnchecked(key));

    // 2) Delete via a snapshot, mergeWithHead, and ensure cache is invalidated.
    Snapshot snapshot2 = chainbase.getHead().advance();
    chainbase.setHead(snapshot2);
    chainbase.delete(key);
    root.mergeWithHead(snapshot2, Arrays.asList(snapshot2));
    chainbase.setHead(root);

    Assert.assertNull(chainbase.getUnchecked(key));
    Assert.assertFalse(chainbase.has(key));

    chainbase.close();
  }

  @Test
  public void testSecondCacheCheck()
      throws ItemNotFoundException {
    revokingDatabase = context.getBean(SnapshotManager.class);
    allRevokingDBNames = parseRevokingDBNames(context);
    allDBNames = Arrays.stream(new File("output_revokingStore_test/database").list())
            .collect(Collectors.toSet());
    if (CollectionUtils.isEmpty(allDBNames)) {
      throw new ItemNotFoundException("No DBs found");
    }
    allDBNames.removeAll(noSecondCacheDBs);
    allDBNames.removeAll(CacheStrategies.CACHE_DBS);
    allDBNames.retainAll(allRevokingDBNames);
    org.junit.Assert.assertEquals(String.format("New added dbs %s "
                    + "shall consider to add second cache or add to noNeedCheckDBs!",
        allDBNames.stream().collect(Collectors.joining(","))), allDBNames.size(), 0);
  }

  @Test
  public void testSecondCacheCheckAddDb()
          throws ItemNotFoundException {
    revokingDatabase = context.getBean(SnapshotManager.class);
    allRevokingDBNames = parseRevokingDBNames(context);
    allRevokingDBNames.add("secondCheckTestDB");
    FileUtil.createDirIfNotExists("output_revokingStore_test/database/secondCheckTestDB");
    allDBNames = Arrays.stream(new File("output_revokingStore_test/database").list())
            .collect(Collectors.toSet());
    FileUtil.deleteDir(new File("output_revokingStore_test/database/secondCheckTestDB"));
    if (CollectionUtils.isEmpty(allDBNames)) {
      throw new ItemNotFoundException("No DBs found");
    }
    allDBNames.removeAll(noSecondCacheDBs);
    allDBNames.removeAll(CacheStrategies.CACHE_DBS);
    allDBNames.retainAll(allRevokingDBNames);
    org.junit.Assert.assertTrue(String.format("New added dbs %s "
                    + "check second cache failed!",
            allDBNames.stream().collect(Collectors.joining(","))), allDBNames.size() == 1);
  }

  private Set<String> parseRevokingDBNames(TronApplicationContext context) {
    SnapshotManager snapshotManager = context.getBean(SnapshotManager.class);
    return snapshotManager.getDbs().stream().map(chainbase ->
        chainbase.getDbName()).collect(Collectors.toSet());
  }


  @NoArgsConstructor
  @AllArgsConstructor
  @EqualsAndHashCode
  public static class ProtoCapsuleTest implements ProtoCapsule<Object> {

    private byte[] value;

    @Override
    public byte[] getData() {
      return value;
    }

    @Override
    public Object getInstance() {
      return value;
    }

    @Override
    public String toString() {
      return "ProtoCapsuleTest{"
          + "value=" + Arrays.toString(value)
          + ", string=" + (value == null ? "" : new String(value))
          + '}';
    }
  }

  private static final class InMemoryFlusherDb implements DB<byte[], byte[]>, Flusher {
    private final String dbName;
    private final java.util.Map<WrappedByteArray, WrappedByteArray> store;

    private InMemoryFlusherDb(String dbName) {
      this(dbName, new java.util.HashMap<>());
    }

    private InMemoryFlusherDb(
        String dbName,
        java.util.Map<WrappedByteArray, WrappedByteArray> sharedStore) {
      this.dbName = dbName;
      this.store = sharedStore;
    }

    @Override
    public byte[] get(byte[] key) {
      WrappedByteArray value = store.get(WrappedByteArray.of(key));
      return value != null ? value.getBytes() : null;
    }

    @Override
    public void put(byte[] key, byte[] value) {
      store.put(WrappedByteArray.copyOf(key), WrappedByteArray.copyOf(value));
    }

    @Override
    public long size() {
      return store.size();
    }

    @Override
    public boolean isEmpty() {
      return store.isEmpty();
    }

    @Override
    public void remove(byte[] key) {
      store.remove(WrappedByteArray.of(key));
    }

    @Override
    public java.util.Iterator<java.util.Map.Entry<byte[], byte[]>> iterator() {
      java.util.Iterator<java.util.Map.Entry<WrappedByteArray, WrappedByteArray>> it =
          store.entrySet().iterator();
      return new java.util.Iterator<java.util.Map.Entry<byte[], byte[]>>() {
        @Override
        public boolean hasNext() {
          return it.hasNext();
        }

        @Override
        public java.util.Map.Entry<byte[], byte[]> next() {
          java.util.Map.Entry<WrappedByteArray, WrappedByteArray> entry = it.next();
          return new java.util.AbstractMap.SimpleImmutableEntry<>(
              entry.getKey().getBytes(),
              entry.getValue() != null ? entry.getValue().getBytes() : null);
        }
      };
    }

    @Override
    public void close() {
      // no-op
    }

    @Override
    public String getDbName() {
      return dbName;
    }

    @Override
    public void stat() {
      // no-op
    }

    @Override
    public DB<byte[], byte[]> newInstance() {
      return new InMemoryFlusherDb(dbName, store);
    }

    @Override
    public void flush(java.util.Map<WrappedByteArray, WrappedByteArray> batch) {
      for (java.util.Map.Entry<WrappedByteArray, WrappedByteArray> entry : batch.entrySet()) {
        WrappedByteArray key = entry.getKey();
        WrappedByteArray value = entry.getValue();
        if (value == null || value.getBytes() == null) {
          store.remove(WrappedByteArray.of(key.getBytes()));
        } else {
          store.put(WrappedByteArray.copyOf(key.getBytes()), WrappedByteArray.copyOf(value.getBytes()));
        }
      }
    }

    @Override
    public void reset() {
      store.clear();
    }
  }
}
