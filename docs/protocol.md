# Protocol, federation lifecycle and trust

## Canonical representation

All consensus-critical values use the explicit Rust types in `chorus-protocol`. Canonical bytes are:

```
ASCII "CHORUS\0" | u16 little-endian version (1)
| u16 little-endian domain-byte-length | UTF-8 domain
| Borsh-encoded typed value
```

SHA-256 hashes those exact bytes. Raw WASM, state and output blobs use SHA-256 of the exact bytes. Hashes and public keys are lowercase 64-character hexadecimal strings inside typed messages. Struct field order, enum discriminants, optional values, lengths and integer endianness follow Borsh. Members and relay lists must be sorted and unique; unordered maps and floating-point protocol fields are excluded. Decoding rejects wrong domains/versions, trailing bytes and oversized messages. A wire change requires a protocol version change; do not reorder Rust fields casually.

The JSON API is never hashed as a substitute for this encoding. Inner Nostr events in certificates are serialized signed event strings; their Nostr IDs/signatures are independently verified and the typed contents decoded again.

## Genesis and membership

Genesis pins a random federation ID, epoch zero, initial members, threshold, approved WASM, VM version, optional initial execution state, relay set and privacy. A genesis member signs it. Genesis is a **trust anchor**, not proof of pre-existing collective approval. Every joining user must independently pin the exact genesis state hash. A matching federation name, ID or relay advertisement is insufficient.

Joining imports verified state and bytes locally. It does not enroll the local node as a member. The existing federation approves enrollment through an epoch transition. Each node uses one Nostr identity for federation votes and private envelopes. New states automatically map each member to its own npub. The canonical `communication_keys` field is retained so historical two-key states and their signatures remain verifiable. Changing an existing mapping requires a governance-certified epoch.

An epoch change is a proposal for the complete successor state. Its parent hash must match, epoch must increment by one, initial execution state is immutable, and all fields are validated. Approval commits to protocol, federation, old epoch, parent hash and proposal hash. Unique old members must reach the **old rule**, including when reducing a threshold, removing members or changing the contract. Historical states and signatures are immutable. Membership/threshold changes invalidate old FROST configuration unless a fresh setup is supplied.

## Threshold rules and limits of agreement

A quorum is an explicit signature count `k`, with `floor(n/2) + 1 <= k <= n`. Display it as `k/n`, for example **6/10** or **2/3**. Zero, half-or-less, fractional, percentage, and above-member-count values are invalid. Member counts are not fixed. A quorum change is a signed proposal event certified by approvals from the current member set under the current quorum. For example, changing **8/10** to **6/10** requires eight current members; six applies only after the certified next epoch activates. A membership proposal must include a valid strict-majority quorum for its proposed member set.

**Strict majority is not Byzantine total-order consensus.** Two valid size-k quorums intersect because `2k > n`, but safety requires enough overlapping members to be honest and preserve signing reservations. A common uniqueness condition under at most f Byzantine members is `2k > n+f`; it is a trust assumption, not a feature that makes arbitrary thresholds safe.

Chorus detects competing epoch certificates, incompatible results and equivocation. Stateful conflicts quarantine the federation. It cannot detect evidence withheld by censored relays, and cannot revoke an external irreversible action already taken on an earlier certificate. No automatic external-action adapter is provided. Conservative per-epoch/per-sequence signing reservations can stall a federation after conflicting proposals or failed jobs; there is no unsafe local override.

## Jobs and accepted results

A job pins the full federation-state hash as well as federation ID, epoch, WASM, VM, canonical input, input hash, nonce, creation/expiry and optional sequence/previous state. Job ID is the canonical job digest. Stateful sequences span epochs; contract upgrades do not reset them.

An attestation binds protocol version, federation ID, epoch, federation-state hash, job ID, sequence, VM version, WASM hash, previous state hash, input hash, result hash and next state hash. Signatures are normal Nostr event signatures whose content is the lowercase hex encoding of the canonical commitment.

The collector verifies event IDs/signatures, epoch membership, matching job fields and unique signers. A threshold of distinct responses that disagree is not agreement. A proof is accepted only for one matching group and no observed equivocation. Additional conflicting evidence is kept separately from immutable historical proofs; the API stops exporting a formerly accepted proof when its local evidence is disputed.

Jobs expire for new local signing. Signed historical proofs remain verifiable without requiring the verifier's present time to match the job time. Relay delay and signer clocks therefore affect liveness and expiry semantics; a malicious signer can choose a Nostr timestamp. Timestamps alone are not trusted evidence of timely independent computation.

## Nostr event model

Experimental regular event kinds, not replaceable events:

| Kind | Payload domain |
| --- | --- |
| 17900 | Genesis `federation-state`, or `certified-epoch` with proposal/approvals |
| 17901 | `federation-proposal` |
| 17902 | `federation-approval` |
| 17903 | `job` |
| 17904 | `job-attestation` |
| 17905 | `certified-result` with output and individual attestations |
| 17906 | Reserved `contract-descriptor`; transport ingestion is not enabled |

The signed `h` tag contains the federation ID; `protocol=chorus/1` identifies this protocol. Filters use `#h`. The engine verifies payload bindings independently of filtering. Duplicate event IDs are persisted and ignored. The SDK reconnects across multiple relays. The node performs bounded backfills and retries its outbox.

Private transport encrypts the complete inner member-signed event using NIP-17/NIP-59 and NIP-44 via the maintained Nostr implementation. Recipients follow the epoch's signed routing map; in new federations each recipient is the member's own npub. Private envelopes are never sent as bare public inner events. Content encryption does not hide all metadata or protect data from authorized members, the local OS or unencrypted SQLite backups.

Recipient kind-10050 inbox relay discovery is not yet implemented; private envelopes use configured node relays. This implements the NIP-17 encrypted format, not complete NIP-17 routing compliance.

Backfill is currently capped at 5,000 events per query and does not have a complete archival pagination cursor. Private envelopes randomize timestamps and are retried in bounded dependency passes. Contract upgrades require each member to import the exact approved bytes out of band. These are explicit deployment limits, not guarantees of unlimited resynchronization.

References: [Nostr NIPs](https://github.com/nostr-protocol/nips), [maintained Rust Nostr SDK](https://docs.rs/nostr-sdk/0.45.4/nostr_sdk/), [Borsh specification](https://borsh.io/).
