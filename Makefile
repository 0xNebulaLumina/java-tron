# Makefile for java-tron storage PoC

.PHONY: help build test clean rust-build java-build docker-build docker-test integration-test performance-test test-all

# Configuration
GRPC_HOST ?= localhost
GRPC_PORT ?= 50011

# Default target
help:
	@echo "Available targets:"
	@echo "  build              - Build both Rust and Java components"
	@echo "  test               - Run basic tests"
	@echo "  test-all           - Run all tests (unit + integration + performance)"
	@echo "  integration-test   - Run integration tests (requires gRPC server)"
	@echo "  performance-test   - Run performance benchmarks (requires gRPC server)"
	@echo "  perf-analysis      - Run detailed performance analysis with reports"
	@echo "  perf-analysis-strict - Run performance analysis with dependency verification"
	@echo "  dual-mode-test     - Test both embedded and remote storage modes"
	@echo "  embedded-test      - Test embedded storage mode only"
	@echo "  remote-test        - Test remote storage mode only (requires gRPC server)"
	@echo "  dual-mode-perf     - Compare performance of embedded vs remote modes"
	@echo "  clean              - Clean build artifacts"
	@echo "  rust-build         - Build Rust storage service"
	@echo "  java-build         - Build Java components"
	@echo "  docker-build       - Build Docker images"
	@echo "  docker-test        - Run tests in Docker"
	@echo "  rust-run           - Run Rust storage service locally" 
	@echo "  java-test          - Run Java unit tests locally"
	@echo "  fix-verification   - Fix Gradle dependency verification issues"
	@echo "  update-verification - Update dependency verification metadata"

# Build all components
build: rust-build java-build

# Build Rust storage service
rust-build:
	@echo "Building Rust storage service..."
	cd rust-storage-service && cargo build --release

# Build Java components
java-build:
	@echo "Building Java components..."
	./gradlew build -x test --dependency-verification=off

# Run tests
test: java-test

# Run Java tests
java-test:
	@echo "Running Java unit tests..."
	./gradlew :framework:test --tests "org.tron.core.storage.spi.StorageSPITest" --dependency-verification=off

# Run integration tests (requires gRPC server)
integration-test:
	@echo "Running integration tests..."
	./gradlew :framework:test --tests "org.tron.core.storage.spi.StorageSPIIntegrationTest" \
		--dependency-verification=off \
		-Dstorage.remote.host=$(GRPC_HOST) -Dstorage.remote.port=$(GRPC_PORT)

# Run performance benchmarks (requires gRPC server)
performance-test:
	@echo "Running performance benchmarks..."
	./gradlew :framework:test --tests "org.tron.core.storage.spi.StoragePerformanceBenchmark" \
		--dependency-verification=off \
		-Dstorage.remote.host=$(GRPC_HOST) -Dstorage.remote.port=$(GRPC_PORT)

# Run all tests
test-all: java-test integration-test performance-test

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	./gradlew clean --dependency-verification=off
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
	curl -s http://localhost:50011 || echo "Service not responding"

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

# End-to-end testing workflow
e2e-test:
	@echo "Running end-to-end testing workflow..."
	@echo "1. Building components..."
	$(MAKE) build
	@echo "2. Starting Rust storage service in background..."
	$(MAKE) rust-run &
	@sleep 5
	@echo "3. Running integration tests..."
	$(MAKE) integration-test
	@echo "4. Running performance benchmarks..."
	$(MAKE) performance-test
	@echo "5. Stopping background services..."
	@pkill -f "cargo run" || true

# Quick smoke test
smoke-test:
	@echo "Running smoke test..."
	$(MAKE) build
	$(MAKE) java-test

# Performance analysis with detailed output
perf-analysis:
	@echo "Running detailed performance analysis..."
	@mkdir -p reports
	./gradlew :framework:test --tests "org.tron.core.storage.spi.StoragePerformanceBenchmark" \
		--dependency-verification=off \
		-Dstorage.remote.host=$(GRPC_HOST) -Dstorage.remote.port=$(GRPC_PORT) 2>&1 | tee reports/performance-report-$(shell date +%Y%m%d-%H%M%S).txt
	@echo "Extracting performance metrics..."
	@./scripts/extract-metrics.sh reports/performance-report-*.txt || echo "Metrics extraction script not found, check reports directory for raw output"

