# Security review and production release gates

Review date: 2026-09-30. This is an implementation self-review, **not an independent security audit or a production certification**.

## Boundaries reviewed

| Boundary | Implemented controls | Remaining concern |
| --- | --- | --- |
| Canonical protocol | Typed/versioned/domain-separated Borsh, bounded decode, exact hash binding, sorted members | Cross-language conformance vectors and long-running fuzzing |
| Governance | Current-epoch member signatures, old threshold, immutable history, durable one-successor reservation | No view-change/cancellation recovery; partition liveness; certificate withholding |
| Attestations | Exact commitment grouping, signer deduplication, equivocation evidence, no arbitrary signing API | Strict majority alone is insufficient for Byzantine quorum intersection; rollback/cloned keys defeat local reservations |
| Stateful jobs | Derived head, previous-state/sequence binding, cross-epoch signing guard, conflict quarantine | Not a replicated total-order service; recovery after splits is manual |
| Guest execution | No imports/WASI, integer profile, fuel/memory/output bounds, fresh store, secondary execution deadline | JIT/compiler is in-process; compilation not externally killable; architecture reproducibility needs independent validation |
| Nostr | Maintained SDK verification, inner member signatures, multiple relays, durable event dedupe, outbox, real NIP-17 | Bounded archival resync; event ordering/dependency retries; hostile relay/member throughput and storage growth |
| FROST | Maintained library, DKG and real threshold-signature tests, accepted-proof gate, single-use nonce wrapper | Distributed ceremony/coordinator, encrypted share lifecycle and durable crash/dropout protocol are absent |
| Admin HTTP | Argon2id, persistent exponential account backoff, HttpOnly/SameSite/Secure cookies, CSRF/Origin, expiry, strict CSP | Account-global backoff permits denial of service; endpoint-specific quotas and native deployment review |
| Key custody | Separate communication/member vaults, authenticated encryption, purpose binding, local-only unlock | No OS keychain/hardware signer; Windows ACL validation; process compromise; one member key shared across federations |
| Database | Bound SQL parameters, WAL/FULL, append-only history/reservations, content-addressed blobs | No rollback-proof external counter; plaintext private data; migrations/query scale and transaction recovery need more coverage |
| Desktop | Separate node process, piped credentials, no remote IPC privileges | Only compile-checked on Linux; installer/runtime, macOS/Windows, crash/restart and single-instance behavior need testing |

## Findings addressed during implementation

- Explicit quorums must exceed half the member set; boundaries are tested across arbitrary member counts.
- Threshold changes are certified under the old member set/rule.
- Duplicate responses cannot inflate agreement; mismatched commitment fields fail.
- Equivocation and multiple threshold groups suppress acceptance.
- Competing epoch/stateful sequence certificates are retained and quarantine execution.
- Guest start functions/imports/floats and oversized results fail before signing.
- Local signature decisions are committed to SQLite before generating signatures.
- Genesis is explicitly pinned; event signatures alone are not a trusted federation bootstrap.
- The static bootstrap was externalized so a strict script CSP works without unsafe inline code.
- Nostr was upgraded from 0.44 to maintained 0.45.4 after the audit identified its retired relay-pool crate and a transitive LRU memory-safety advisory.
- The transitive `cookie` package was overridden to a patched release after npm audit; the UI is statically served by Rust and does not run SvelteKit's server cookie handling.

## Dependency review

The core Cargo lockfile audit reports **zero vulnerability advisories** after the SDK upgrade. It still reports the transitive `atomic-polyfill` unmaintained notice (RUSTSEC-2023-0089), through the FROST serialization dependency graph. This is recorded, not suppressed. The application must monitor maintained FROST releases that remove that dependency. The frontend npm audit reports zero vulnerabilities after the cookie override.

The desktop lockfile has zero vulnerability-class advisories, but reports **RUSTSEC-2024-0429**, unsoundness in the upstream GTK binding’s `glib::VariantStrIter`, and **RUSTSEC-2024-0370**, unmaintained `proc-macro-error`. These remain unresolved in Tauri’s Linux GTK dependency graph; desktop is not cleared for production. Its native platform status must be considered separately. A clean audit does not prove the absence of exploitable bugs or defects in integration code. Re-run both audits at release time.

## Blocking release gates

1. Implement and externally review distributed FROST setup/signing orchestration and share storage before advertising compact threshold signing as operational.
2. Establish tested stateful ordering/cancellation/recovery rules. Define Byzantine and quorum assumptions explicitly for each federation; strict majority is not a complete Byzantine consensus protocol.
3. Move untrusted compilation/execution behind a supervised process boundary with hard OS limits and robust cancellation. Run deterministic conformance across supported architectures/versions.
4. Implement durable paginated public/private archival resynchronization, dependency buffering, relay acknowledgements, spam quotas, database retention and complete operational metrics.
5. Exercise private multi-node federation lifecycle under outages/restart, production-size histories, key rotation, stateful epoch changes and crash injection. Existing encryption and relay tests do not cover all of these.
6. Complete native key custody, installed desktop runtime tests, signed platform builds, backup-rollback and cloned-key protections.
7. Run sustained coverage-guided fuzz campaigns with sanitizers and an independent review of protocol, Wasmtime, Nostr, HTTP, storage and FROST boundaries.

No external-effect adapter is implemented. Do not use an accepted certificate alone to authorize valuable irreversible actions before these gates and the federation's application-specific trust review are satisfied.
