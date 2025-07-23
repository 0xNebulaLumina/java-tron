package org.tron.core.config;

import com.alibaba.fastjson.parser.ParserConfig;
import javax.annotation.PostConstruct;
import lombok.extern.slf4j.Slf4j;
import org.rocksdb.RocksDB;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.context.ApplicationContext;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Conditional;
import org.springframework.context.annotation.Configuration;
import org.springframework.context.annotation.Import;
import org.tron.common.utils.StorageUtils;
import org.tron.core.config.args.Args;
import org.tron.core.db.RevokingDatabase;
import org.tron.core.db.backup.BackupRocksDBAspect;
import org.tron.core.db.backup.NeedBeanCondition;
import org.tron.core.db2.core.SnapshotManager;
import org.tron.core.services.interfaceOnPBFT.RpcApiServiceOnPBFT;
import org.tron.core.services.interfaceOnPBFT.http.PBFT.HttpApiOnPBFTService;
import org.tron.core.services.interfaceOnSolidity.RpcApiServiceOnSolidity;
import org.tron.core.services.interfaceOnSolidity.http.solidity.HttpApiOnSolidityService;
import org.tron.core.storage.spi.StorageBackendFactoryImpl;
import org.tron.core.execution.spi.ExecutionSpiFactory;

@Slf4j(topic = "app")
@Configuration
@Import(CommonConfig.class)
public class DefaultConfig {

  static {
    RocksDB.loadLibrary();
    ParserConfig.getGlobalInstance().setSafeMode(true);

    // Initialize StorageBackendFactory as early as possible during class loading
    // This ensures it's available when database constructors are called during Spring bean creation
    try {
      StorageBackendFactoryImpl.initialize();
      logger.info("StorageBackendFactory initialized during static class loading");
    } catch (Exception e) {
      logger.warn(
          "Failed to initialize StorageBackendFactory during static loading, "
              + "will retry in @PostConstruct",
          e);
    }

    // Initialize ExecutionSPI factory
    try {
      ExecutionSpiFactory.initialize();
      logger.info("ExecutionSPI factory initialized during static class loading");
    } catch (Exception e) {
      logger.warn(
          "Failed to initialize ExecutionSPI during static loading, "
              + "will retry in @PostConstruct",
          e);
    }
  }

  @Autowired public ApplicationContext appCtx;

  @Autowired public CommonConfig commonConfig;

  public DefaultConfig() {}

  /**
   * Initialize StorageBackendFactory early in Spring lifecycle. This is a backup initialization in
   * case the static block failed.
   */
  @PostConstruct
  public void initializeStorageBackend() {
    try {
      // Check if already initialized
      if (org.tron.core.storage.spi.StorageBackendFactory.getInstance() == null) {
        StorageBackendFactoryImpl.initialize();
        logger.info("StorageBackendFactory initialized in @PostConstruct");
      } else {
        logger.debug("StorageBackendFactory already initialized");
      }
    } catch (Exception e) {
      logger.error("Failed to initialize StorageBackendFactory in @PostConstruct", e);
    }
  }

  /**
   * Initialize ExecutionSPI factory early in Spring lifecycle. This is a backup initialization in
   * case the static block failed.
   */
  @PostConstruct
  public void initializeExecutionSpi() {
    try {
      // Check if already initialized
      if (ExecutionSpiFactory.getInstance() == null) {
        ExecutionSpiFactory.initialize();
        logger.info("ExecutionSPI factory initialized in @PostConstruct");
      } else {
        logger.debug("ExecutionSPI factory already initialized");
      }
    } catch (Exception e) {
      logger.error("Failed to initialize ExecutionSPI in @PostConstruct", e);
    }
  }

  @Bean(destroyMethod = "")
  public RevokingDatabase revokingDatabase() {
    try {
      return new SnapshotManager(StorageUtils.getOutputDirectoryByDbName("block"));
    } finally {
      logger.info("key-value data source created.");
    }
  }

  @Bean
  public RpcApiServiceOnSolidity getRpcApiServiceOnSolidity() {
    boolean isSolidityNode = Args.getInstance().isSolidityNode();
    if (!isSolidityNode) {
      return new RpcApiServiceOnSolidity();
    }

    return null;
  }

  @Bean
  public HttpApiOnSolidityService getHttpApiOnSolidityService() {
    boolean isSolidityNode = Args.getInstance().isSolidityNode();
    if (!isSolidityNode) {
      return new HttpApiOnSolidityService();
    }

    return null;
  }

  @Bean
  public RpcApiServiceOnPBFT getRpcApiServiceOnPBFT() {
    boolean isSolidityNode = Args.getInstance().isSolidityNode();
    if (!isSolidityNode) {
      return new RpcApiServiceOnPBFT();
    }

    return null;
  }

  @Bean
  public HttpApiOnPBFTService getHttpApiOnPBFTService() {
    boolean isSolidityNode = Args.getInstance().isSolidityNode();
    if (!isSolidityNode) {
      return new HttpApiOnPBFTService();
    }

    return null;
  }

  @Bean
  @Conditional(NeedBeanCondition.class)
  public BackupRocksDBAspect backupRocksDBAspect() {
    return new BackupRocksDBAspect();
  }
}