# Performance analysis with dependency verification (will fail if metadata is stale)
perf-analysis-strict:
	@echo "Running detailed performance analysis with strict dependency verification..."
	@mkdir -p reports
	./gradlew :framework:test --tests "org.tron.core.storage.spi.StoragePerformanceBenchmark.generatePerformanceReport" \
		-Dstorage.remote.host=$(GRPC_HOST) -Dstorage.remote.port=$(GRPC_PORT) | tee reports/performance-report-$(shell date +%Y%m%d-%H%M%S).txt

# Update dependency verification metadata (run this to fix verification issues permanently)
update-verification:
	@echo "Updating dependency verification metadata..."
	./gradlew --write-verification-metadata sha256 help

# Fix dependency verification by regenerating metadata
fix-verification:
	@echo "Fixing dependency verification metadata..."
	@echo "This will update gradle/verification-metadata.xml with current dependency checksums"
	./gradlew --write-verification-metadata sha256 :framework:dependencies

# Dual-mode testing targets
dual-mode-test:
	@echo "Running dual-mode storage tests..."
	./gradlew :framework:test --tests "org.tron.core.storage.spi.StorageSpiFactoryTest" \
		--dependency-verification=off
	./gradlew :framework:test --tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest" \
		--dependency-verification=off \
		-Dstorage.remote.host=$(GRPC_HOST) -Dstorage.remote.port=$(GRPC_PORT)

# Test embedded storage mode only
embedded-test:
	@echo "Running embedded storage tests..."
	./gradlew :framework:test --tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest.testEmbeddedStorageMode" \
		--dependency-verification=off

# Test remote storage mode only (requires gRPC server)
remote-test:
	@echo "Running remote storage tests..."
	./gradlew :framework:test --tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest.testRemoteStorageMode" \
		--dependency-verification=off \
		-Dstorage.remote.host=$(GRPC_HOST) -Dstorage.remote.port=$(GRPC_PORT)

# Compare performance of embedded vs remote modes
dual-mode-perf:
	@echo "Running dual-mode performance comparison..."
	@mkdir -p reports
	./gradlew :framework:test --tests "org.tron.core.storage.spi.DualModePerformanceBenchmark.generateComparativePerformanceReport" \
		--dependency-verification=off \
		-Dstorage.remote.host=$(GRPC_HOST) -Dstorage.remote.port=$(GRPC_PORT) 2>&1 | tee reports/dual-mode-performance-$(shell date +%Y%m%d-%H%M%S).txt

# Test embedded mode performance only
embedded-perf:
	@echo "Running embedded mode performance tests..."
	@mkdir -p reports
	./gradlew :framework:test --tests "org.tron.core.storage.spi.DualModePerformanceBenchmark.generateEmbeddedPerformanceReport" \
		--dependency-verification=off 2>&1 | tee reports/embedded-performance-$(shell date +%Y%m%d-%H%M%S).txt

# Test remote mode performance only (requires gRPC server)
remote-perf:
	@echo "Running remote mode performance tests..."
	@mkdir -p reports
	./gradlew :framework:test --tests "org.tron.core.storage.spi.DualModePerformanceBenchmark.generateRemotePerformanceReport" \
		--dependency-verification=off \
		-Dstorage.remote.host=$(GRPC_HOST) -Dstorage.remote.port=$(GRPC_PORT) 2>&1 | tee reports/remote-performance-$(shell date +%Y%m%d-%H%M%S).txt

# Show storage configuration info
storage-config:
	@echo "Displaying current storage configuration..."
	./gradlew :framework:test --tests "org.tron.core.storage.spi.StorageSpiFactoryTest.testConfigurationInfo" \
		--dependency-verification=off 