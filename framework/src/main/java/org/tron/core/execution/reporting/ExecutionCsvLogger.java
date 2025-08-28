package org.tron.core.execution.reporting;

import java.io.BufferedWriter;
import java.io.File;
import java.io.FileWriter;
import java.io.IOException;
import java.time.LocalDateTime;
import java.time.format.DateTimeFormatter;
import java.util.UUID;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;
import java.util.concurrent.atomic.AtomicReference;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Execution CSV logger with background queue and file rotation.
 * 
 * <p>This logger provides a production-safe, non-blocking way to record transaction
 * execution details to CSV files. It uses a bounded queue and background thread
 * to avoid impacting the main execution path.
 * 
 * <p>Features:
 * <ul>
 *   <li>Non-blocking enqueue with backpressure handling
 *   <li>File rotation based on size limits
 *   <li>Configurable sampling rate
 *   <li>Metrics collection for monitoring
 * </ul>
 * 
 * <p>Configuration via system properties:
 * <ul>
 *   <li>exec.csv.enabled: Enable/disable logging (default: false)
 *   <li>exec.csv.dir: Output directory (default: output-directory/execution-csv)
 *   <li>exec.csv.sampleRate: Sample every Nth transaction (default: 1)
 *   <li>exec.csv.rotateMb: Rotate when file exceeds MB (default: 256)
 *   <li>exec.csv.queueSize: Queue capacity (default: 10000)
 * </ul>
 */
public class ExecutionCsvLogger {
  
  private static final Logger logger = LoggerFactory.getLogger(ExecutionCsvLogger.class);
  
  // Configuration constants
  private static final String CONFIG_ENABLED = "exec.csv.enabled";
  private static final String CONFIG_DIR = "exec.csv.dir";
  private static final String CONFIG_SAMPLE_RATE = "exec.csv.sampleRate";
  private static final String CONFIG_ROTATE_MB = "exec.csv.rotateMb";
  private static final String CONFIG_QUEUE_SIZE = "exec.csv.queueSize";
  
  // Default values
  private static final String DEFAULT_DIR = "output-directory/execution-csv";
  private static final int DEFAULT_SAMPLE_RATE = 1;
  private static final int DEFAULT_ROTATE_MB = 256;
  private static final int DEFAULT_QUEUE_SIZE = 10000;
  private static final long ROTATE_BYTES_THRESHOLD = 1024 * 1024; // 1MB
  
  // Singleton instance
  private static volatile ExecutionCsvLogger instance;
  private static final Object lock = new Object();
  
  // Configuration
  private final String outputDir;
  private final int sampleRate;
  private final long rotateBytes;
  private final String runId;
  
  // Queue and background processing
  private final BlockingQueue<ExecutionCsvRecord> queue;
  private final Thread writerThread;
  private final AtomicBoolean running = new AtomicBoolean(false);
  private final AtomicBoolean shutdown = new AtomicBoolean(false);
  
  // File management
  private final AtomicReference<BufferedWriter> currentWriter = new AtomicReference<>();
  private final AtomicReference<String> currentFilePath = new AtomicReference<>();
  private final AtomicLong currentFileSize = new AtomicLong(0);
  private final AtomicInteger fileSequence = new AtomicInteger(0);
  
  // Metrics and sampling
  private final AtomicLong transactionCounter = new AtomicLong(0);
  private final AtomicLong recordsEnqueued = new AtomicLong(0);
  private final AtomicLong recordsDropped = new AtomicLong(0);
  private final AtomicLong recordsWritten = new AtomicLong(0);
  private final AtomicLong writeErrors = new AtomicLong(0);
  
  /**
   * Private constructor for singleton pattern.
   */
  private ExecutionCsvLogger(String outputDir, int sampleRate, long rotateBytes, int queueSize) {
    this.outputDir = outputDir;
    this.sampleRate = sampleRate;
    this.rotateBytes = rotateBytes * ROTATE_BYTES_THRESHOLD;
    this.runId = generateRunId();
    
    this.queue = new ArrayBlockingQueue<>(queueSize);
    this.writerThread = new Thread(this::processQueue, "ExecutionCsvWriter");
    this.writerThread.setDaemon(true);
  }
  
  /**
   * Get the singleton instance, creating it if necessary.
   * 
   * @return ExecutionCsvLogger instance
   */
  public static ExecutionCsvLogger getInstance() {
    ExecutionCsvLogger result = instance;
    if (result == null) {
      synchronized (lock) {
        result = instance;
        if (result == null) {
          if (isEnabled()) {
            instance = result = createInstance();
          } else {
            // Return a no-op instance
            instance = result = new ExecutionCsvLogger("", 1, 0, 1) {
              @Override
              public void logRecord(ExecutionCsvRecord record) {
                // No-op
              }
            };
          }
        }
      }
    }
    return result;
  }
  
