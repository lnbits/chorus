//! FROST is a compression of an already accepted proof, never a consensus mechanism.
use anyhow::{Result, ensure};
use chorus_consensus::{Accepted, FrostProof};
use chorus_protocol::{FrostMetadata, canonical, hash};
pub use frost::keys::{KeyPackage, PublicKeyPackage, dkg};
pub use frost::{Identifier, SigningPackage};
use frost_ed25519 as frost;
use std::collections::BTreeMap;
/// Nonces are intentionally neither Clone nor serializable. Consumed even when round 2 fails.
pub struct SigningRound {
    nonces: frost::round1::SigningNonces,
    message: Vec<u8>,
    pub commitment: frost::round1::SigningCommitments,
}
pub fn message(accepted: &Accepted, session_id: &str) -> Result<Vec<u8>> {
    ensure!(
        chorus_protocol::valid_hash(session_id),
        "invalid FROST session ID"
    );
    let mut msg = b"CHORUS-FROST-1\0".to_vec();
    msg.extend(session_id.as_bytes());
    msg.extend(canonical(&accepted.result().commitment)?);
    Ok(msg)
}
impl SigningRound {
    pub fn begin(key: &KeyPackage, accepted: &Accepted, session_id: &str) -> Result<Self> {
        let message = message(accepted, session_id)?;
        let (nonces, commitment) =
            frost::round1::commit(key.signing_share(), &mut rand::rngs::OsRng);
        Ok(Self {
            nonces,
            message,
            commitment,
        })
    }
    pub fn finish(
        self,
        key: &KeyPackage,
        package: &SigningPackage,
    ) -> Result<frost::round2::SignatureShare> {
        ensure!(
            package.message() == &self.message,
            "coordinator substituted commitment"
        );
        Ok(frost::round2::sign(package, &self.nonces, key)?)
    }
}
pub fn aggregate(
    package: &SigningPackage,
    shares: &BTreeMap<Identifier, frost::round2::SignatureShare>,
    public: &PublicKeyPackage,
) -> Result<Vec<u8>> {
    Ok(frost::aggregate(package, shares, public)?
        .serialize()?
        .to_vec())
}
pub fn verify(accepted: &Accepted, metadata: &FrostMetadata, proof: &FrostProof) -> Result<()> {
    ensure!(
        metadata.ciphersuite == "FROST-ED25519-SHA512-v1",
        "unsupported FROST suite"
    );
    let key = frost::VerifyingKey::deserialize(&metadata.verifying_key)?;
    let sig = frost::Signature::deserialize(&proof.signature)?;
    key.verify(&message(accepted, &proof.session_id)?, &sig)?;
    Ok(())
}
pub fn session_id(entropy: &[u8; 32], commitment: &[u8]) -> String {
    let mut b = b"CHORUS-FROST-SESSION-1".to_vec();
    b.extend(entropy);
    b.extend(commitment);
    hash(&b)
}
