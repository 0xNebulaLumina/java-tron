use prometheus::{Counter, Histogram, Gauge, Registry, Opts, HistogramOpts};

pub struct Metrics {
    registry: Registry,
    pub request_counter: Counter,
    pub request_duration: Histogram,
    pub active_connections: Gauge,
    pub module_health: Gauge,
}

impl Metrics {
    pub fn new() -> Result<Self, prometheus::Error> {
        let registry = Registry::new();
        
        let request_counter = Counter::with_opts(
            Opts::new("backend_requests_total", "Total number of requests")
                .namespace("tron")
                .subsystem("backend")
        )?;
        
        let request_duration = Histogram::with_opts(
            HistogramOpts::new("backend_request_duration_seconds", "Request duration in seconds")
                .namespace("tron")
                .subsystem("backend")
        )?;
        
        let active_connections = Gauge::with_opts(
            Opts::new("backend_active_connections", "Number of active connections")
                .namespace("tron")
                .subsystem("backend")
        )?;
        
        let module_health = Gauge::with_opts(
            Opts::new("backend_module_health", "Health status of modules (1=healthy, 0=unhealthy)")
                .namespace("tron")
                .subsystem("backend")
        )?;
        
        registry.register(Box::new(request_counter.clone()))?;
        registry.register(Box::new(request_duration.clone()))?;
        registry.register(Box::new(active_connections.clone()))?;
        registry.register(Box::new(module_health.clone()))?;
        
        Ok(Self {
            registry,
            request_counter,
            request_duration,
            active_connections,
            module_health,
        })
    }
    
    pub fn registry(&self) -> &Registry {
        &self.registry
    }
    
    pub fn gather(&self) -> Vec<prometheus::proto::MetricFamily> {
        self.registry.gather()
    }
    
    pub fn to_text_format(&self) -> String {
        // Simple text format for now - in a real implementation we'd use the encoder
        format!("# Tron Backend Metrics\n# (placeholder implementation)")
    }
    
    pub fn update_module_health(&self, _module: &str, healthy: bool) {
        let value = if healthy { 1.0 } else { 0.0 };
        // Note: with_label_values is not available in this version of prometheus
        // We'll set the value directly for now
        self.module_health.set(value);
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new().expect("Failed to create metrics")
    }
} 