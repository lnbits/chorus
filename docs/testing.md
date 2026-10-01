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

The Rust suite contains 32 passing tests:

- 16 federation/API integration tests: independently executing 3-node and 10-node groups; one member offline; 100-member state; strict-majority boundaries; an 8/10 to 6/10 quorum change requiring eight old-member approvals; member add/remove and WASM upgrade; stale/forged events; duplicate handling; conflicting results; stateful double-signing refusal; cross-job equivocation and proof withdrawal; pinned-genesis substitution; HTTP authentication/CSRF/logout; NIP-17 recipient validation; real FROST DKG and threshold signing; exported CLI proof; a clean node joining and syncing over two actual local WebSocket relays, for both public and private federations using one identity per node.
- 7 FedVM tests: deterministic output; malformed bytes/hash mismatch; forbidden imports; infinite loop/fuel exhaustion; memory bomb; oversized output/out-of-bounds pointers; start functions and floating-point rejection.
- 3 real-relay transport tests: multi-relay operation, one relay offline, restart/backfill, encrypted NIP-17 delivery, and legacy inbox decryption with outgoing messages authenticated by the current identity.
- 2 protocol tests: strict-majority counts across arbitrary member counts and canonical round-trip/domain/trailing-byte validation.
- 1 CLI identity lifecycle test: imported nsec, one-vault initialization, running-node exclusion, wrong password/invalid key rejection, membership-loss guard, encrypted backup, replacement, unchanged history/reservations and no secret output.
- 3 storage tests: immutable history and signature reservations, encrypted-vault purpose binding/wrong credentials, and disk restart preserving signing guards and append-only history.

Relay tests need permission to bind local sockets. They use disposable relay servers, never live third-party relays. The full 10-node test runs independent engines; the 100-member test checks state and quorum computation, not a 100-machine deployment.

## Browser against a real node

Build the binaries and static frontend first. The runner creates a disposable SQLite directory, generates random test passwords, initializes one Nostr identity per node, starts a real node on an ephemeral loopback port, runs Playwright, and removes the directory after stopping the node. It does not mock HTTP, execute a fake contract or use production identities.

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

This does not certify installed desktop runtime behavior. The Linux release workflow builds an AppImage and server archive on Ubuntu 22.04, smoke-tests the extracted server (initialization, unlock, health, SPA routes and JavaScript assets), and checks the AppImage's sidecar/resource layout before publication. Run its artifact smoke test locally with Python 3.12+:

```sh
python3 scripts/test-linux-package.py dist/chorus-0.1.0-linux-x86_64.tar.gz
python3 scripts/test-appimage.py dist/Chorus_0.1.0_amd64.AppImage
```

The AppImage test unsets display variables, exercises headless initialization and startup on an explicit wildcard host/port, checks browser-origin enforcement and Secure cookies, and rejects invalid flags before any GUI is created. The same headless test also passed in a minimal Ubuntu 22.04 container with only the documented runtime libraries and Python for the test harness; GTK/WebKit development packages and a display server were absent. A virtual-display test additionally unlocked the desktop launcher and verified its bundled node at an explicitly selected `127.0.0.1:18787`. Launcher unit tests cover port defaults, remote HTTPS requirements, malformed origins and occupied ports.

The tests use temporary credentials and disables default public relays in its disposable database before startup. It cleans up the process and data on exit. A local Ubuntu 22.04 container with a virtual display also opened the release AppImage, unlocked a disposable keystore, started its bundled node and displayed its packaged web UI. Network access was disabled for that graphical test. Broader interactive desktop behavior, Windows/macOS builds, code signing, Docker service deployment and complete Nix package builds remain platform release checks. The manual desktop CI workflow retains a three-platform build matrix. See [release packaging](deployment.md#linux-release-packages).

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
