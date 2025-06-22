# Build stage
FROM rust:1.75-slim as builder

WORKDIR /app

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libclang-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src
COPY proto ./proto
COPY build.rs ./

# Build the application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy the binary from builder stage
COPY --from=builder /app/target/release/storage-service /app/storage-service

# Create data directory
RUN mkdir -p /app/data

# Expose ports
EXPOSE 50051 9090

# Set environment variables
ENV RUST_LOG=info
ENV TRON_STORAGE_HOST=0.0.0.0
ENV TRON_STORAGE_PORT=50051
ENV TRON_STORAGE_DATA_DIR=/app/data

# Run the service
CMD ["./storage-service"] 