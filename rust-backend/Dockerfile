# Build stage
FROM rust:1.75-slim as builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# Copy proto files first
# TODO: fixme, now we should use `{project_root}/framework/src/main/proto/`
COPY proto/ /app/proto/

# Copy Cargo files
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/

# Copy source code
COPY src/ ./src/
COPY build.rs ./

# Build the application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -r -s /bin/false tron-backend

# Create data directory
RUN mkdir -p /app/data && chown tron-backend:tron-backend /app/data

# Copy binary from builder stage
COPY --from=builder /app/target/release/tron-backend /usr/local/bin/

# Copy configuration
COPY config.toml /app/

# Set working directory
WORKDIR /app

# Switch to app user
USER tron-backend

# Expose port
EXPOSE 50011

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD grpc_health_probe -addr=localhost:50011 || exit 1

# Run the application
CMD ["tron-backend"] 