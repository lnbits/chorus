//! Chorus v1 canonical wire model. Borsh bytes, never JSON text, are signed.
use anyhow::{Result, bail, ensure};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const VERSION: u16 = 1;
pub const VM_VERSION: &str = "fedvm/1-wasmtime48.0.3";
pub const MAX_MESSAGE: usize = 2 * 1024 * 1024;
pub const MAX_INPUT: usize = 256 * 1024;
pub const MAX_WASM: usize = 4 * 1024 * 1024;
pub type Hash = String;
pub type Member = String;

pub trait Canonical: BorshSerialize + BorshDeserialize {
    const DOMAIN: &'static str;
}
pub fn canonical<T: Canonical>(value: &T) -> Result<Vec<u8>> {
    let mut bytes = Vec::from(b"CHORUS\0".as_slice());
    bytes.extend(VERSION.to_le_bytes());
    bytes.extend((T::DOMAIN.len() as u16).to_le_bytes());
    bytes.extend(T::DOMAIN.as_bytes());
    value.serialize(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_MESSAGE,
        "canonical message exceeds limit"
    );
    Ok(bytes)
}
pub fn decode<T: Canonical>(bytes: &[u8]) -> Result<T> {
    ensure!(bytes.len() <= MAX_MESSAGE, "message exceeds limit");
    let mut prefix = Vec::from(b"CHORUS\0".as_slice());
    prefix.extend(VERSION.to_le_bytes());
    prefix.extend((T::DOMAIN.len() as u16).to_le_bytes());
    prefix.extend(T::DOMAIN.as_bytes());
    ensure!(bytes.starts_with(&prefix), "incorrect domain or version");
    let value = T::try_from_slice(&bytes[prefix.len()..])?;
    ensure!(canonical(&value)? == bytes, "noncanonical encoding");
    Ok(value)
}
pub fn hash(bytes: &[u8]) -> Hash {
    hex::encode(Sha256::digest(bytes))
}
pub fn digest<T: Canonical>(value: &T) -> Result<Hash> {
    Ok(hash(&canonical(value)?))
}
pub fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ThresholdRule {
    Absolute(u32),
}
impl ThresholdRule {
    /// An explicit quorum, strictly larger than half the current member set.
    pub fn required(&self, members: usize) -> Result<usize> {
        ensure!(
            members > 0 && members <= u32::MAX as usize,
            "invalid member count"
        );
        let Self::Absolute(required) = *self;
        let required = required as usize;
        ensure!(
            required > members / 2 && required <= members,
            "quorum must be greater than half the members and no greater than the member count"
        );
        Ok(required)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct FrostMetadata {
    pub ciphersuite: String,
    pub verifying_key: Vec<u8>,
    pub participants: Vec<(Member, u16)>,
    pub required: u16,
    pub setup_id: Hash,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct FederationState {
    pub protocol_version: u16,
    pub federation_id: Hash,
    pub epoch: u64,
    pub name: String,
    pub description: String,
    pub public: bool,
    pub members: Vec<Member>,
    pub communication_keys: Vec<(Member, String)>,
    pub threshold: ThresholdRule,
    pub wasm_hash: Hash,
    pub vm_version: String,
    pub initial_state: Option<Vec<u8>>,
    pub previous_federation_state_hash: Option<Hash>,
    pub created_at: u64,
    pub relays: Vec<String>,
    pub frost: Option<FrostMetadata>,
}
impl Canonical for FederationState {
    const DOMAIN: &'static str = "federation-state";
}
impl FederationState {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.protocol_version == VERSION,
            "unsupported protocol version"
        );
        ensure!(
            valid_hash(&self.federation_id) && valid_hash(&self.wasm_hash),
            "invalid hash"
        );
        ensure!(
            !self.name.trim().is_empty()
                && self.name.len() <= 128
                && self.description.len() <= 4096,
            "invalid federation name or description"
        );
        ensure!(
            self.members.windows(2).all(|w| w[0] < w[1])
                && self.members.iter().all(|m| valid_hash(m)),
            "members must be unique sorted hex keys"
        );
        let required = self.threshold.required(self.members.len())?;
        ensure!(
            self.communication_keys.windows(2).all(|w| w[0].0 < w[1].0)
                && self
                    .communication_keys
                    .iter()
                    .all(|(m, k)| self.members.contains(m) && valid_hash(k)),
            "invalid communication identity mapping"
        );
        ensure!(
            self.public || self.communication_keys.len() == self.members.len(),
            "private federations require every communication key"
        );
        ensure!(self.vm_version == VM_VERSION, "unsupported VM profile");
        ensure!(
            self.relays.len() <= 32 && self.relays.windows(2).all(|w| w[0] < w[1]),
            "relay list must be unique and sorted"
        );
        ensure!(
            self.relays
                .iter()
                .all(|r| r.len() <= 512 && r.starts_with("wss://")),
            "federation relays require wss"
        );
        ensure!(
            self.initial_state
                .as_ref()
                .is_none_or(|s| s.len() <= MAX_INPUT),
            "initial state exceeds limit"
        );
        if self.epoch == 0 {
            ensure!(
                self.previous_federation_state_hash.is_none(),
                "genesis has parent"
            );
        } else {
            ensure!(
                self.previous_federation_state_hash
                    .as_deref()
                    .is_some_and(valid_hash),
                "missing parent state hash"
            );
        }
        if let Some(frost) = &self.frost {
            ensure!(
                frost.ciphersuite == "FROST-ED25519-SHA512-v1" && frost.verifying_key.len() == 32,
                "invalid FROST suite or key"
            );
            ensure!(
                frost.required as usize == required
                    && required >= 2
                    && self.members.len() <= u16::MAX as usize,
                "FROST threshold mismatch"
            );
            ensure!(valid_hash(&frost.setup_id), "invalid FROST setup ID");
            ensure!(
                frost
                    .participants
                    .iter()
                    .map(|(m, _)| m)
                    .eq(self.members.iter()),
                "FROST member mapping mismatch"
            );
            let ids: std::collections::BTreeSet<_> =
                frost.participants.iter().map(|(_, id)| *id).collect();
            ensure!(
                !ids.contains(&0) && ids.len() == self.members.len(),
                "invalid participant IDs"
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct FederationProposal {
    pub protocol_version: u16,
    pub federation_id: Hash,
    pub parent_state_hash: Hash,
    pub next_state: FederationState,
}
impl Canonical for FederationProposal {
    const DOMAIN: &'static str = "federation-proposal";
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct FederationApproval {
    pub protocol_version: u16,
    pub federation_id: Hash,
    pub epoch: u64,
    pub parent_state_hash: Hash,
    pub proposal_hash: Hash,
}
impl Canonical for FederationApproval {
    const DOMAIN: &'static str = "federation-approval";
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct ContractDescriptor {
    pub wasm_hash: Hash,
    pub name: String,
    pub size: u64,
    pub vm_version: String,
}
impl Canonical for ContractDescriptor {
    const DOMAIN: &'static str = "contract-descriptor";
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct InputEnvelope {
    pub previous_state: Option<Vec<u8>>,
    pub command: Vec<u8>,
    pub attestations: Vec<Vec<u8>>,
}
impl Canonical for InputEnvelope {
    const DOMAIN: &'static str = "execution-input";
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct OutputEnvelope {
    pub result: Vec<u8>,
    pub next_state: Option<Vec<u8>>,
    /// Opaque deterministic declarations. The node never executes them.
    pub declared_actions: Vec<Vec<u8>>,
}
impl Canonical for OutputEnvelope {
    const DOMAIN: &'static str = "execution-output";
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Job {
    pub protocol_version: u16,
    pub federation_id: Hash,
    pub epoch: u64,
    pub federation_state_hash: Hash,
    pub nonce: Hash,
    pub sequence: Option<u64>,
    pub vm_version: String,
    pub wasm_hash: Hash,
    pub previous_state_hash: Option<Hash>,
    pub input: Vec<u8>,
    pub input_hash: Hash,
    pub created_at: u64,
    pub expires_at: u64,
}
impl Canonical for Job {
    const DOMAIN: &'static str = "job";
}
impl Job {
    pub fn validate(&self, state: &FederationState) -> Result<InputEnvelope> {
        state.validate()?;
        ensure!(
            self.protocol_version == VERSION
                && self.federation_id == state.federation_id
                && self.epoch == state.epoch
                && self.federation_state_hash == digest(state)?,
            "stale or wrong federation epoch"
        );
        ensure!(
            self.vm_version == state.vm_version && self.wasm_hash == state.wasm_hash,
            "wrong contract or VM"
        );
        ensure!(
            valid_hash(&self.nonce)
                && self.input.len() <= MAX_INPUT
                && self.input_hash == hash(&self.input),
            "invalid job input"
        );
        ensure!(
            self.expires_at > self.created_at && self.expires_at - self.created_at <= 86400,
            "invalid job lifetime"
        );
        let input: InputEnvelope = decode(&self.input)?;
        ensure!(
            self.previous_state_hash == input.previous_state.as_ref().map(|s| hash(s)),
            "previous state hash mismatch"
        );
        ensure!(
            self.sequence.is_some() == state.initial_state.is_some()
                && self.sequence.is_some() == input.previous_state.is_some(),
            "statefulness mismatch"
        );
        Ok(input)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct JobAttestation {
    pub protocol_version: u16,
    pub federation_id: Hash,
    pub epoch: u64,
    pub federation_state_hash: Hash,
    pub job_id: Hash,
    pub sequence: Option<u64>,
    pub vm_version: String,
    pub wasm_hash: Hash,
    pub previous_state_hash: Option<Hash>,
    pub input_hash: Hash,
    pub result_hash: Hash,
    pub next_state_hash: Option<Hash>,
}
impl Canonical for JobAttestation {
    const DOMAIN: &'static str = "job-attestation";
}
impl JobAttestation {
    pub fn from_execution(job: &Job, output: &[u8]) -> Result<Self> {
        ensure!(output.len() <= MAX_INPUT, "output exceeds VM profile limit");
        let envelope: OutputEnvelope = decode(output)?;
        if job.sequence.is_some() != envelope.next_state.is_some() {
            bail!("output statefulness mismatch");
        }
        Ok(Self {
            protocol_version: VERSION,
            federation_id: job.federation_id.clone(),
            epoch: job.epoch,
            federation_state_hash: job.federation_state_hash.clone(),
            job_id: digest(job)?,
            sequence: job.sequence,
            vm_version: job.vm_version.clone(),
            wasm_hash: job.wasm_hash.clone(),
            previous_state_hash: job.previous_state_hash.clone(),
            input_hash: job.input_hash.clone(),
            result_hash: hash(output),
            next_state_hash: envelope.next_state.as_ref().map(|s| hash(s)),
        })
    }
    pub fn validate_job(&self, job: &Job) -> Result<()> {
        ensure!(
            self.protocol_version == VERSION
                && self.federation_id == job.federation_id
                && self.epoch == job.epoch
                && self.federation_state_hash == job.federation_state_hash
                && self.job_id == digest(job)?
                && self.sequence == job.sequence
                && self.vm_version == job.vm_version
                && self.wasm_hash == job.wasm_hash
                && self.previous_state_hash == job.previous_state_hash
                && self.input_hash == job.input_hash,
            "commitment does not match job"
        );
        ensure!(
            valid_hash(&self.result_hash)
                && self.next_state_hash.as_deref().is_none_or(valid_hash)
                && self.next_state_hash.is_some() == job.sequence.is_some(),
            "invalid result hashes"
        );
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_majority_boundaries() {
        for n in [1, 2, 3, 7, 10, 100, 10001] {
            assert!(ThresholdRule::Absolute((n / 2) as u32).required(n).is_err());
            assert_eq!(
                ThresholdRule::Absolute((n / 2 + 1) as u32)
                    .required(n)
                    .unwrap(),
                n / 2 + 1
            );
            assert_eq!(ThresholdRule::Absolute(n as u32).required(n).unwrap(), n);
            assert!(ThresholdRule::Absolute((n + 1) as u32).required(n).is_err());
        }
        assert_eq!(ThresholdRule::Absolute(6).required(10).unwrap(), 6);
        assert!(ThresholdRule::Absolute(5).required(10).is_err());
        assert!(ThresholdRule::Absolute(1).required(0).is_err());
    }
    #[test]
    fn canonical_roundtrip_rejects_trailing_and_wrong_domain() {
        let input = InputEnvelope {
            previous_state: None,
            command: vec![1, 2],
            attestations: vec![],
        };
        let mut b = canonical(&input).unwrap();
        assert_eq!(decode::<InputEnvelope>(&b).unwrap(), input);
        assert!(decode::<OutputEnvelope>(&b).is_err());
        b.push(0);
        assert!(decode::<InputEnvelope>(&b).is_err());
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct CertifiedResult {
    pub job_id: Hash,
    pub output: Vec<u8>,
    pub attestations: Vec<String>,
}
impl Canonical for CertifiedResult {
    const DOMAIN: &'static str = "certified-result";
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct CertifiedEpoch {
    pub proposal: String,
    pub approvals: Vec<String>,
}
impl Canonical for CertifiedEpoch {
    const DOMAIN: &'static str = "certified-epoch";
}
