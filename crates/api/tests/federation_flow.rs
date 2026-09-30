use anyhow::Result;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chorus_api::{Api, engine::Engine};
use chorus_consensus::collect;
use chorus_federation::{self as federation, signed};
use chorus_fedvm::FedVm;
use chorus_nostr::Transport;
use chorus_protocol::*;
use chorus_storage::{Storage, now, token};
use http_body_util::BodyExt;
use nostr_sdk::prelude::*;
use tower::ServiceExt;

fn wasm(result: &[u8], state: Option<Vec<u8>>) -> Vec<u8> {
    let output = canonical(&OutputEnvelope {
        result: result.to_vec(),
        next_state: state,
        declared_actions: vec![],
    })
    .unwrap();
    let data = output
        .iter()
        .map(|v| format!("\\{v:02x}"))
        .collect::<String>();
    wat::parse_str(format!("(module (memory (export \"memory\") 1 256) (data (i32.const 8192) \"{data}\") (func (export \"alloc\") (param i32) (result i32) i32.const 0) (func (export \"execute\") (param i32 i32) (result i64) i64.const {}))",(8192u64<<32)|output.len() as u64)).unwrap()
}
async fn node(key: Keys) -> Result<Engine> {
    let t = Transport::new(key.clone(), &[]).await?;
    Engine::new(Storage::open(":memory:")?, FedVm::new()?, key, t)
}
fn state(keys: &[Keys], contract: &[u8], threshold: ThresholdRule) -> FederationState {
    let mut members = keys
        .iter()
        .map(|k| k.public_key().to_hex())
        .collect::<Vec<_>>();
    members.sort();
    FederationState {
        protocol_version: VERSION,
        federation_id: token(),
        epoch: 0,
        name: "Test federation".into(),
        description: "Independent execution test".into(),
        public: true,
        members,
        communication_keys: vec![],
        threshold,
        wasm_hash: hash(contract),
        vm_version: VM_VERSION.into(),
        initial_state: None,
        previous_federation_state_hash: None,
        created_at: now(),
        relays: vec![],
        frost: None,
    }
}
async fn network(
    n: usize,
    threshold: ThresholdRule,
) -> Result<(Vec<Engine>, Vec<Keys>, FederationState)> {
    let keys = (0..n).map(|_| Keys::generate()).collect::<Vec<_>>();
    let mut nodes = vec![];
    for key in &keys {
        nodes.push(node(key.clone()).await?);
    }
    let contract = wasm(b"verified", None);
    let s = nodes[0].create(state(&keys, &contract, threshold), &contract)?;
    let invite = nodes[0].invitation(&s.federation_id)?;
    for node in &nodes[1..] {
        node.join(&invite, &invite.genesis_hash)?;
    }
    Ok((nodes, keys, s))
}
async fn run(n: usize, required: usize) -> Result<()> {
    let (nodes, _, s) = network(n, ThresholdRule::Absolute(required as u32)).await?;
    let (id, _) = nodes[0].submit_job(&s, b"input".to_vec())?;
    nodes[0].execute(&id).await?;
    let event = nodes[0].db.get::<Event>("job", &id)?.unwrap();
    // One member may remain offline. Responses are independently executed, not fabricated.
    for node in nodes.iter().take(required).skip(1) {
        node.ingest(event.clone()).await?;
    }
    let mut attestations = vec![];
    for node in nodes.iter().take(required) {
        attestations.extend(node.attestations(&id, &s.federation_id)?);
    }
    for node in nodes.iter().take(required) {
        for a in &attestations {
            node.ingest(a.clone()).await?;
            node.ingest(a.clone()).await?;
        }
        let p = node.proof(&id)?;
        assert_eq!(p.result.attestations.len(), required);
        assert_eq!(
            hash(&hex::decode(&p.output_hex)?),
            p.result.commitment.result_hash
        );
    }
    Ok(())
}
#[tokio::test]
async fn three_nodes_one_offline_duplicates_and_proof() -> Result<()> {
    run(3, 2).await
}
#[tokio::test]
async fn ten_nodes_arbitrary_threshold() -> Result<()> {
    run(10, 8).await
}
#[test]
fn one_hundred_members_explicit_quorum_and_canonical_order() -> Result<()> {
    let keys = (0..100).map(|_| Keys::generate()).collect::<Vec<_>>();
    let mut s = state(&keys, &wasm(b"a", None), ThresholdRule::Absolute(67));
    s.validate()?;
    assert_eq!(s.threshold.required(s.members.len())?, 67);
    let h = digest(&s)?;
    s.members.swap(0, 1);
    assert!(s.validate().is_err());
    assert_ne!(h, digest(&s)?);
    Ok(())
}
#[tokio::test]
async fn old_threshold_governs_reduction_and_stale_approval_rejected() -> Result<()> {
    let (nodes, keys, s) = network(10, ThresholdRule::Absolute(8)).await?;
    let mut next = s.clone();
    next.threshold = ThresholdRule::Absolute(6);
    let pid = nodes[0].propose(&s.federation_id, next)?;
    let proposal = nodes[0].db.get::<Event>("proposal", &pid)?.unwrap();
    for node in &nodes[1..] {
        node.ingest(proposal.clone()).await?;
    }
    for (i, node) in nodes.iter().take(8).enumerate() {
        node.approve(&s.federation_id, &pid)?;
        for approval in node.db.list::<Event>("approval", Some(&s.federation_id))? {
            nodes[0].ingest(approval).await?;
        }
        assert_eq!(
            nodes[0].active(&s.federation_id)?.epoch,
            if i < 7 { 0 } else { 1 }
        );
    }
    assert_eq!(
        nodes[0].active(&s.federation_id)?.threshold.required(10)?,
        6
    );
    let approval = FederationApproval {
        protocol_version: VERSION,
        federation_id: s.federation_id.clone(),
        epoch: 0,
        parent_state_hash: digest(&s)?,
        proposal_hash: token(),
    };
    assert!(
        nodes[0]
            .ingest(signed(
                &keys[0],
                federation::APPROVAL,
                &s.federation_id,
                &approval
            )?)
            .await
            .is_err()
    );
    Ok(())
}
#[tokio::test]
async fn membership_change_and_wasm_upgrade_preserve_history() -> Result<()> {
    let (nodes, _, s) = network(3, ThresholdRule::Absolute(2)).await?;
    let extra = Keys::generate();
    let contract = wasm(b"upgraded", None);
    nodes[0].db.store_blob(&contract)?;
    let mut next = s.clone();
    next.members.push(extra.public_key().to_hex());
    next.communication_keys
        .push((extra.public_key().to_hex(), extra.public_key().to_hex()));
    next.threshold = ThresholdRule::Absolute(3);
    next.wasm_hash = hash(&contract);
    let pid = nodes[0].propose(&s.federation_id, next)?;
    let pe = nodes[0].db.get::<Event>("proposal", &pid)?.unwrap();
    nodes[1].ingest(pe).await?;
    nodes[1].db.store_blob(&contract)?;
    for node in &nodes[..2] {
        node.approve(&s.federation_id, &pid)?;
        for e in node.db.list::<Event>("approval", Some(&s.federation_id))? {
            nodes[0].ingest(e).await?;
        }
    }
    assert_eq!(nodes[0].active(&s.federation_id)?.members.len(), 4);
    assert_eq!(nodes[0].at_epoch(&s.federation_id, 0)?, s);
    assert_eq!(
        nodes[0].active(&s.federation_id)?.wasm_hash,
        hash(&contract)
    );
    let invitation = nodes[0].invitation(&s.federation_id)?;
    let fresh = node(extra).await?;
    fresh.join(&invitation, &invitation.genesis_hash)?;
    assert_eq!(fresh.active(&s.federation_id)?.epoch, 1);
    // Removing a member and reducing 3/4 to 2/3 requires three OLD members.
    let mut next = nodes[0].active(&s.federation_id)?;
    next.members
        .retain(|m| m != &nodes[2].member_key().to_hex());
    next.communication_keys
        .retain(|(member, _)| next.members.contains(member));
    next.threshold = ThresholdRule::Absolute(2);
    let pid = nodes[0].propose(&s.federation_id, next)?;
    for n in &nodes[1..] {
        n.join(&invitation, &invitation.genesis_hash)?;
        n.ingest(nodes[0].db.get::<Event>("proposal", &pid)?.unwrap())
            .await?;
    }
    for n in &nodes {
        n.approve(&s.federation_id, &pid)?;
        for e in n.db.list::<Event>("approval", Some(&s.federation_id))? {
            nodes[0].ingest(e).await?;
        }
    }
    assert_eq!(nodes[0].active(&s.federation_id)?.members.len(), 3);
    Ok(())
}
#[tokio::test]
async fn conflicting_results_equivocation_and_forgery() -> Result<()> {
    let (nodes, keys, s) = network(3, ThresholdRule::Absolute(2)).await?;
    let (id, j) = nodes[0].submit_job(&s, vec![])?;
    nodes[0].execute(&id).await?;
    let a = nodes[0].attestations(&id, &s.federation_id)?[0].clone();
    let mut c: JobAttestation = federation::payload(&a, federation::ATTESTATION, &s.federation_id)?;
    c.result_hash = token();
    let conflict = signed(&keys[1], federation::ATTESTATION, &s.federation_id, &c)?;
    let result = collect(&s, &j, &[a.clone(), conflict.clone()])?;
    assert!(result.accepted.is_none());
    assert_eq!(result.groups.len(), 2);
    let equiv = signed(&keys[0], federation::ATTESTATION, &s.federation_id, &c)?;
    let result = collect(&s, &j, &[a.clone(), conflict, equiv])?;
    assert_eq!(result.equivocators.len(), 1);
    assert!(result.accepted.is_none());
    let mut forged = a;
    forged.content.push('0');
    assert!(nodes[0].ingest(forged).await.is_err());
    c.epoch += 1;
    assert!(
        nodes[0]
            .ingest(signed(
                &keys[2],
                federation::ATTESTATION,
                &s.federation_id,
                &c
            )?)
            .await
            .is_err()
    );
    Ok(())
}
#[tokio::test]
async fn minority_and_half_quorums_rejected_in_genesis_and_governance() -> Result<()> {
    let (nodes, _, s) = network(10, ThresholdRule::Absolute(6)).await?;
    for k in [0, 1, 5, 11] {
        let mut invalid = s.clone();
        invalid.threshold = ThresholdRule::Absolute(k);
        assert!(invalid.validate().is_err());
        assert!(nodes[0].propose(&s.federation_id, invalid).is_err());
    }
    // Increasing the member count cannot silently turn an old quorum into a minority.
    let mut next = s.clone();
    next.members.push(Keys::generate().public_key().to_hex());
    next.members.push(Keys::generate().public_key().to_hex());
    assert!(nodes[0].propose(&s.federation_id, next).is_err());
    Ok(())
}
#[tokio::test]
async fn stateful_conflicting_local_jobs_never_sign_twice() -> Result<()> {
    let key = Keys::generate();
    let node = node(key.clone()).await?;
    let contract = wasm(b"result", Some(b"next".to_vec()));
    let mut s = state(&[key], &contract, ThresholdRule::Absolute(1));
    s.initial_state = Some(vec![]);
    let s = node.create(s, &contract)?;
    let (a, _) = node.submit_job(&s, vec![1])?;
    let (b, _) = node.submit_job(&s, vec![2])?;
    node.execute(&a).await?;
    assert!(node.execute(&b).await.is_err());
    assert!(node.attestations(&b, &s.federation_id)?.is_empty());
    Ok(())
}
#[tokio::test]
async fn nip17_roundtrip_and_wrong_recipient() -> Result<()> {
    let sender = Keys::generate();
    let receiver = Keys::generate();
    let outsider = Keys::generate();
    let k = Keys::generate();
    let s = state(
        std::slice::from_ref(&k),
        &wasm(b"hello", None),
        ThresholdRule::Absolute(1),
    );
    let event = signed(&k, federation::STATE, &s.federation_id, &s)?;
    let wrapped = PrivateDirectMessageBuilder::new(receiver.public_key(), event.as_json())
        .finalize(&sender)?;
    assert!(!wrapped.content.contains(&s.name));
    let t = Transport::new(receiver, &[]).await?;
    assert_eq!(t.unwrap(wrapped.clone()).await?, event);
    assert!(
        Transport::new(outsider, &[])
            .await?
            .unwrap(wrapped)
            .await
            .is_err()
    );
    Ok(())
}
#[tokio::test]
async fn pin_and_history_substitution_rejected() -> Result<()> {
    let (nodes, _, s) = network(3, ThresholdRule::Absolute(2)).await?;
    let mut invite = nodes[0].invitation(&s.federation_id)?;
    assert!(nodes[1].join(&invite, &token()).is_err());
    invite.epochs[0].state.threshold = ThresholdRule::Absolute(1);
    assert!(nodes[1].join(&invite, &invite.genesis_hash).is_err());
    Ok(())
}
#[tokio::test]
async fn http_auth_csrf_private_visibility_and_logout() -> Result<()> {
    let n = node(Keys::generate()).await?;
    n.db.init_admin("admin", "testing-password-123")?;
    let router = chorus_api::router(
        Api {
            engine: n.clone(),
            origin: "https://node.example".into(),
            secure_cookies: true,
            session_ttl: 3600,
        },
        None,
    );
    let login = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/login")
                .header("origin", "https://node.example")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"username":"admin","password":"testing-password-123"}"#,
                ))?,
        )
        .await?;
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login.headers()["set-cookie"].to_str()?.to_owned();
    assert!(
        cookie.contains("HttpOnly")
            && cookie.contains("Secure")
            && cookie.contains("SameSite=Strict")
    );
    let data: serde_json::Value =
        serde_json::from_slice(&login.into_body().collect().await?.to_bytes())?;
    let csrf = data["csrf"].as_str().unwrap();
    let deny = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/logout")
                .header("cookie", &cookie)
                .header("origin", "https://evil.example")
                .header("x-csrf-token", csrf)
                .body(Body::empty())?,
        )
        .await?;
    assert_ne!(deny.status(), StatusCode::OK);
    let logout = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/logout")
                .header("cookie", &cookie)
                .header("origin", "https://node.example")
                .header("x-csrf-token", csrf)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(logout.status(), StatusCode::OK);
    let denied = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/settings")
                .header("cookie", cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_ne!(denied.status(), StatusCode::OK);
    Ok(())
}
#[tokio::test]
async fn frost_dkg_and_threshold_signature_after_execution_agreement() -> Result<()> {
    use frost_ed25519 as frost;
    use std::collections::BTreeMap;
    let (nodes, _, s) = network(3, ThresholdRule::Absolute(2)).await?;
    let (id, job) = nodes[0].submit_job(&s, vec![])?;
    nodes[0].execute(&id).await?;
    nodes[1]
        .ingest(nodes[0].db.get::<Event>("job", &id)?.unwrap())
        .await?;
    for e in nodes[1].attestations(&id, &s.federation_id)? {
        nodes[0].ingest(e).await?;
    }
    let proof = nodes[0].proof(&id)?;
    let accepted = chorus_consensus::verify(&s, &job, &proof.result)?;
    let mut r1s = BTreeMap::new();
    let mut r1p = BTreeMap::new();
    for i in 1u16..=3 {
        let id = frost::Identifier::try_from(i)?;
        let (secret, package) = frost::keys::dkg::part1(id, 3, 2, rand::rngs::OsRng)?;
        r1s.insert(id, secret);
        r1p.insert(id, package);
    }
    let mut r2s = BTreeMap::new();
    let mut r2p = BTreeMap::new();
    for (id, secret) in r1s {
        let others = r1p
            .iter()
            .filter(|(k, _)| **k != id)
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        let (secret, packages) = frost::keys::dkg::part2(secret, &others)?;
        r2s.insert(id, secret);
        r2p.insert(id, packages);
    }
    let mut keys = BTreeMap::new();
    let mut public = None;
    for (id, secret) in r2s {
        let others = r1p
            .iter()
            .filter(|(k, _)| **k != id)
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        let incoming = r2p
            .iter()
            .filter(|(k, _)| **k != id)
            .map(|(k, v)| (*k, v[&id].clone()))
            .collect();
        let (key, pubkey) = frost::keys::dkg::part3(&secret, &others, &incoming)?;
        keys.insert(id, key);
        public = Some(pubkey);
    }
    let session = token();
    let mut rounds = BTreeMap::new();
    let mut commitments = BTreeMap::new();
    for (id, key) in keys.iter().take(2) {
        let round = chorus_frost::SigningRound::begin(key, &accepted, &session)?;
        commitments.insert(*id, round.commitment);
        rounds.insert(*id, round);
    }
    let package =
        frost::SigningPackage::new(commitments, &chorus_frost::message(&accepted, &session)?);
    let mut shares = BTreeMap::new();
    for (id, round) in rounds {
        shares.insert(id, round.finish(&keys[&id], &package)?);
    }
    let public = public.unwrap();
    let signature = chorus_frost::aggregate(&package, &shares, &public)?;
    let metadata = FrostMetadata {
        ciphersuite: "FROST-ED25519-SHA512-v1".into(),
        verifying_key: public.verifying_key().serialize()?,
        participants: vec![],
        required: 2,
        setup_id: token(),
    };
    let frost_proof = chorus_consensus::FrostProof {
        session_id: session,
        signature,
    };
    chorus_frost::verify(&accepted, &metadata, &frost_proof)?;
    let mut replay = frost_proof;
    replay.session_id = token();
    assert!(chorus_frost::verify(&accepted, &metadata, &replay).is_err());
    Ok(())
}
#[tokio::test]
async fn export_reproducible_cli_fixture() -> Result<()> {
    let (nodes, _, s) = network(3, ThresholdRule::Absolute(2)).await?;
    let (id, _) = nodes[0].submit_job(&s, b"example".to_vec())?;
    nodes[0].execute(&id).await?;
    nodes[1]
        .ingest(nodes[0].db.get::<Event>("job", &id)?.unwrap())
        .await?;
    for e in nodes[1].attestations(&id, &s.federation_id)? {
        nodes[0].ingest(e).await?;
    }
    let proof = nodes[0].proof(&id)?;
    if let Ok(path) = std::env::var("CHORUS_TEST_PROOF") {
        std::fs::write(path, serde_json::to_vec_pretty(&proof)?)?;
    }
    Ok(())
}

