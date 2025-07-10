package org.tron.core.storage.spi;

import java.util.Map;

/** Callback interface for receiving storage metrics. */
public interface MetricsCallback {
  void onMetrics(String dbName, Map<String, Object> metrics);
}
