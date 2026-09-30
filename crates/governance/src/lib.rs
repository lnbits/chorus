use anyhow::{Result, ensure};
use chorus_federation::{APPROVAL, member, payload};
use chorus_protocol::*;
use nostr_sdk::prelude::Event;
use std::collections::BTreeSet;
pub fn validate_proposal(current: &FederationState, proposal: &FederationProposal) -> Result<()> {
    current.validate()?;
    proposal.next_state.validate()?;
    let next = &proposal.next_state;
    ensure!(
        proposal.protocol_version == VERSION
            && proposal.federation_id == current.federation_id
            && next.federation_id == current.federation_id,
        "wrong proposal federation"
    );
    ensure!(
        proposal.parent_state_hash == digest(current)?
            && next.previous_federation_state_hash.as_ref() == Some(&proposal.parent_state_hash),
        "wrong parent epoch"
    );
    ensure!(
        Some(next.epoch) == current.epoch.checked_add(1),
        "epoch must increment by one"
    );
    ensure!(
        next.initial_state == current.initial_state,
        "genesis execution state is immutable"
    );
    ensure!(
        next.created_at >= current.created_at,
        "epoch time moved backwards"
    );
    if next.members != current.members || next.threshold != current.threshold {
        ensure!(
            next.frost.is_none() || next.frost != current.frost,
            "membership or threshold changes require fresh FROST setup"
        );
    }
    Ok(())
}
pub fn approve(
    current: &FederationState,
    proposal: &FederationProposal,
    approvals: &[Event],
) -> Result<FederationState> {
    validate_proposal(current, proposal)?;
    let expected = FederationApproval {
        protocol_version: VERSION,
        federation_id: current.federation_id.clone(),
        epoch: current.epoch,
        parent_state_hash: digest(current)?,
        proposal_hash: digest(proposal)?,
    };
    let mut voters = BTreeSet::new();
    for event in approvals {
        let signer = member(event, current)?;
        let approval: FederationApproval = payload(event, APPROVAL, &current.federation_id)?;
        ensure!(approval == expected, "approval is for a different proposal");
        voters.insert(signer);
    }
    ensure!(
        voters.len() >= current.threshold.required(current.members.len())?,
        "current threshold has not approved this transition"
    );
    Ok(proposal.next_state.clone())
}
