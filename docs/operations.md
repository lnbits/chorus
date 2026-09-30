# Identity, keys, backup and recovery

There are three security domains:

- **Admin**: username/password verifier and expiring sessions in SQLite. Passwords use Argon2id with 64 MiB memory, three iterations and random salt. Authentication is independent of federation membership.
- **Communication identity**: a Nostr secret key used to encrypt/decrypt private envelopes. The UI sees only its npub.
- **Member signing identity**: a different Nostr secret key for federation attestations and governance. The node only signs results returned by verified execution. The current implementation uses one member identity across the node's federations; it does not provide a separate per-federation key vault.

Communication/member keys are separate encrypted `.vault` files: versioned format, fresh 16-byte salt, Argon2id key derivation, XChaCha20-Poly1305 with fresh 24-byte nonce, and purpose-bound associated data. Unix files are mode 0600 and the data directory is 0700. Wrong passwords, modified ciphertext or purpose swaps fail authentication. Password/key buffers are zeroized where possible; a compromised running process or OS can still read unlocked keys. Windows ACL hardening needs native platform review.

There is no HTTP nsec/share export endpoint. Private job content is encrypted in transit but stored in the node's SQLite database in plaintext. Use full-disk encryption where required. A private relay designation is a configuration label; encryption is determined by signed federation visibility and applies even on public relay infrastructure.

## Backup

Stop the node and copy the **entire data directory**, including SQLite, WAL/SHM files if present, and both key vaults. Alternatively use SQLite's online backup API with a coordinated snapshot of the vaults. Do not copy just the main database file while WAL writes are active. Protect the unlock credential separately from the backup.

Immutable signing reservations are as important as keys. Restoring an older snapshot and reusing the same signing identity can lose reservations and allow equivocation. Do not operate simultaneous clones of a member identity. This release has no remote monotonic signing counter or rollback-proof hardware. Before recovering a member from a stale backup, remove/rotate its identity under federation governance, or obtain an audited complete signing history and reconciliation process. Do not delete the `signing_guards` table to unstick a job.

A fresh node with new keys can import a pinned invitation and verify/backfill public history without impersonating an existing member. Membership of its new key requires governance. Private recovery additionally requires the intended recipient communication key and availability of encrypted envelopes.

## Conflicts and liveness

Conflicting certificates/equivocation remain append-only evidence. Stateful/epoch conflicts quarantine the federation; the administrator cannot silently pick a winner. This release does not include an automatic quarantine-resolution protocol. Export relevant database/evidence, preserve the old identities/history and agree on an explicit recovery federation through a trusted process. Merely lowering the UI threshold is not recovery.

A per-sequence signature reservation or a per-epoch proposal reservation can block progress if a job expires or members split their votes. This is a deliberate safety-over-liveness policy. A secure cancellation/view-change protocol remains a release gate.

## Observability

`/healthz` checks the database and returns node/protocol/VM liveness. Authenticated settings return SDK relay connection states. Activity includes federation creation/import, proposal/epoch changes, accepted/disputed jobs and execution failures. Logs are structured JSON and do not include credentials, nsecs, shares, session tokens or private job payloads.

Complete Prometheus metrics, durable per-relay last-success timestamps, normalized failure counters, retention/quotas and operational alerting are not implemented. Liveness is not evidence that all relays are reachable or every federation is making progress.
