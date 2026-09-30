use anyhow::{Context, Result, ensure};
use chorus_api::engine::{Invitation, ProofBundle};
use chorus_federation::{JOB, PROPOSAL, member, payload, validate_genesis};
use chorus_protocol::*;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
#[derive(Parser)]
#[command(
    name = "fedwasm",
    version,
    about = "Independently verify Chorus threshold attestations"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Verify {
        file: PathBuf,
        #[arg(long)]
        genesis_hash: String,
        #[arg(long)]
        rerun: bool,
    },
    Hash {
        file: PathBuf,
    },
}
fn verify_history(invite: &Invitation, pin: &str) -> Result<FederationState> {
    ensure!(!invite.epochs.is_empty(), "empty federation history");
    let first = &invite.epochs[0];
    let mut current = validate_genesis(
        first
            .genesis
            .as_ref()
            .context("missing genesis signature")?,
        pin,
    )?;
    ensure!(current == first.state, "genesis substitution");
    for e in invite.epochs.iter().skip(1) {
        let event = e.proposal.as_ref().context("missing governance proposal")?;
        member(event, &current)?;
        let p: FederationProposal = payload(event, PROPOSAL, &current.federation_id)?;
        current = chorus_governance::approve(&current, &p, &e.approvals)?;
        ensure!(current == e.state, "epoch substitution");
    }
    Ok(current)
}
fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Hash { file } => println!("{}", hash(&std::fs::read(file)?)),
        Command::Verify {
            file,
            genesis_hash,
            rerun,
        } => {
            ensure!(
                std::fs::metadata(&file)?.len() <= 32 * 1024 * 1024,
                "proof exceeds verifier file size limit"
            );
            let proof: ProofBundle = serde_json::from_slice(&std::fs::read(file)?)?;
            ensure!(proof.protocol == "chorus/1", "unsupported proof protocol");
            let state = verify_history(&proof.invitation, &genesis_hash)?;
            member(&proof.job, &state)?;
            let job: Job = payload(&proof.job, JOB, &state.federation_id)?;
            job.validate(&state)?;
            let accepted = chorus_consensus::verify(&state, &job, &proof.result)?;
            let wasm = hex::decode(&proof.invitation.wasm_hex)?;
            ensure!(hash(&wasm) == state.wasm_hash, "WASM hash mismatch");
            let output = hex::decode(&proof.output_hex)?;
            ensure!(
                JobAttestation::from_execution(&job, &output)? == proof.result.commitment,
                "output or state hash mismatch"
            );
            if let Some(frost) = &proof.result.frost {
                chorus_frost::verify(
                    &accepted,
                    state
                        .frost
                        .as_ref()
                        .context("FROST key missing from signed epoch")?,
                    frost,
                )?;
            }
            if rerun {
                let actual =
                    chorus_fedvm::FedVm::new()?.execute(&wasm, &state.wasm_hash, &job.input)?;
                ensure!(
                    actual == output,
                    "independent execution disagrees with accepted result"
                );
            }
            println!(
                "VERIFIED {} — {} of {} signatures{}",
                digest(&job)?,
                proof
                    .result
                    .attestations
                    .iter()
                    .map(|e| e.pubkey)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                state.members.len(),
                if rerun {
                    "; independent execution matches"
                } else {
                    "; execution not requested"
                }
            );
        }
    }
    Ok(())
}