#[path = "../../nostr/tests/common/mod.rs"]
mod relay_support;
#[tokio::test]
async fn clean_node_syncs_executes_and_verifies_through_multiple_relays() -> Result<()> {
    relay_federation(true).await
}
#[tokio::test]
async fn private_federation_uses_one_identity_to_join_execute_and_sync() -> Result<()> {
    relay_federation(false).await
}
async fn relay_federation(public: bool) -> Result<()> {
    use std::time::Duration;
    let r1 = relay_support::relay().await?;
    let r2 = relay_support::relay().await?;
    let keys = (0..3).map(|_| Keys::generate()).collect::<Vec<_>>();
    let mut nodes = vec![];
    for key in &keys {
        let transport = Transport::new(
            key.clone(),
            &[
                relay_support::config(&r1.url),
                relay_support::config(&r2.url),
            ],
        )
        .await?;
        transport
            .client
            .connect()
            .and_wait(Duration::from_secs(3))
            .await;
        nodes.push(Engine::new(
            Storage::open(":memory:")?,
            FedVm::new()?,
            key.clone(),
            transport,
        )?);
    }
    let contract = wasm(b"over actual relays", None);
    let mut initial = state(&keys, &contract, ThresholdRule::Absolute(2));
    initial.public = public;
    let s = nodes[0].create(initial, &contract)?;
    assert!(
        s.communication_keys
            .iter()
            .all(|(member, inbox)| member == inbox)
    );
    let invite = nodes[0].invitation(&s.federation_id)?;
    nodes[1].join(&invite, &invite.genesis_hash)?;
    let (id, _) = nodes[0].submit_job(&s, b"network input".to_vec())?;
    nodes[0].execute(&id).await?;
    // Deliver only via WebSockets and backfill; no cross-node storage or attestations are copied.
    for _ in 0..3 {
        for n in &nodes[..2] {
            for (event, recipients) in n.db.pending_events()? {
                n.transport.publish(&event, &recipients).await?;
                n.db.mark_sent(&event.id.to_hex())?;
            }
        }
        for n in &nodes[..2] {
            let events = if public {
                n.transport.backfill(&s.federation_id).await?
            } else {
                n.transport.private_backfill().await?
            };
            for event in events {
                let _ = n.ingest(event).await;
            }
        }
    }
    assert!(nodes[0].proof(&id).is_ok());
    // A previously offline clean member joins with a pinned invitation and reconstructs relay history.
    nodes[2].join(&invite, &invite.genesis_hash)?;
    for _ in 0..2 {
        let events = if public {
            nodes[2].transport.backfill(&s.federation_id).await?
        } else {
            nodes[2].transport.private_backfill().await?
        };
        for event in events {
            let _ = nodes[2].ingest(event).await;
        }
    }
    let proof = nodes[2].proof(&id)?;
    let (_, job, epoch) = nodes[2].job(&id)?;
    chorus_consensus::verify(&epoch, &job, &proof.result)?;
    let output = FedVm::new()?.execute(
        &hex::decode(&proof.invitation.wasm_hex)?,
        &epoch.wasm_hash,
        &job.input,
    )?;
    assert_eq!(hex::encode(output), proof.output_hex);
    for n in nodes {
        n.transport.client.shutdown().await;
    }
    Ok(())
}

