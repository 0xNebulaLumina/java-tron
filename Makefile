# Tron Storage Service Development Makefile

.PHONY: help build test clean run docker-build docker-run compose-up compose-down

# Default target
help:
	@echo "Available targets:"
	@echo "  build         - Build the Rust storage service"
	@echo "  test          - Run tests"
	@echo "  clean         - Clean build artifacts"
	@echo "  run           - Run the storage service locally"
	@echo "  docker-build  - Build Docker images"
	@echo "  docker-run    - Run with Docker"
	@echo "  compose-up    - Start services with Docker Compose"
	@echo "  compose-down  - Stop Docker Compose services"
	@echo "  proto-gen     - Generate protobuf code"

# Build the Rust storage service
build:
	cd rust-storage-service && cargo build --release

# Run tests
test:
	cd rust-storage-service && cargo test
	./gradlew test

# Clean build artifacts
clean:
	cd rust-storage-service && cargo clean
	./gradlew clean
	docker system prune -f

# Run the storage service locally
run:
	cd rust-storage-service && RUST_LOG=info cargo run

# Build Docker images
docker-build:
	docker build -t tron-storage-service rust-storage-service/
	docker build -t java-tron -f Dockerfile.java-tron .

# Run with Docker
docker-run:
	docker run -d --name tron-storage \
		-p 50051:50051 -p 9090:9090 \
		-v $(PWD)/data:/app/data \
		tron-storage-service

# Start services with Docker Compose
compose-up:
	docker-compose up -d

# Stop Docker Compose services
compose-down:
	docker-compose down -v

# Generate protobuf code
proto-gen:
	cd rust-storage-service && cargo build

# Development setup
dev-setup:
	@echo "Setting up development environment..."
	@echo "1. Installing Rust dependencies..."
	cd rust-storage-service && cargo fetch
	@echo "2. Building Java dependencies..."
	./gradlew build -x test
	@echo "3. Creating data directories..."
	mkdir -p data output-directory
	@echo "Development setup complete!"

# Performance test
perf-test:
	@echo "Running performance tests..."
	cd rust-storage-service && cargo test --release -- --ignored perf_tests

# Check code style
lint:
	cd rust-storage-service && cargo clippy -- -D warnings
	cd rust-storage-service && cargo fmt --check

# Fix code style
fmt:
	cd rust-storage-service && cargo fmt

# Generate documentation
docs:
	cd rust-storage-service && cargo doc --open

# Monitor logs
logs:
	docker-compose logs -f

# Health check
health:
	@echo "Checking storage service health..."
	@curl -f http://localhost:9090/metrics > /dev/null && echo "✓ Metrics endpoint OK" || echo "✗ Metrics endpoint failed"
	@grpc_health_probe -addr=localhost:50051 && echo "✓ gRPC service OK" || echo "✗ gRPC service failed"

# Backup data
backup:
	@echo "Creating backup..."
	tar -czf backup-$(shell date +%Y%m%d-%H%M%S).tar.gz data/
	@echo "Backup created: backup-$(shell date +%Y%m%d-%H%M%S).tar.gz" 