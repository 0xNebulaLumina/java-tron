pub mod service;

pub use service::BackendService;

// Include generated protobuf code
pub mod backend {
    tonic::include_proto!("tron.backend");
}

#[cfg(test)]
mod tests; 