#[tokio::test]
async fn cross_job_equivocation_withdraws_proof_but_preserves_public_evidence() -> Result<()> {
    let keys = (0..3).map(|_| Keys::generate()).collect::<Vec<_>>();
    let engine = node(keys[0].clone()).await?;
    let contract = wasm(b"result", Some(b"next".to_vec()));
    let mut state = state(&keys, &contract, ThresholdRule::Absolute(2));
    state.initial_state = Some(vec![]);
    let state = engine.create(state, &contract)?;
    let (first, _) = engine.submit_job(&state, vec![1])?;
    let (second, _) = engine.submit_job(&state, vec![2])?;
    engine.execute(&first).await?;
    let (_, job, _) = engine.job(&first)?;
    let output = engine.vm.execute(&contract, &state.wasm_hash, &job.input)?;
    let commitment = JobAttestation::from_execution(&job, &output)?;
    engine
        .ingest(signed(
            &keys[1],
            federation::ATTESTATION,
            &state.federation_id,
            &commitment,
        )?)
        .await?;
    engine.proof(&first)?;
    // A Byzantine member signs a second job for the already committed sequence.
    let (_, other, _) = engine.job(&second)?;
    let conflicting = JobAttestation::from_execution(&other, &output)?;
    engine
        .ingest(signed(
            &keys[1],
            federation::ATTESTATION,
            &state.federation_id,
            &conflicting,
        )?)
        .await?;
    assert!(engine.active(&state.federation_id).is_err());
    assert!(engine.proof(&first).is_err());
    assert_eq!(
        engine.consensus(&first)?.equivocators,
        vec![keys[1].public_key().to_hex()]
    );
    let app = chorus_api::router(
        Api {
            engine,
            origin: "https://node.example".into(),
            secure_cookies: true,
            session_ttl: 3600,
        },
        None,
    );
    for path in ["jobs", "proposals"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/federations/{}/{path}",
                        state.federation_id
                    ))
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        if path == "jobs" {
            let rows: serde_json::Value =
                serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
            assert!(
                rows.as_array()
                    .unwrap()
                    .iter()
                    .all(|row| row["status"] == "disputed")
            );
        }
    }
    Ok(())
}