  /**
   * Create a new logger instance based on configuration.
   */
  private static ExecutionCsvLogger createInstance() {
    String dir = System.getProperty(CONFIG_DIR, DEFAULT_DIR);
    int sampleRate = Integer.parseInt(System.getProperty(CONFIG_SAMPLE_RATE, String.valueOf(DEFAULT_SAMPLE_RATE)));
    int rotateMb = Integer.parseInt(System.getProperty(CONFIG_ROTATE_MB, String.valueOf(DEFAULT_ROTATE_MB)));
    int queueSize = Integer.parseInt(System.getProperty(CONFIG_QUEUE_SIZE, String.valueOf(DEFAULT_QUEUE_SIZE)));
    
    ExecutionCsvLogger logger = new ExecutionCsvLogger(dir, sampleRate, rotateMb, queueSize);
    logger.start();
    return logger;
  }
  
  /**
   * Check if CSV logging is enabled.
   * 
   * @return true if enabled
   */
  public static boolean isEnabled() {
    return Boolean.parseBoolean(System.getProperty(CONFIG_ENABLED, "false"));
  }
  
  /**
   * Start the background writer thread.
   */
  public void start() {
    if (running.compareAndSet(false, true)) {
      // Create output directory
      File dir = new File(outputDir);
      if (!dir.exists()) {
        if (!dir.mkdirs()) {
          logger.error("Failed to create CSV output directory: {}", outputDir);
          return;
        }
      }
      
      // Start background thread
      writerThread.start();
      logger.info("ExecutionCsvLogger started with dir={}, sampleRate={}, rotateMb={}", 
                  outputDir, sampleRate, rotateBytes / ROTATE_BYTES_THRESHOLD);
      
      // Add shutdown hook to ensure proper cleanup
      Runtime.getRuntime().addShutdownHook(new Thread(this::shutdown, "ExecutionCsvLogger-Shutdown"));
    }
  }
  
  /**
   * Stop the logger and flush remaining records.
   */
  public void shutdown() {
    if (shutdown.compareAndSet(false, true)) {
      running.set(false);
      
      try {
        // Wait for writer thread to finish
        writerThread.join(5000); // 5 second timeout
      } catch (InterruptedException e) {
        Thread.currentThread().interrupt();
      }
      
      // Close current writer
      BufferedWriter writer = currentWriter.get();
      if (writer != null) {
        try {
          writer.close();
        } catch (IOException e) {
          logger.error("Error closing CSV writer", e);
        }
      }
      
      logger.info("ExecutionCsvLogger shutdown. Records enqueued={}, written={}, dropped={}", 
                  recordsEnqueued.get(), recordsWritten.get(), recordsDropped.get());
    }
  }
  
  /**
   * Log an execution record (main entry point).
   * 
   * @param record Record to log
   */
  public void logRecord(ExecutionCsvRecord record) {
    if (!running.get() || record == null) {
      return;
    }
    
    // Apply sampling
    long txCount = transactionCounter.incrementAndGet();
    if (sampleRate > 1 && (txCount % sampleRate) != 0) {
      return;
    }
    
    // Try to enqueue (non-blocking)
    if (queue.offer(record)) {
      recordsEnqueued.incrementAndGet();
    } else {
      recordsDropped.incrementAndGet();
      if (recordsDropped.get() % 1000 == 0) {
        logger.warn("CSV queue full, dropped {} records so far", recordsDropped.get());
      }
    }
  }
  
  /**
   * Background thread that processes the queue and writes to files.
   */
  private void processQueue() {
    logger.info("CSV writer thread started");
    
    while (running.get() || !queue.isEmpty()) {
      try {
        ExecutionCsvRecord record = queue.poll(1, TimeUnit.SECONDS);
        if (record != null) {
          writeRecord(record);
        }
      } catch (InterruptedException e) {
        Thread.currentThread().interrupt();
        break;
      } catch (Exception e) {
        logger.error("Error processing CSV record", e);
        writeErrors.incrementAndGet();
      }
    }
    
    logger.info("CSV writer thread stopped");
  }
  
