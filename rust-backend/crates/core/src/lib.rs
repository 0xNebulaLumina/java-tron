pub mod service;

pub use service::BackendService;

// Include generated protobuf code
pub mod backend {
    tonic::include_proto!("tron.backend");
}

// Conformance testing framework
pub mod conformance;

#[cfg(test)]
mod tests; 