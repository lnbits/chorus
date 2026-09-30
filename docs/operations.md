# Identity, keys, backup and recovery

The admin account is separate from one persistent **Nostr identity**. That identity signs federation jobs, results and governance votes, and sends/receives encrypted private messages. The same npub is used when joining every federation; joining does not create another key. FROST shares, when available, are distinct threshold-signing material rather than another Nostr account.

The Nostr key is encrypted in `member.vault` using a random salt, Argon2id and XChaCha20-Poly1305 with purpose-bound authenticated data. The filename and `member` purpose are retained for compatibility. Unix files are mode 0600 and the data directory is 0700. Password/key buffers are zeroized where possible; a compromised running process or OS can still read unlocked keys. Windows ACL hardening needs native platform review.

## Create, import or replace your nsec

New nodes generate one key by default. To import your existing nsec during setup:

```sh
chorus-node init --username admin --import-nsec
```

The command prompts without echoing the nsec or passwords. The desktop bootstrap also offers **Generate a new key** or **Import my nsec**. The normal HTTP settings API never receives or exports private keys.

To replace the nsec in an initialized node, **stop the running node first**, then use:

```sh
chorus-node replace-key
# For a non-default data directory:
chorus-node --data-dir /path/to/data replace-key
```

Enter the existing keystore password and replacement nsec at the hidden prompts. Chorus validates both, writes an encrypted `member-backup-<old-pubkey>-<random>.vault` backup, then atomically replaces `member.vault`. It retains the same keystore password, admin account, relays, signed federation history and all signing reservations. A process lock prevents replacement while an updated node is running. Older versions did not take this lock, so stop them explicitly before upgrading or changing keys.

If any known current federation still lists the old npub, replacement refuses by default and shows the proposed new npub. Coordinate admission of the new key and removal of the old key under each federation's current quorum. If you intentionally want to switch before that process completes, `replace-key --allow-membership-loss` explicitly acknowledges that the new key cannot act as the old member. Importing an invitation alone does not grant membership. A key change is not a governance bypass and does not rotate a federation's FROST group key.

Automation may pass `{ "key_password": "…", "nsec": "…" }` through a protected pipe to `replace-key --credentials-stdin`. Initialization accepts an optional `nsec` field in its credential JSON. Do not put private keys in CLI arguments, environment variables, logs or committed files.

## Compatibility with existing two-key nodes

Upgrading keeps the existing **member** nsec/npub and its signing reservations. It becomes the single active signing and outgoing messaging identity. No signed epoch, membership list or trust anchor is rewritten.

If an old `communication.vault` exists, it is retained as a **receive-only legacy inbox**, so old private envelopes and federations with old routing remain readable. New nodes do not create that file. New federation states automatically map each member to the same npub for private delivery. Existing private federations can use **Governance → Use member identities for private messaging** to approve the routing change under their current quorum; members should upgrade before approving. The old inbox key is never used for outgoing messages. Retain it in backups for historical decryption.

There is no HTTP nsec/share export endpoint. Private job content is encrypted in transit but stored in the node's SQLite database in plaintext. Use full-disk encryption where required. A private relay designation is a configuration label; encryption is determined by signed federation visibility and applies even on public relay infrastructure.

## Backup

Stop the node and copy the **entire data directory**, including SQLite, WAL/SHM files if present, `member.vault`, and any legacy/backup vaults. Alternatively use SQLite's online backup API with a coordinated snapshot of the vaults. Do not copy just the main database file while WAL writes are active. Protect the unlock credential separately from the backup.

Immutable signing reservations are as important as keys. Restoring an older snapshot and reusing the same signing identity can lose reservations and allow equivocation. Do not operate simultaneous clones of a member identity. This release has no remote monotonic signing counter or rollback-proof hardware. Before recovering a member from a stale backup, remove/rotate its identity under federation governance, or obtain an audited complete signing history and reconciliation process. Do not delete the `signing_guards` table to unstick a job.

A fresh node with new keys can import a pinned invitation and verify/backfill public history without impersonating an existing member. Membership of its new key requires governance. Private recovery additionally requires the key to which the messages were addressed and availability of encrypted envelopes.

## Conflicts and liveness

Conflicting certificates/equivocation remain append-only evidence. Stateful/epoch conflicts quarantine the federation; the administrator cannot silently pick a winner. This release does not include an automatic quarantine-resolution protocol. Export relevant database/evidence, preserve the old identities/history and agree on an explicit recovery federation through a trusted process. Merely lowering the UI threshold is not recovery.

A per-sequence signature reservation or a per-epoch proposal reservation can block progress if a job expires or members split their votes. This is a deliberate safety-over-liveness policy. A secure cancellation/view-change protocol remains a release gate.

## Observability

`/healthz` checks the database and returns node/protocol/VM liveness. Authenticated settings return SDK relay connection states. Activity includes federation creation/import, proposal/epoch changes, accepted/disputed jobs and execution failures. Logs are structured JSON and do not include credentials, nsecs, shares, session tokens or private job payloads.

Complete Prometheus metrics, durable per-relay last-success timestamps, normalized failure counters, retention/quotas and operational alerting are not implemented. Liveness is not evidence that all relays are reachable or every federation is making progress.
