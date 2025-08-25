package org.tron.core.execution.spi;

import java.io.BufferedWriter;
import java.io.File;
import java.io.FileWriter;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.time.LocalDate;
import java.time.format.DateTimeFormatter;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.LinkedBlockingQueue;
import java.util.concurrent.atomic.AtomicBoolean;
import lombok.extern.slf4j.Slf4j;

/**
 * Thread-safe CSV logger for execution metrics with automatic file rotation.
 *
 * <p>Features:
 * - Asynchronous CSV writing to minimize performance impact
 * - Daily file rotation with timestamped filenames
 * - Thread-safe operations
 * - Graceful shutdown handling
 * - Automatic directory creation
 */
@Slf4j
public class ExecutionMetricsLogger implements AutoCloseable {

  private static final String FILE_PREFIX = "execution-metrics";
  private static final String FILE_EXTENSION = ".csv";
  private static final DateTimeFormatter DATE_FORMATTER = DateTimeFormatter.ofPattern("yyyy-MM-dd");

  private final Path outputDirectory;
  private final BlockingQueue<ExecutionMetrics> metricsQueue;
  private final AtomicBoolean running;
  private final Thread writerThread;
  
  private volatile String currentDateStr;
  private volatile BufferedWriter currentWriter;

  /**
   * Create a new ExecutionMetricsLogger.
   *
   * @param outputDirectory Directory where CSV files will be written
   * @throws IOException if directory cannot be created or accessed
   */
  public ExecutionMetricsLogger(String outputDirectory) throws IOException {
    this.outputDirectory = Paths.get(outputDirectory);
    this.metricsQueue = new LinkedBlockingQueue<>();
    this.running = new AtomicBoolean(true);
    
    // Ensure output directory exists
    Files.createDirectories(this.outputDirectory);
    
    // Initialize current date and writer
    this.currentDateStr = LocalDate.now().format(DATE_FORMATTER);
    this.currentWriter = createWriter(currentDateStr);
    
    // Start background writer thread
    this.writerThread = new Thread(this::writerLoop, "ExecutionMetricsWriter");
    this.writerThread.setDaemon(true);
    this.writerThread.start();
    
    logger.info("ExecutionMetricsLogger initialized. Output directory: {}", outputDirectory);
  }

  /**
   * Log execution metrics asynchronously.
   *
   * @param metrics The metrics to log
   */
  public void log(ExecutionMetrics metrics) {
    if (metrics == null) {
      return;
    }
    
    if (!running.get()) {
      logger.warn("ExecutionMetricsLogger is shutdown, ignoring metrics: {}", 
          metrics.getTransactionId());
      return;
    }

    try {
      if (!metricsQueue.offer(metrics)) {
        logger.warn("Metrics queue is full, dropping metrics for transaction: {}", 
            metrics.getTransactionId());
      }
    } catch (Exception e) {
      logger.error("Failed to queue metrics for transaction: {}", 
          metrics.getTransactionId(), e);
    }
  }

  /**
   * Flush any pending metrics and close the logger.
   */
  @Override
  public void close() {
    logger.info("Shutting down ExecutionMetricsLogger...");
    
    running.set(false);
    
    // Interrupt writer thread to wake it up
    writerThread.interrupt();
    
    try {
      // Wait for writer thread to finish processing remaining items
      writerThread.join(5000); // Wait up to 5 seconds
    } catch (InterruptedException e) {
      logger.warn("Interrupted while waiting for writer thread to finish");
      Thread.currentThread().interrupt();
    }
    
    // Close current writer
    closeCurrentWriter();
    
    logger.info("ExecutionMetricsLogger shutdown complete");
  }

  /**
   * Main writer loop that processes metrics from the queue.
   */
  private void writerLoop() {
    logger.debug("ExecutionMetrics writer thread started");
    
    while (running.get() || !metricsQueue.isEmpty()) {
      try {
        // Poll with timeout to check running status periodically
        ExecutionMetrics metrics = metricsQueue.poll(1000, java.util.concurrent.TimeUnit.MILLISECONDS);
        
        if (metrics != null) {
          writeMetrics(metrics);
        }
        
      } catch (InterruptedException e) {
        // Thread was interrupted, check if we should continue
        logger.debug("Writer thread interrupted");
        if (!running.get()) {
          break;
        }
      } catch (Exception e) {
        logger.error("Unexpected error in metrics writer thread", e);
      }
    }
    
    logger.debug("ExecutionMetrics writer thread finished");
  }

  /**
   * Write metrics to the current CSV file, handling rotation if necessary.
   */
  private void writeMetrics(ExecutionMetrics metrics) {
    try {
      // Check if we need to rotate the file (new day)
      String dateStr = LocalDate.now().format(DATE_FORMATTER);
      if (!dateStr.equals(currentDateStr)) {
        rotateFile(dateStr);
      }
      
      // Write metrics to current file
      if (currentWriter != null) {
        currentWriter.write(metrics.toCsvRow());
        currentWriter.newLine();
        currentWriter.flush(); // Ensure data is written immediately
      }
      
    } catch (IOException e) {
      logger.error("Failed to write metrics for transaction: {}", 
          metrics.getTransactionId(), e);
      
      // Try to recover by recreating the writer
      try {
        closeCurrentWriter();
        currentWriter = createWriter(currentDateStr);
        logger.info("Recreated CSV writer after error");
      } catch (IOException retryError) {
        logger.error("Failed to recreate CSV writer", retryError);
      }
    }
  }

  /**
   * Rotate to a new CSV file for the given date.
   */
  private void rotateFile(String newDateStr) throws IOException {
    logger.info("Rotating metrics file from {} to {}", currentDateStr, newDateStr);
    
    closeCurrentWriter();
    currentDateStr = newDateStr;
    currentWriter = createWriter(newDateStr);
  }

  /**
   * Create a new CSV writer for the given date.
   */
  private BufferedWriter createWriter(String dateStr) throws IOException {
    String filename = FILE_PREFIX + "-" + dateStr + FILE_EXTENSION;
    Path filePath = outputDirectory.resolve(filename);
    
    boolean fileExists = Files.exists(filePath);
    
    BufferedWriter writer = new BufferedWriter(
        new FileWriter(filePath.toFile(), true)); // Append mode
    
    // Write header if this is a new file
    if (!fileExists) {
      writer.write(ExecutionMetrics.getCsvHeader());
      writer.newLine();
      logger.info("Created new metrics file: {}", filePath);
    } else {
      logger.info("Appending to existing metrics file: {}", filePath);
    }
    
    return writer;
  }

  /**
   * Close the current writer if it exists.
   */
  private void closeCurrentWriter() {
    if (currentWriter != null) {
      try {
        currentWriter.flush();
        currentWriter.close();
      } catch (IOException e) {
        logger.warn("Error closing CSV writer", e);
      } finally {
        currentWriter = null;
      }
    }
  }

  /**
   * Get the current queue size (for monitoring/debugging).
   */
  public int getQueueSize() {
    return metricsQueue.size();
  }

  /**
   * Check if the logger is running.
   */
  public boolean isRunning() {
    return running.get();
  }

  /**
   * Get the output directory path.
   */
  public Path getOutputDirectory() {
    return outputDirectory;
  }
}