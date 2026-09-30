# FedVM deterministic profile and ABI

Profile identifier: `fedvm/1-wasmtime48.0.3`. The exact engine release is pinned; changing it requires a new profile and governance-approved epoch. Wasmtime is compiled without general WASI, threads or GC support. Validation allows an integer-only WASM1 subset with sign extension and bulk memory. Floating-point types/operations, start functions, imports, SIMD, shared memory, memory64 and newer unapproved proposals are rejected.

| Resource | Bound |
| --- | --- |
| WASM file | 4 MiB |
| Canonical input | 256 KiB |
| Canonical output | 256 KiB |
| Linear memory | 16 MiB |
| Execution fuel | 10,000,000 |
| WASM stack | 256 KiB |
| Tables / elements | 1 / 4,096 |
| Functions | 10,000 |
| Secondary execution deadline | Approximately 5 seconds, epoch interruption |
| Concurrent executions | 2 per node |

Every execution uses a fresh instance/store. Import validation precedes compilation. The content hash is checked on the same bytes compiled. Fuel covers guest allocation and execution. Output size and memory bounds are checked before allocation/copy. The runtime exposes no filesystem, network, randomness, clock, environment, subprocesses or arbitrary host functions.

Fuel is deterministic for the pinned engine/profile, but cross-architecture conformance must still be established before production. An in-process compilation phase has no hard wall-clock/OS kill deadline yet. Deployment memory limits provide another layer, not a substitute for an isolated worker supervisor. See the release gates.

## Guest exports

```text
memory: exported linear memory
alloc(length: i32) -> i32
execute(input_pointer: i32, input_length: i32) -> i64
```

The packed i64 return is `(output_pointer as u64 << 32) | output_length as u64`. Pointers are unsigned WASM32 offsets. The host copies input into guest memory after `alloc`, calls `execute`, reads exactly the returned bytes, then drops the entire instance. No guest free function is needed.

Input is the canonical `InputEnvelope`:

- `previous_state: Option<Vec<u8>>`
- `command: Vec<u8>`
- `attestations: Vec<Vec<u8>>`

Output is the canonical `OutputEnvelope`:

- `result: Vec<u8>`
- `next_state: Option<Vec<u8>>`
- `declared_actions: Vec<Vec<u8>>`

The same canonical domain/version prefix used elsewhere precedes Borsh encoding. Stateful inputs and outputs require matching optional state fields. Use integer or explicit fixed-point financial arithmetic. A guest may return declared actions as bytes, but this host does not execute them.

[`examples/echo.wat`](../examples/echo.wat) is a complete stateless example with comments documenting byte offsets. Compile it with `cargo run -p chorus-fedvm --example build_contract`. The resulting WASM copies the supplied command into the canonical result; the browser integration test submits actual text and checks independently reproduced output.

Upstream: [Wasmtime determinism](https://docs.wasmtime.dev/examples-deterministic-wasm-execution.html), [Wasmtime security](https://docs.wasmtime.dev/security.html).
