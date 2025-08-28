# Repository Guidelines

## Project Structure & Module Organization
- `framework`: Node runtime and distributions. JARs (e.g., `FullNode.jar`) end up in `build/libs`. Sources in `framework/src/main/java`; tests in `framework/src/test/java`.
- Core modules: `protocol` (protobuf), `consensus`, `crypto`, `chainbase`, `common`, `actuator`.
- Extensions: `plugins`; examples in `example/actuator-example`.
- Rust backend: `rust-backend` (optional gRPC backend, binary `tron-backend`).
- Supporting dirs: `config/`, `proto/`, `scripts/`, `docker/`, `output-directory/`, `logs/`.

## Build, Test, and Development Commands
- Build (Java): `./gradlew clean build -x test --dependency-verification=off` → JARs in `build/libs/`.
- Build (Rust backend): `cd rust-backend && cargo build --release` → `target/release/tron-backend`.
- Run node: `java -jar build/libs/FullNode.jar -c main_net_config.conf`.
- Lint/Style: `./gradlew :framework:checkstyleMain :plugins:checkstyleMain` (or `:framework:lint`).
- Unit tests: `./gradlew :framework:test` (targeted: `--tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest"`).
- Coverage: `./gradlew :framework:jacocoTestReport` (HTML in `framework/build/jacocoHtml`).
- Remote storage tests: start backend (`./target/release/tron-backend`, default port 50011), then:
  `./gradlew :framework:test --tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest.testRemoteStorageMode" -Dstorage.remote.host=127.0.0.1 -Dstorage.remote.port=50011`.

## Coding Style & Naming Conventions
- Java 8; follow Google Java Style (enforced via Checkstyle). Run the tasks above before pushing.
- Packages `org.tron.*`; classes PascalCase; methods/fields camelCase; tests end with `*Test.java`.
- Lombok is used where present; avoid one-letter identifiers and keep methods small and focused.
- Protobuf stubs are generated during build; do not edit generated sources.

## Testing Guidelines
- Framework: JUnit 4 + Mockito. Place tests under `module/src/test/java` and name by behavior, e.g., `StorageSPITest`.
- Prefer deterministic tests; isolate I/O; use `--tests` to run focused suites during iteration.

## Commit & Pull Request Guidelines
- Commits: `feat(scope): concise subject` (≤50 chars, imperative, lowercase first letter). Example: `fix(protocol): handle empty contract name`. Reference issues in footer: `Closes #123`.
- Branches: `feature/...`, `hotfix/...`, `release_*`. Merge to `develop` via PR.
- PRs: one issue per PR; clear description, linked issues, and rationale; avoid oversized diffs; ensure Checkstyle/tests pass.

## Security & Configuration Tips
- Never commit private keys or secrets. Use local env/config (e.g., `main_net_config.conf`); keep secrets out of VCS.
- If dependency verification blocks builds, use `./gradlew --dependency-verification=off` locally or update metadata with `./gradlew --write-verification-metadata sha256`.
