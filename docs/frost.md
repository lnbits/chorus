# FROST model

Chorus uses the maintained `frost-ed25519` 3.x implementation (FROST-Ed25519/SHA-512). This compact signature is distinct from Nostr's secp256k1 event signature. It is not claimed to be a Nostr Schnorr signature or a Bitcoin-compatible signature.

The library exposes the maintained implementation's DKG phases and validated key packages. Its signing wrapper accepts an opaque `Accepted` value, which can only be constructed after validating ordinary independent attestations. It binds the already accepted canonical commitment to a fresh session ID. Signing nonces live in a non-Clone, non-serializable wrapper and are consumed by round two even if signing fails. A coordinator cannot change the signed message after nonce generation. Aggregation verifies shares; public verification checks the group key and signature.

Integration tests perform a real three-party, two-of-three DKG, independently finalize a WASM job, create signing commitments, produce/aggregate shares, verify the group signature, and reject replay under a different session ID. This is library-level orchestration in a test, not a deployed DKG service.

Signed epoch metadata includes ciphersuite, group verifying key, member-to-participant mapping, setup ID and required count. Member/threshold changes require fresh setup metadata. Member counts above the library's u16 participant bound still work for ordinary attestations; they cannot enable this compact-signature profile without an explicit future protocol extension. A one-member threshold also uses ordinary attestations.

## Operational release gate

The node intentionally does not expose a FROST enable/sign button or fabricate a compact proof. A production distributed coordinator must still provide:

- Authenticated, session-bound DKG packages and encrypted recipient-specific round-two delivery.
- Explicit all-participant key-setup consent and transcript/key confirmation.
- Encrypted share storage, backup policy and key erasure/rekey ceremonies.
- Durable session IDs, expiration, abort/retry and coordinator/dropout handling.
- Crash-safe nonce burn before releasing commitments; no nonce recovery/reuse.
- Mapping and threshold checks against the exact accepted epoch on every signing round.
- Cross-node adversarial tests under reordered, replayed, duplicated and interrupted rounds.

These missing orchestration and storage boundaries prevent describing the requested full product as production-ready. Ordinary individually signed threshold attestations are the currently operational proof format.

References: [FROST library](https://docs.rs/frost-ed25519/3.0.0/frost_ed25519/), [FROST book](https://frost.zfnd.org/), [RFC 9591](https://www.rfc-editor.org/rfc/rfc9591.html).
