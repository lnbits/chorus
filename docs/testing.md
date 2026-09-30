# Verification and test coverage

Validated locally on x86_64 Linux on 2026-09-30, with Rust 1.98 and Node.js 24. This is implementation validation, not an independent security audit. See [release gates](security-review.md).

## Core and web

```sh
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
npm ci --prefix apps/web
npm run check --prefix apps/web
npm run lint --prefix apps/web
npm run build --prefix apps/web
```

The Rust suite contains 29 passing tests:

- 15 federation/API integration tests: independently executing 3-node and 10-node groups; one member offline; 100-member state; strict-majority boundaries; an 8/10 to 6/10 quorum change requiring eight old-member approvals; member add/remove and WASM upgrade; stale/forged events; duplicate handling; conflicting results; stateful double-signing refusal; cross-job equivocation and proof withdrawal; pinned-genesis substitution; HTTP authentication/CSRF/logout; NIP-17 recipient validation; real FROST DKG and threshold signing; exported CLI proof; a clean node joining and syncing over two actual local WebSocket relays.
- 7 FedVM tests: deterministic output; malformed bytes/hash mismatch; forbidden imports; infinite loop/fuel exhaustion; memory bomb; oversized output/out-of-bounds pointers; start functions and floating-point rejection.
- 2 real-relay transport tests: multi-relay operation, one relay offline, restart/backfill, and encrypted NIP-17 delivery.
- 2 protocol tests: strict-majority counts across arbitrary member counts and canonical round-trip/domain/trailing-byte validation.
- 3 storage tests: immutable history and signature reservations, encrypted-vault purpose binding/wrong credentials, and disk restart preserving signing guards and append-only history.

Relay tests need permission to bind local sockets. They use disposable relay servers, never live third-party relays. The full 10-node test runs independent engines; the 100-member test checks state and quorum computation, not a 100-machine deployment.

## Browser against a real node

Build the binaries and static frontend first. The runner creates a disposable SQLite directory, generates random test passwords, initializes independent identities, starts a real node on an ephemeral loopback port, runs Playwright, and removes the directory after stopping the node. It does not mock HTTP, execute a fake contract or use production identities.

```sh
cargo build --locked -p chorus-node -p chorus-verifier
npm run build --prefix apps/web
(cd apps/web && npx playwright install chromium)
python3 scripts/test-browser.py
```

On NixOS, use the system browser:

```sh
CHORUS_CHROME=/run/current-system/sw/bin/google-chrome python3 scripts/test-browser.py
```

The three passing browser tests cover:

1. Public navigation, light/dark mode, desktop/mobile rendering and horizontal overflow.
2. Login, federation creation, real WASM execution, finalized proof download, CLI signature verification and independent re-execution, governance proposal/approval/epoch activation, settings, CSRF rejection, absence of exposed nsec and logout.
3. The node's strict script CSP and externalized static bootstrap.

For an already-running **disposable** node, set `CHORUS_E2E_URL`, `CHORUS_E2E_USERNAME`, `CHORUS_E2E_PASSWORD`, and optionally `CHORUS_CHROME`, then run `npm run test:e2e --prefix apps/web`. These tests create federation history that cannot be deleted through the UI.

## Desktop and packaging

The Tauri target was checked and linted on Linux using the Nix GTK/WebKit development environment:

```sh
nix develop --command cargo clippy --locked --manifest-path apps/desktop/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path apps/desktop/Cargo.toml -- --check
```

This does not certify installed desktop runtime behavior. Windows/macOS builds, code signing, packaging, Docker deployment and complete Nix package builds remain platform release checks. The manual desktop CI workflow includes a three-platform build matrix.

## Dependency audits and fuzzing

```sh
cargo audit
cargo audit --file apps/desktop/Cargo.lock
npm audit --prefix apps/web
cargo check --locked --manifest-path fuzz/Cargo.toml --bins
```

The node audit has zero vulnerability advisories, with a recorded unmaintained `atomic-polyfill` transitive dependency. The frontend audit has zero vulnerabilities. The desktop audit also has zero vulnerability-class advisories but reports an upstream `glib` unsoundness warning and unmaintained `proc-macro-error`. These are unresolved findings, not ignored checks; details are in the security review.

Five compiled cargo-fuzz targets cover canonical decoding, Nostr parsing, governance messages, jobs and execution envelopes. Run sustained campaigns with a nightly sanitizer-capable toolchain:

```sh
cargo install cargo-fuzz --locked
cargo +nightly fuzz run canonical
cargo +nightly fuzz run nostr
cargo +nightly fuzz run governance
cargo +nightly fuzz run job
cargo +nightly fuzz run envelope
```

No sustained fuzz campaign, cross-architecture determinism campaign, private multi-node outage lifecycle or external penetration test has been completed. The real FROST test establishes library interoperability and nonce consumption for one ceremony/signing session; it does not establish operational distributed coordinator safety.
