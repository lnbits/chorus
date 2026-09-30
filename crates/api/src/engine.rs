use anyhow::{Context, Result, ensure};
use chorus_consensus::{self as consensus, FinalizedResult};
use chorus_federation::{self as federation, member, payload, signed};
use chorus_fedvm::FedVm;
use chorus_nostr::Transport;
use chorus_protocol::*;
use chorus_storage::{Storage, now, token};
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EpochRecord {
    pub state: FederationState,
    pub genesis: Option<Event>,
    pub proposal: Option<Event>,
    pub approvals: Vec<Event>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Invitation {
    pub genesis_hash: String,
    pub epochs: Vec<EpochRecord>,
    pub wasm_hex: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProofBundle {
    pub protocol: String,
    pub invitation: Invitation,
    pub job: Event,
    pub output_hex: String,
    pub result: FinalizedResult,
}
#[derive(Clone)]
pub struct Engine {
    pub db: Storage,
    pub vm: FedVm,
    signing: Arc<Keys>,
    pub transport: Transport,
    pub mutation: Arc<Mutex<()>>,
    execution: Arc<Semaphore>,
}
impl Engine {
    pub fn new(db: Storage, vm: FedVm, signing: Keys, transport: Transport) -> Result<Self> {
        ensure!(
            signing.public_key() == transport.public_key(),
            "signing and messaging must use the same Nostr identity"
        );
        Ok(Self {
            db,
            vm,
            signing: Arc::new(signing),
            transport,
            mutation: Arc::new(Mutex::new(())),
            execution: Arc::new(Semaphore::new(2)),
        })
    }
    pub fn member_key(&self) -> PublicKey {
        self.signing.public_key()
    }
    pub fn epochs(&self, id: &str) -> Result<Vec<EpochRecord>> {
        let mut v: Vec<EpochRecord> = self.db.list("epoch", Some(id))?;
        v.sort_by_key(|e| e.state.epoch);
        Ok(v)
    }
    pub fn active(&self, id: &str) -> Result<FederationState> {
        ensure!(
            self.db.get::<bool>("quarantine", id)?.is_none(),
            "federation quarantined: conflicting certificates require manual recovery"
        );
        self.epochs(id)?
            .last()
            .map(|e| e.state.clone())
            .context("federation not found")
    }
    pub fn states(&self) -> Result<Vec<FederationState>> {
        let epochs: Vec<EpochRecord> = self.db.list("epoch", None)?;
        let mut map = std::collections::BTreeMap::new();
        for e in epochs {
            let entry = map
                .entry(e.state.federation_id.clone())
                .or_insert(e.state.clone());
            if entry.epoch < e.state.epoch {
                *entry = e.state;
            }
        }
        Ok(map.into_values().collect())
    }
    pub fn at_epoch(&self, id: &str, epoch: u64) -> Result<FederationState> {
        self.epochs(id)?
            .into_iter()
            .find(|e| e.state.epoch == epoch)
            .map(|e| e.state)
            .context("epoch not found")
    }
    pub fn recipients(&self, state: &FederationState) -> Vec<String> {
        if state.public {
            vec![]
        } else {
            state
                .communication_keys
                .iter()
                .map(|(_, k)| k.clone())
                .collect()
        }
    }
    pub fn queue(&self, event: &Event, state: &FederationState) -> Result<()> {
        self.db.queue_event(event, &self.recipients(state))
    }
    pub async fn refresh_subscriptions(&self) -> Result<()> {
        let ids = self
            .states()?
            .into_iter()
            .map(|s| s.federation_id)
            .collect::<Vec<_>>();
        self.transport.subscribe(&ids).await
    }
    pub fn invitation(&self, id: &str) -> Result<Invitation> {
        let epochs = self.epochs(id)?;
        let last = epochs.last().context("federation not found")?;
        Ok(Invitation {
            genesis_hash: digest(&epochs[0].state)?,
            wasm_hex: hex::encode(self.db.blob(&last.state.wasm_hash)?),
            epochs,
        })
    }
    pub fn join(&self, invite: &Invitation, pin: &str) -> Result<FederationState> {
        ensure!(
            !invite.epochs.is_empty() && invite.epochs.len() <= 10000,
            "invalid epoch history"
        );
        ensure!(invite.genesis_hash == pin, "trust anchor mismatch");
        let first = &invite.epochs[0];
        let mut current = federation::validate_genesis(
            first.genesis.as_ref().context("missing genesis event")?,
            pin,
        )?;
        ensure!(current == first.state, "genesis state mismatch");
        for epoch in invite.epochs.iter().skip(1) {
            let event = epoch.proposal.as_ref().context("missing epoch proposal")?;
            member(event, &current)?;
            let proposal: FederationProposal =
                payload(event, federation::PROPOSAL, &current.federation_id)?;
            let next = chorus_governance::approve(&current, &proposal, &epoch.approvals)?;
            ensure!(
                next == epoch.state,
                "epoch state does not match certificate"
            );
            current = next;
        }
        let wasm = hex::decode(&invite.wasm_hex)?;
        self.vm.validate(&wasm, &current.wasm_hash)?;
        // Reject collisions, even if a different genesis reuses a federation ID.
        if let Some(old) = self.epochs(&current.federation_id)?.first() {
            ensure!(
                digest(&old.state)? == pin,
                "federation ID already has another trust anchor"
            );
        }
        self.db.store_blob(&wasm)?;
        for epoch in &invite.epochs {
            self.store_epoch(epoch)?;
        }
        self.db.activity(
            "federation_joined",
            Some(&current.federation_id),
            &current.federation_id,
            "Federation history verified and imported",
        )?;
        Ok(current)
    }
    fn store_epoch(&self, record: &EpochRecord) -> Result<()> {
        let s = &record.state;
        let id = digest(s)?;
        for existing in self.epochs(&s.federation_id)? {
            if existing.state.epoch == s.epoch && digest(&existing.state)? != id {
                self.db
                    .put("epoch_conflict", &id, Some(&s.federation_id), record)?;
                self.db.put(
                    "quarantine",
                    &s.federation_id,
                    Some(&s.federation_id),
                    &true,
                )?;
                self.db.activity(
                    "epoch_conflict",
                    Some(&s.federation_id),
                    &id,
                    "Conflicting epoch certificates; federation quarantined",
                )?;
                anyhow::bail!("conflicting epoch certificate");
            }
        }
        if self.db.get::<EpochRecord>("epoch", &id)?.is_none() {
            self.db.put("epoch", &id, Some(&s.federation_id), record)?;
        }
        Ok(())
    }
    pub fn create(&self, mut state: FederationState, wasm: &[u8]) -> Result<FederationState> {
        state.members.sort();
        ensure!(
            state
                .communication_keys
                .iter()
                .all(|(m, k)| m == k && state.members.contains(m)),
            "new federations use member identities for private messaging"
        );
        state.communication_keys = state
            .members
            .iter()
            .map(|m| (m.clone(), m.clone()))
            .collect();
        state.relays.sort();
        state.relays.dedup();
        state.validate()?;
        ensure!(
            state.epoch == 0 && state.members.contains(&self.member_key().to_hex()),
            "creator must sign as an initial member"
        );
        self.vm.validate(wasm, &state.wasm_hash)?;
        self.db.store_blob(wasm)?;
        let event = signed(
            &self.signing,
            federation::STATE,
            &state.federation_id,
            &state,
        )?;
        self.store_epoch(&EpochRecord {
            state: state.clone(),
            genesis: Some(event.clone()),
            proposal: None,
            approvals: vec![],
        })?;
        self.queue(&event, &state)?;
        self.db.activity(
            "federation_created",
            Some(&state.federation_id),
            &state.federation_id,
            "Genesis created; subsequent changes require threshold approval",
        )?;
        Ok(state)
    }
    pub fn submit_job(&self, state: &FederationState, command: Vec<u8>) -> Result<(String, Job)> {
        ensure!(
            state.members.contains(&self.member_key().to_hex()),
            "this node is not a member"
        );
        let (sequence, previous) = self.execution_head(state)?;
        let input = canonical(&InputEnvelope {
            previous_state: previous.clone(),
            command,
            attestations: vec![],
        })?;
        let job = Job {
            protocol_version: VERSION,
            federation_id: state.federation_id.clone(),
            epoch: state.epoch,
            federation_state_hash: digest(state)?,
            nonce: token(),
            sequence,
            vm_version: state.vm_version.clone(),
            wasm_hash: state.wasm_hash.clone(),
            previous_state_hash: previous.as_ref().map(|v| hash(v)),
            input_hash: hash(&input),
            input,
            created_at: now(),
            expires_at: now() + 3600,
        };
        job.validate(state)?;
        let id = digest(&job)?;
        let event = signed(&self.signing, federation::JOB, &state.federation_id, &job)?;
        self.db
            .put("job", &id, Some(&state.federation_id), &event)?;
        self.queue(&event, state)?;
        Ok((id, job))
    }
    pub fn execution_head(
        &self,
        state: &FederationState,
    ) -> Result<(Option<u64>, Option<Vec<u8>>)> {
        let Some(initial) = &state.initial_state else {
            return Ok((None, None));
        };
        let results: Vec<FinalizedResult> = self.db.list("final", Some(&state.federation_id))?;
        let mut ordered = results
            .iter()
            .filter_map(|r| r.commitment.sequence.map(|s| (s, r)))
            .collect::<Vec<_>>();
        ordered.sort_by_key(|(s, _)| *s);
        let mut next = 0;
        let mut previous = initial.clone();
        for (sequence, result) in ordered {
            ensure!(sequence == next, "state sequence gap or conflicting result");
            let bytes = self.db.blob(&result.commitment.result_hash)?;
            let out: OutputEnvelope = decode(&bytes)?;
            previous = out.next_state.context("missing next state")?;
            next += 1;
        }
        Ok((Some(next), Some(previous)))
    }
    pub fn job(&self, id: &str) -> Result<(Event, Job, FederationState)> {
        let event: Event = self.db.get("job", id)?.context("job not found")?;
        let fid = event_federation(&event)?;
        let job: Job = payload(&event, federation::JOB, &fid)?;
        let state = self.at_epoch(&fid, job.epoch)?;
        Ok((event, job, state))
    }
    pub fn attestations(&self, id: &str, fid: &str) -> Result<Vec<Event>> {
        let all: Vec<Event> = self.db.list("attestation", Some(fid))?;
        Ok(all
            .into_iter()
            .filter(|e| {
                payload::<JobAttestation>(e, federation::ATTESTATION, fid)
                    .is_ok_and(|a| a.job_id == id)
            })
            .collect())
    }
    pub fn consensus(&self, id: &str) -> Result<consensus::Consensus> {
        let (_, job, state) = self.job(id)?;
        let events = self.attestations(id, &state.federation_id)?;
        let mut result = consensus::collect(&state, &job, &events)?;
        if let Some(sequence) = job.sequence {
            let signers = events
                .iter()
                .map(|e| e.pubkey)
                .collect::<std::collections::BTreeSet<_>>();
            for other in self
                .db
                .list::<Event>("attestation", Some(&state.federation_id))?
            {
                if signers.contains(&other.pubkey) {
                    let a: JobAttestation =
                        payload(&other, federation::ATTESTATION, &state.federation_id)?;
                    if a.sequence == Some(sequence) && a.job_id != id {
                        result.equivocators.push(other.pubkey.to_hex());
                    }
                }
            }
            result.equivocators.sort();
            result.equivocators.dedup();
            if !result.equivocators.is_empty() {
                result.accepted = None;
            }
        }
        Ok(result)
    }
    pub fn evaluate(&self, id: &str) -> Result<consensus::Consensus> {
        let (_, job, state) = self.job(id)?;
        let c = self.consensus(id)?;
        if c.conflicting_certificates || !c.equivocators.is_empty() {
            self.db.activity(
                "job_disputed",
                Some(&state.federation_id),
                id,
                "Incompatible signed commitments detected",
            )?;
            if job.sequence.is_some() {
                self.db.put(
                    "quarantine",
                    &state.federation_id,
                    Some(&state.federation_id),
                    &true,
                )?;
            }
        }
        if let Some(accepted) = &c.accepted {
            let group = c
                .groups
                .iter()
                .find(|g| digest(&g.commitment).as_ref().ok() == Some(accepted))
                .context("accepted group missing")?;
            if self.db.get::<FinalizedResult>("final", id)?.is_none() {
                // A local independent execution supplies exact output bytes; peers verify by re-execution.
                if self.db.blob(&group.commitment.result_hash).is_ok() {
                    let proof = FinalizedResult {
                        commitment: group.commitment.clone(),
                        attestations: group.attestations.clone(),
                        frost: None,
                    };
                    consensus::verify(&state, &job, &proof)?;
                    if let Some(sequence) = job.sequence {
                        for old in self
                            .db
                            .list::<FinalizedResult>("final", Some(&state.federation_id))?
                        {
                            if old.commitment.sequence == Some(sequence)
                                && old.commitment.job_id != id
                            {
                                self.db.put(
                                    "conflicting_final",
                                    id,
                                    Some(&state.federation_id),
                                    &proof,
                                )?;
                                self.db.put(
                                    "quarantine",
                                    &state.federation_id,
                                    Some(&state.federation_id),
                                    &true,
                                )?;
                                anyhow::bail!("conflicting stateful sequence certificates");
                            }
                        }
                        let (expected, previous) = self.execution_head(&state)?;
                        ensure!(
                            expected == Some(sequence)
                                && job.previous_state_hash == previous.as_ref().map(|p| hash(p)),
                            "final result is not at stateful head"
                        );
                    }
                    self.db
                        .put("final", id, Some(&state.federation_id), &proof)?;
                    let result = CertifiedResult {
                        job_id: id.to_owned(),
                        output: self.db.blob(&proof.commitment.result_hash)?,
                        attestations: proof.attestations.iter().map(|e| e.as_json()).collect(),
                    };
                    if state.members.contains(&self.member_key().to_hex()) {
                        self.queue(
                            &signed(
                                &self.signing,
                                federation::FINALIZED,
                                &state.federation_id,
                                &result,
                            )?,
                            &state,
                        )?;
                    }
                    self.db.activity(
                        "job_finalized",
                        Some(&state.federation_id),
                        id,
                        "Matching independent attestations reached the configured threshold",
                    )?;
                }
            }
        }
        Ok(c)
    }
    /// There is deliberately no sign-result-hash endpoint. Execution is the sole caller of result signing.
    pub async fn execute(&self, id: &str) -> Result<()> {
        let result = self.execute_verified(id).await;
        if let Ok((_, job, _)) = self.job(id) {
            self.db
                .set_setting(&format!("execution_failed:{id}"), &result.is_err())?;
            if result.is_err() {
                self.db.activity("execution_failed", Some(&job.federation_id), id,
                    "Execution could not complete; inspect contract, epoch, expiry and resource limits")?;
            }
        }
        result
    }
    async fn execute_verified(&self, id: &str) -> Result<()> {
        let _permit = self.execution.acquire().await?;
        let (_, job, state) = self.job(id)?;
        job.validate(&state)?;
        let active = self.active(&state.federation_id)?;
        ensure!(active.epoch == job.epoch, "cannot execute a stale epoch");
        ensure!(
            now() <= job.expires_at && job.created_at <= now().saturating_add(300),
            "job expired or created too far in the future"
        );
        let (sequence, previous) = self.execution_head(&state)?;
        ensure!(
            job.sequence == sequence
                && job.previous_state_hash == previous.as_ref().map(|v| hash(v)),
            "stateful job is not at current head"
        );
        let wasm = self.db.blob(&state.wasm_hash)?;
        let vm = self.vm.clone();
        let input = job.input.clone();
        let wasm_hash = state.wasm_hash.clone();
        let output =
            tokio::task::spawn_blocking(move || vm.execute(&wasm, &wasm_hash, &input)).await??;
        let commitment = JobAttestation::from_execution(&job, &output)?;
        let _guard = self.mutation.lock().await;
        ensure!(
            self.active(&state.federation_id)?.epoch == job.epoch && now() <= job.expires_at,
            "epoch changed or job expired during execution"
        );
        self.db.store_blob(&output)?;
        if state.members.contains(&self.member_key().to_hex()) {
            let scope = if let Some(sequence) = job.sequence {
                format!("execution:{}:sequence:{sequence}", state.federation_id)
            } else {
                format!("execution:{}:{id}", state.federation_id)
            };
            self.db.reserve_signature(
                &self.member_key().to_hex(),
                &scope,
                &digest(&commitment)?,
            )?;
            let event = signed(
                &self.signing,
                federation::ATTESTATION,
                &state.federation_id,
                &commitment,
            )?;
            self.db.put(
                "attestation",
                &event.id.to_hex(),
                Some(&state.federation_id),
                &event,
            )?;
            self.queue(&event, &state)?;
        }
        self.evaluate(id)?;
        Ok(())
    }
    pub fn propose(&self, id: &str, mut next: FederationState) -> Result<String> {
        let current = self.active(id)?;
        ensure!(
            current.members.contains(&self.member_key().to_hex()),
            "not a current member"
        );
        next.epoch = current.epoch.checked_add(1).context("epoch overflow")?;
        next.previous_federation_state_hash = Some(digest(&current)?);
        next.created_at = now();
        next.members.sort();
        next.communication_keys.sort();
        next.relays.sort();
        let p = FederationProposal {
            protocol_version: VERSION,
            federation_id: id.to_string(),
            parent_state_hash: digest(&current)?,
            next_state: next,
        };
        chorus_governance::validate_proposal(&current, &p)?;
        // Validate replacement bytes before locally proposing their hash.
        self.vm.validate(
            &self.db.blob(&p.next_state.wasm_hash)?,
            &p.next_state.wasm_hash,
        )?;
        let pid = digest(&p)?;
        let event = signed(&self.signing, federation::PROPOSAL, id, &p)?;
        self.db.put("proposal", &pid, Some(id), &event)?;
        self.queue(&event, &current)?;
        self.db.activity(
            "proposal_created",
            Some(id),
            &pid,
            "Federation change proposed",
        )?;
        Ok(pid)
    }
    pub fn approve(&self, id: &str, pid: &str) -> Result<()> {
        let current = self.active(id)?;
        ensure!(
            current.members.contains(&self.member_key().to_hex()),
            "not a current member"
        );
        let event: Event = self
            .db
            .get("proposal", pid)?
            .context("proposal not found")?;
        let p: FederationProposal = payload(&event, federation::PROPOSAL, id)?;
        chorus_governance::validate_proposal(&current, &p)?;
        self.vm.validate(
            &self.db.blob(&p.next_state.wasm_hash)?,
            &p.next_state.wasm_hash,
        )?;
        let approval = FederationApproval {
            protocol_version: VERSION,
            federation_id: id.to_string(),
            epoch: current.epoch,
            parent_state_hash: digest(&current)?,
            proposal_hash: pid.to_string(),
        };
        self.db.reserve_signature(
            &self.member_key().to_hex(),
            &format!("governance:{id}:{}", current.epoch),
            pid,
        )?;
        let event = signed(&self.signing, federation::APPROVAL, id, &approval)?;
        self.db
            .put("approval", &event.id.to_hex(), Some(id), &event)?;
        self.queue(&event, &current)?;
        self.try_transition(id, pid)?;
        Ok(())
    }
    pub fn try_transition(&self, id: &str, pid: &str) -> Result<()> {
        let event: Event = self
            .db
            .get("proposal", pid)?
            .context("proposal not found")?;
        let p: FederationProposal = payload(&event, federation::PROPOSAL, id)?;
        let current = self.at_epoch(id, p.next_state.epoch - 1)?;
        let approvals: Vec<Event> = self
            .db
            .list::<Event>("approval", Some(id))?
            .into_iter()
            .filter(|e| {
                payload::<FederationApproval>(e, federation::APPROVAL, id)
                    .is_ok_and(|a| a.proposal_hash == pid)
            })
            .collect();
        let unique: std::collections::BTreeSet<_> = approvals.iter().map(|e| e.pubkey).collect();
        if unique.len() < current.threshold.required(current.members.len())? {
            return Ok(());
        }
        let next = chorus_governance::approve(&current, &p, &approvals)?;
        let certificate = CertifiedEpoch {
            proposal: event.as_json(),
            approvals: approvals.iter().map(|e| e.as_json()).collect(),
        };
        self.store_epoch(&EpochRecord {
            state: next,
            genesis: None,
            proposal: Some(event),
            approvals,
        })?;
        if current.members.contains(&self.member_key().to_hex()) {
            self.queue(
                &signed(&self.signing, federation::STATE, id, &certificate)?,
                &current,
            )?;
        }
        self.db.activity(
            "epoch_changed",
            Some(id),
            pid,
            "Current threshold approved a new federation epoch",
        )?;
        Ok(())
    }
    pub async fn ingest(&self, event: Event) -> Result<()> {
        let event = self.transport.unwrap(event).await?;
        let fid = event_federation(&event)?;
        if self
            .db
            .get::<Event>("received", &event.id.to_hex())?
            .is_some()
        {
            return Ok(());
        }
        let _guard = self.mutation.lock().await;
        let mut execute = None;
        match event.kind.as_u16() {
            federation::JOB => {
                let job: Job = payload(&event, federation::JOB, &fid)?;
                let state = self.at_epoch(&fid, job.epoch)?;
                member(&event, &state)?;
                job.validate(&state)?;
                let id = digest(&job)?;
                self.db.put("job", &id, Some(&fid), &event)?;
                execute = Some(id);
            }
            federation::ATTESTATION => {
                let a: JobAttestation = payload(&event, federation::ATTESTATION, &fid)?;
                let (_, job, state) = self.job(&a.job_id)?;
                member(&event, &state)?;
                a.validate_job(&job)?;
                ensure!(
                    event.created_at.as_secs() <= job.expires_at,
                    "expired attestation"
                );
                self.db
                    .put("attestation", &event.id.to_hex(), Some(&fid), &event)?;
                self.evaluate(&a.job_id)?;
            }
            federation::PROPOSAL => {
                let p: FederationProposal = payload(&event, federation::PROPOSAL, &fid)?;
                ensure!(p.next_state.epoch > 0, "invalid proposal epoch");
                let state = self.at_epoch(&fid, p.next_state.epoch - 1)?;
                member(&event, &state)?;
                chorus_governance::validate_proposal(&state, &p)?;
                self.db.put("proposal", &digest(&p)?, Some(&fid), &event)?;
            }
            federation::APPROVAL => {
                let a: FederationApproval = payload(&event, federation::APPROVAL, &fid)?;
                let state = self.at_epoch(&fid, a.epoch)?;
                member(&event, &state)?;
                let pe: Event = self
                    .db
                    .get("proposal", &a.proposal_hash)?
                    .context("proposal not available yet")?;
                let p: FederationProposal = payload(&pe, federation::PROPOSAL, &fid)?;
                chorus_governance::validate_proposal(&state, &p)?;
                ensure!(
                    a.protocol_version == VERSION
                        && a.parent_state_hash == digest(&state)?
                        && a.proposal_hash == digest(&p)?,
                    "invalid approval"
                );
                self.db
                    .put("approval", &event.id.to_hex(), Some(&fid), &event)?;
                self.try_transition(&fid, &a.proposal_hash)?;
            }
            federation::FINALIZED => {
                let packet: CertifiedResult = payload(&event, federation::FINALIZED, &fid)?;
                let (_, job, state) = self.job(&packet.job_id)?;
                member(&event, &state)?;
                let commitment = JobAttestation::from_execution(&job, &packet.output)?;
                let attestations = packet
                    .attestations
                    .iter()
                    .map(Event::from_json)
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let proof = FinalizedResult {
                    commitment,
                    attestations: attestations.clone(),
                    frost: None,
                };
                consensus::verify(&state, &job, &proof)?;
                self.db.store_blob(&packet.output)?;
                for a in attestations {
                    self.db.put("attestation", &a.id.to_hex(), Some(&fid), &a)?;
                }
                self.evaluate(&packet.job_id)?;
            }
            federation::STATE => {
                if let Ok(s) = payload::<FederationState>(&event, federation::STATE, &fid) {
                    let known = self.at_epoch(&fid, s.epoch)?;
                    ensure!(known == s, "unapproved federation state");
                    member(&event, &s)?;
                } else {
                    let certificate: CertifiedEpoch = payload(&event, federation::STATE, &fid)?;
                    let pe = Event::from_json(certificate.proposal)?;
                    let p: FederationProposal = payload(&pe, federation::PROPOSAL, &fid)?;
                    ensure!(p.next_state.epoch > 0, "invalid epoch");
                    let current = self.at_epoch(&fid, p.next_state.epoch - 1)?;
                    member(&event, &current)?;
                    member(&pe, &current)?;
                    let approvals = certificate
                        .approvals
                        .iter()
                        .map(Event::from_json)
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    let next = chorus_governance::approve(&current, &p, &approvals)?;
                    self.store_epoch(&EpochRecord {
                        state: next,
                        genesis: None,
                        proposal: Some(pe),
                        approvals,
                    })?;
                }
            }
            _ => anyhow::bail!("unsupported protocol event"),
        }
        self.db
            .put("received", &event.id.to_hex(), Some(&fid), &event)?;
        drop(_guard);
        if let Some(id) = execute {
            let _ = self.execute(&id).await;
        }
        Ok(())
    }
    pub fn proof(&self, id: &str) -> Result<ProofBundle> {
        let (event, job, state) = self.job(id)?;
        let result: FinalizedResult = self
            .db
            .get("final", id)?
            .context("job has no accepted result")?;
        let current = self.consensus(id)?;
        ensure!(
            current.accepted.as_ref() == Some(&digest(&result.commitment)?),
            "result disputed by additional evidence"
        );
        let mut invitation = self.invitation(&state.federation_id)?;
        invitation.epochs.retain(|e| e.state.epoch <= job.epoch);
        invitation.wasm_hex = hex::encode(self.db.blob(&state.wasm_hash)?);
        Ok(ProofBundle {
            protocol: "chorus/1".into(),
            invitation,
            job: event,
            output_hex: hex::encode(self.db.blob(&result.commitment.result_hash)?),
            result,
        })
    }
}
pub fn event_federation(event: &Event) -> Result<String> {
    event
        .tags
        .iter()
        .find_map(|t| {
            let s = t.as_slice();
            if s.first().is_some_and(|v| v == "h") {
                s.get(1).cloned()
            } else {
                None
            }
        })
        .context("missing federation tag")
}
