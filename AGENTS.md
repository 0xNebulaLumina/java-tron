# Repository Guidelines

## Project Structure & Architecture
- Java modules: `framework` (runtime, distributions), `protocol` (protobuf), `consensus`, `crypto`, `chainbase`, `common`, `actuator`, `plugins`, `example/`.
- Rust unified backend: `rust-backend/` (binary `tron-backend`) providing Storage and Execution over gRPC; configured via `rust-backend/config.toml` (default port 50011).
- Sources: `framework/src/main/java`; tests: `framework/src/test/java`; artifacts: `framework/build/libs`.

## Build, Test, and Run
- Build Java (fast): `./gradlew clean build -x test --dependency-verification=off` → JARs in `build/libs`.
- Build Rust backend: `cd rust-backend && cargo build --release` → `target/release/tron-backend`.
- Start backend: `./target/release/tron-backend` (or `cargo run --release`). Health checks are gRPC-based.
- Run node (mainnet config): `java -jar build/libs/FullNode.jar -c main_net_config_remote.conf`.
- Select storage mode: `STORAGE_MODE=embedded|remote` (or `-Dstorage.remote.host=127.0.0.1 -Dstorage.remote.port=50011`).
- Unit tests: `./gradlew :framework:test` (focus with `--tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest"`).
- Coverage: `./gradlew :framework:jacocoTestReport` (HTML in `framework/build/jacocoHtml`).
- Make targets (optional): `make build`, `make java-test`, `make performance-test`, `make dual-mode-test`.

## Coding Style & Conventions
- Java 8 with Google Java Style enforced by Checkstyle. Prefer `:framework:checkstyleMain` and `:plugins:checkstyleMain`; to skip locally, use `-x checkstyleMain -x checkstyleTest` (not `-x checkstyle`).
- Packages `org.tron.*`; classes PascalCase; methods/fields camelCase; tests end with `*Test.java`.
- Protobuf classes are generated; avoid editing generated sources.

## Testing Guidelines
- Framework: JUnit 4 + Mockito. Keep tests deterministic and isolated from env; clean up system properties/env vars in `@After`.
- Dual-mode focus: validate both embedded and remote storage paths. For remote, ensure backend is running and pass `storage.remote.*` properties.

## Commit & Pull Requests
- Commits: `feat(scope): concise subject` (≤50 chars, imperative, lowercase first letter). Reference issues in footer: `Closes #123`.
- Branches: `feature/...`, `hotfix/...`, `release_*`; merge via PR to `develop`.
- PRs: one issue per PR; include rationale, linked issues, and, when applicable, test output or performance notes. Ensure Checkstyle/tests pass.

## Security & Configuration
- Do not commit secrets or private keys. Prefer config files and local env vars.
- If Gradle dependency verification blocks builds, add `--dependency-verification=off` or refresh with `./gradlew --write-verification-metadata sha256`.
