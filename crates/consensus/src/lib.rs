use anyhow::{Result, ensure};
use chorus_federation::{ATTESTATION, member, payload};
use chorus_protocol::*;
use nostr_sdk::prelude::Event;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResultGroup {
    pub commitment: JobAttestation,
    pub signers: Vec<String>,
    pub attestations: Vec<Event>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Consensus {
    pub required: usize,
    pub groups: Vec<ResultGroup>,
    pub equivocators: Vec<String>,
    pub accepted: Option<Hash>,
    pub conflicting_certificates: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrostProof {
    pub session_id: Hash,
    pub signature: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FinalizedResult {
    pub commitment: JobAttestation,
    pub attestations: Vec<Event>,
    pub frost: Option<FrostProof>,
}
/// The only constructor validates every signature, membership and the complete commitment.
pub struct Accepted {
    result: FinalizedResult,
}
impl Accepted {
    pub fn result(&self) -> &FinalizedResult {
        &self.result
    }
}
pub fn collect(state: &FederationState, job: &Job, events: &[Event]) -> Result<Consensus> {
    job.validate(state)?;
    let mut groups: BTreeMap<Hash, ResultGroup> = BTreeMap::new();
    let mut votes: BTreeMap<String, BTreeSet<Hash>> = BTreeMap::new();
    for event in events {
        let signer = member(event, state)?;
        let commitment: JobAttestation = payload(event, ATTESTATION, &state.federation_id)?;
        commitment.validate_job(job)?;
        ensure!(
            event.created_at.as_secs() <= job.expires_at,
            "attestation after job expiry"
        );
        let id = digest(&commitment)?;
        votes.entry(signer.clone()).or_default().insert(id.clone());
        let group = groups.entry(id).or_insert(ResultGroup {
            commitment,
            signers: vec![],
            attestations: vec![],
        });
        if !group.signers.contains(&signer) {
            group.signers.push(signer);
            group.attestations.push(event.clone());
        }
    }
    let equivocators: Vec<_> = votes
        .into_iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(k, _)| k)
        .collect();
    // Count certified groups before exclusion too: conflicting certificates must stay visible.
    let required = state.threshold.required(state.members.len())?;
    let certified: Vec<_> = groups
        .iter()
        .filter(|(_, g)| g.signers.len() >= required)
        .map(|(id, _)| id.clone())
        .collect();
    let accepted = if certified.len() == 1 && equivocators.is_empty() {
        certified.first().cloned()
    } else {
        None
    };
    Ok(Consensus {
        required,
        groups: groups.into_values().collect(),
        equivocators,
        accepted,
        conflicting_certificates: certified.len() > 1,
    })
}
pub fn verify(state: &FederationState, job: &Job, proof: &FinalizedResult) -> Result<Accepted> {
    let c = collect(state, job, &proof.attestations)?;
    ensure!(
        c.accepted.as_ref() == Some(&digest(&proof.commitment)?),
        "no unique threshold agreement"
    );
    Ok(Accepted {
        result: proof.clone(),
    })
}