  /**
   * Write a record to the current file, handling rotation.
   */
  private void writeRecord(ExecutionCsvRecord record) {
    try {
      BufferedWriter writer = getCurrentWriter();
      if (writer != null) {
        String line = record.toCsvRow();
        writer.write(line);
        writer.newLine();
        writer.flush();
        
        // Update file size and check for rotation
        currentFileSize.addAndGet(line.length() + 1); // +1 for newline
        recordsWritten.incrementAndGet();
        
        if (currentFileSize.get() > rotateBytes) {
          rotateFile();
        }
      }
    } catch (IOException e) {
      logger.error("Error writing CSV record", e);
      writeErrors.incrementAndGet();
    }
  }
  
  /**
   * Get or create the current writer, handling file creation and headers.
   */
  private BufferedWriter getCurrentWriter() {
    BufferedWriter writer = currentWriter.get();
    if (writer == null) {
      synchronized (this) {
        writer = currentWriter.get();
        if (writer == null) {
          writer = createNewWriter();
          currentWriter.set(writer);
        }
      }
    }
    return writer;
  }
  
  /**
   * Create a new writer for a new file.
   */
  private BufferedWriter createNewWriter() {
    try {
      String fileName = buildFileName();
      String filePath = outputDir + File.separator + fileName;
      
      BufferedWriter writer = new BufferedWriter(new FileWriter(filePath, false));
      
      // Write CSV header
      writer.write(ExecutionCsvRecord.getCsvHeader());
      writer.newLine();
      writer.flush();
      
      currentFilePath.set(filePath);
      currentFileSize.set(ExecutionCsvRecord.getCsvHeader().length() + 1);
      
      logger.info("Created new CSV file: {}", filePath);
      return writer;
      
    } catch (IOException e) {
      logger.error("Failed to create CSV writer", e);
      writeErrors.incrementAndGet();
      return null;
    }
  }
  
  /**
   * Rotate to a new file.
   */
  private void rotateFile() {
    synchronized (this) {
      BufferedWriter oldWriter = currentWriter.getAndSet(null);
      if (oldWriter != null) {
        try {
          oldWriter.close();
        } catch (IOException e) {
          logger.error("Error closing old CSV file", e);
        }
      }
      
      fileSequence.incrementAndGet();
      logger.info("Rotated CSV file at {} MB", currentFileSize.get() / ROTATE_BYTES_THRESHOLD);
    }
  }
  
  /**
   * Build filename for current file.
   */
  private String buildFileName() {
    // Format: runId-EXECMODE-STORAGEMODE-seq.csv
    String execMode = getExecutionMode();
    String storageMode = getStorageMode();
    int seq = fileSequence.get();
    
    if (seq == 0) {
      return String.format("%s-%s-%s.csv", runId, execMode, storageMode);
    } else {
      return String.format("%s-%s-%s-%d.csv", runId, execMode, storageMode, seq);
    }
  }
  
  /**
   * Get current execution mode for filename.
   */
  private String getExecutionMode() {
    try {
      return org.tron.core.execution.spi.ExecutionSpiFactory.determineExecutionMode().toString();
    } catch (Exception e) {
      return "UNKNOWN";
    }
  }
  
  /**
   * Get current storage mode for filename.
   */
  private String getStorageMode() {
    try {
      return org.tron.core.storage.spi.StorageSpiFactory.determineStorageMode().toString();
    } catch (Exception e) {
      return "UNKNOWN";
    }
  }
  
  /**
   * Generate unique run ID.
   */
  private String generateRunId() {
    String timestamp = LocalDateTime.now().format(DateTimeFormatter.ofPattern("yyyyMMdd-HHmmss"));
    String uuid = UUID.randomUUID().toString().substring(0, 8);
    return timestamp + "-" + uuid;
  }
  
  /**
   * Get metrics information.
   * 
   * @return Metrics string
   */
  public String getMetrics() {
    return String.format(
        "ExecutionCsvLogger metrics: enqueued=%d, written=%d, dropped=%d, writeErrors=%d, currentFile=%s",
        recordsEnqueued.get(), recordsWritten.get(), recordsDropped.get(), 
        writeErrors.get(), currentFilePath.get()
    );
  }
  
  /**
   * Get dropped record count.
   * 
   * @return Number of dropped records
   */
  public long getDroppedCount() {
    return recordsDropped.get();
  }
  
  /**
   * Get written record count.
   * 
   * @return Number of written records
   */
  public long getWrittenCount() {
    return recordsWritten.get();
  }
  
  /**
   * Get current queue size.
   * 
   * @return Current queue size
   */
  public int getQueueSize() {
    return queue.size();
  }
}