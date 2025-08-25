# ADR-012: Unified Rust Backend Architecture

## Status
**ACCEPTED** - 2025-01-08

## Context

Java-tron currently has a dual storage mode implementation (feat #11) where storage operations can be handled by either:
- EMBEDDED mode: Local RocksDB within the JVM
- REMOTE mode: External Rust storage service via gRPC

We now need to add EVM execution capabilities to the Rust side, which raises the architectural question: should we maintain separate Rust processes for storage and execution, or unify them into a single backend process?

## Decision

We will implement a **unified Rust backend** that combines both storage and execution modules into a single process, with the following architecture:

```
JVM (java-tron core) ⇄ gRPC/IPC ⇄ Rust-Backend (unified process)
                                      ├── Storage Module (RocksDB)
                                      ├── Execution Module (revm-based)
                                      └── Future Modules (consensus, p2p, etc.)
```

## Rationale

### Performance Benefits
- **Zero serialization cost** between execution and storage (critical for >2000 TPS)
- **Single gRPC channel** between JVM and Rust, reducing connection overhead
- **Atomic semantics** - EVM execution can call storage within the same process/transaction

### Operational Simplicity
- **One service to deploy** instead of managing multiple Rust processes
- **Simplified connection pooling** and health monitoring
- **Unified logging and metrics** collection

### Implementation Efficiency
- **Reuse existing storage crate** by embedding it directly
- **Avoid distributed transaction complexity** between separate services
- **Faster development** with less IPC plumbing

### Future Extensibility
- **Modular plugin architecture** allows adding consensus, p2p, etc. as feature-gated crates
- **Unified gRPC surface** can evolve to support new modules
- **Clear migration path** toward full Rust node implementation

## Technical Decisions

### EVM Engine Choice
- **revm**: High-performance Rust EVM implementation
- **Tron-specific extensions**: Energy accounting, native TRC token support
- **Upstream tracking**: Maintain fork with regular upstream rebases

### Atomicity Model
- **Fully delegated execution**: Entire ExecuteTx flow runs inside Rust backend
- **No two-phase commit**: JVM only sees final execution outcome
- **Transaction isolation**: Each execution runs in isolated storage context

### Module Architecture
- **Plugin trait system**: Uniform lifecycle API for all modules
- **Feature flags**: Compile-time inclusion/exclusion of modules
- **Extensible proto**: New modules register additional gRPC services

### Transport Layer
- **gRPC/protobuf**: Consistent with existing storage implementation
- **Semantic versioning**: Proto packages support backward compatibility
- **Health checks**: Unified health endpoint for all modules

## Implementation Plan

1. **Phase 0**: ADR approval and proto design (0.5 weeks)
2. **Phase 1**: Backend skeleton with embedded storage (1 week)
3. **Phase 2**: REVM integration with Tron extensions (2 weeks)
4. **Phase 3**: Java client and configuration (1 week)
5. **Phase 4**: End-to-end testing and benchmarking (2 weeks)
6. **Phase 5**: Extensibility hooks and module system (1 week)
7. **Phase 6**: Production hardening and deployment (1 week)

**Total: ~8 weeks**

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Larger crash blast-radius | HIGH | Health-probe watchdog + quick restart; Module trait allows future splitting |
| revm fork divergence | MEDIUM | Git submodule tracking; CI rebase alerts |
| Energy/TRC logic bugs | HIGH | Port reference tests from existing JVM engine |
| Module API complexity | LOW | Enforce ADR for new modules; maintain proto compatibility |

## Alternatives Considered

### Option A: Separate Rust Services
- **Pros**: Maximum fault isolation, independent scaling
- **Cons**: Double network overhead, distributed transaction complexity
- **Verdict**: Rejected due to performance impact on hot path

### Option C: Three-Process Hybrid
- **Pros**: Keeps storage reusable, removes JVM from execution path
- **Cons**: Highest operational complexity, still requires distributed transactions
- **Verdict**: Rejected due to operational overhead

## Success Metrics

- **Performance**: ≤1.5× latency overhead vs embedded EVM, ≥1000 TPS
- **Reliability**: 99.9% uptime with graceful failure handling
- **Extensibility**: New modules can be added without breaking changes
- **Operational**: Single deployment artifact, unified monitoring

## References

- [feat: add storage ipc (#11)](https://github.com/0xNebulaLumina/java-tron/pull/11)
- [revm GitHub Repository](https://github.com/bluealloy/revm)
- [Existing Storage SPI Design](./storage-spi-design.md) 