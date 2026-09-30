# Chorus

**Run the same contract. Agree on the same result.**

Chorus is a Rust federation node with a SvelteKit interface and a Tauri 2 desktop package. Members independently execute deterministic WASM, sign versioned canonical commitments, and accept matching attestations under a federation-defined threshold. Nostr is the identity and communications layer. There is no blockchain, mining, gas or token.

**Release status: pre-production.** The executable member-attestation path works end to end, including real multi-relay tests and independent verification. This repository is not yet cleared to coordinate valuable actions. In particular, FROST has a real DKG/signing/verification library and tests, but does **not** yet have an operational distributed ceremony/session coordinator or encrypted share lifecycle in the node. See [release gates and security review](docs/security-review.md). No fabricated FROST signature or consensus fallback is used.

## Downloads

Pushing a version tag such as `v0.1.0` runs the [Linux release workflow](.github/workflows/release.yml). Once it passes, the [GitHub release](https://github.com/lnbits/chorus/releases) contains an x86_64 AppImage, a server archive with the built web UI, and SHA-256 checksums. Neither package needs Cargo or npm to run. See [download and release instructions](docs/deployment.md#linux-release-packages).

## Run locally

Requirements: Rust 1.98+, Node.js 24+, a C toolchain. SQLite is bundled. Docker is optional.

For a fresh Ubuntu VPS without Cargo or npm, first follow the [build-tool installation instructions](docs/deployment.md#vps-without-docker).

```sh
cargo build --locked -p chorus-node -p chorus-verifier
npm ci --prefix apps/web
npm run build --prefix apps/web

# Interactive, non-echoed password prompts. Creates one Nostr identity and a separate admin login.
./target/debug/chorus-node init --username admin
./target/debug/chorus-node serve --web-root apps/web/build
```

Open **http://127.0.0.1:8787** and sign in. New nodes start with six enabled read/write relays: `relay.nostr.com`, `relay.damus.io`, `nos.lol`, `nostr.bitcoiner.social`, `nostr.mom`, and `relay.snort.social`, all using `wss://`. Manage them in Settings → Relays. Saved relay choices are preserved across restarts. The initial node has no sample federations, hard-coded keys, default passwords or invented activity.

For frontend development, run the node with `--origin http://127.0.0.1:5173`, then `npm run dev --prefix apps/web`. Vite proxies the API; use precisely that origin for CSRF validation.

To try a contract, upload [`examples/echo.wasm`](examples/echo.wasm), include your npub from Settings, and choose a threshold appropriate to the actual member set. The example is a real stateless byte-echo contract, not a mock node. Rebuild it with:

```sh
cargo run -p chorus-fedvm --example build_contract
```

Use an existing Nostr key with `chorus-node init --import-nsec`, or stop an existing node and run `chorus-node replace-key` to replace its encrypted nsec. Both use hidden prompts. Key replacement preserves an encrypted backup and requires explicit acknowledgement when the old npub is still a federation member. See [identity and key replacement](docs/operations.md).

## Federation workflow

1. Create with initial member npubs, threshold, exact WASM, visibility, relay set and optional state.
2. Export an invitation. Exchange its genesis hash independently through a trusted channel.
3. Other nodes import the invitation and pin that hash. Joining local storage does not add membership.
4. A member submits a job. Each connected member verifies the epoch, input and contract, runs it, and signs its own result.
5. Matching commitments count toward the configured threshold. Conflicting signatures remain evidence.
6. Download an accepted job’s proof bundle and verify on a separate machine:

```sh
fedwasm verify result.json --genesis-hash <trusted-hash> --rerun
```

Quorums are explicit counts, such as **6/10**, and must be strictly greater than half the member set. All quorum and membership changes are signed governance events and must pass under the **current** quorum before a new epoch activates. The creator has no special privileges after genesis.

Threshold attestation is not a total-order consensus protocol. Even strict-majority quorums can certify incompatible outcomes if their overlapping members equivocate. Detected conflicts freeze stateful advancement; a certificate does not establish that no other certificate exists. Read the [trust model](docs/protocol.md).

## Project map

| Path | Responsibility |
| --- | --- |
| `crates/protocol` | Versioned types, Borsh encoding, hashing, thresholds |
| `crates/federation` | Nostr signature and member validation, pinned genesis |
| `crates/governance` | Old-epoch approvals and immutable successor states |
| `crates/fedvm` | Restricted Wasmtime profile and byte ABI |
| `crates/consensus` | Commitment grouping, equivocation, accepted proofs |
| `crates/nostr` | Maintained Nostr SDK, multiple relays and NIP-17 |
| `crates/storage` | SQLite migrations, immutable records, key vaults and auth |
| `crates/frost` | Real FROST-Ed25519 DKG primitives and single-use signing rounds |
| `crates/api` | Versioned HTTP API and execution-coupled engine |
| `crates/node` | Standalone service; independent of the UI |
| `crates/verifier` | `fedwasm` independent verification CLI |
| `apps/web` | Responsive, monochrome SvelteKit administration/public UI |
| `apps/desktop` | Native initialization/unlock and a packaged node sidecar |

## Checks

```sh
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
npm run check --prefix apps/web
npm run lint --prefix apps/web
npm run build --prefix apps/web
cargo audit
npm audit --prefix apps/web
```

Relay integration tests bind loopback sockets. Browser tests require a disposable running node; see [testing](docs/testing.md). [Architecture](docs/architecture.md), [protocol/governance/trust](docs/protocol.md), [WASM ABI](docs/fedvm.md), [FROST](docs/frost.md), [deployment](docs/deployment.md), [backup and keys](docs/operations.md), and [security review](docs/security-review.md) describe the implementation and its limits.
