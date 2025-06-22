# Makefile for java-tron storage PoC

.PHONY: help build test clean rust-build java-build docker-build docker-test

# Default target
help:
	@echo "Available targets:"
	@echo "  build        - Build both Rust and Java components"
	@echo "  test         - Run tests"
	@echo "  clean        - Clean build artifacts"
	@echo "  rust-build   - Build Rust storage service"
	@echo "  java-build   - Build Java components"
	@echo "  docker-build - Build Docker images"
	@echo "  docker-test  - Run tests in Docker"
	@echo "  rust-run     - Run Rust storage service locally"
	@echo "  java-test    - Run Java tests locally"

# Build all components
build: rust-build java-build

# Build Rust storage service
rust-build:
	@echo "Building Rust storage service..."
	cd rust-storage-service && cargo build --release

# Build Java components
java-build:
	@echo "Building Java components..."
	./gradlew build -x test

# Run tests
test: java-test

# Run Java tests
java-test:
	@echo "Running Java tests..."
	./gradlew :framework:test --tests "org.tron.core.storage.spi.*"

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	./gradlew clean
	cd rust-storage-service && cargo clean
	rm -rf data/

# Build Docker images
docker-build:
	@echo "Building Docker images..."
	docker compose build

# Run tests in Docker
docker-test:
	@echo "Running tests in Docker..."
	mkdir -p data/rust-storage data/java-tron
	docker compose up --build --exit-code-from java-tron-test

# Run Rust storage service locally
rust-run:
	@echo "Starting Rust storage service..."
	mkdir -p data/rust-storage
	cd rust-storage-service && RUST_LOG=info DATA_PATH=../data/rust-storage cargo run

# Check Rust service health
rust-health:
	@echo "Checking Rust service health..."
	curl -s http://localhost:50051 || echo "Service not responding"

# Development setup
dev-setup:
	@echo "Setting up development environment..."
	mkdir -p data/rust-storage data/java-tron
	@echo "Development environment ready!"

# Generate protobuf files (if needed)
proto-gen:
	@echo "Generating protobuf files..."
	cd rust-storage-service && cargo build

# Run integration test
integration-test: docker-test

# Show logs from Docker services
logs:
	docker compose logs -f 