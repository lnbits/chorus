use anyhow::Result;
use chorus_api::engine::Engine;
use chorus_fedvm::FedVm;
use chorus_nostr::Transport;
use chorus_protocol::*;
use chorus_storage::{Storage, load_identity, lock_node, now, token, unseal};
use nostr_sdk::prelude::*;
use serde_json::json;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Result<Self> {
        let p = std::env::temp_dir().join(format!("chorus-identity-{}", token()));
        std::fs::create_dir(&p)?;
        Ok(Self(p))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn cli(dir: &Path, arguments: &[&str], input: serde_json::Value) -> Result<Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_chorus-node"))
        .arg("--data-dir")
        .arg(dir)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(serde_json::to_string(&input)?.as_bytes())?;
    Ok(child.wait_with_output()?)
}
#[tokio::test]
async fn imported_identity_replacement_preserves_history_and_encrypted_backup() -> Result<()> {
    let dir = Directory::new()?;
    let old = Keys::generate();
    let new = Keys::generate();
    let old_nsec = old.secret_key().to_bech32()?;
    let new_nsec = new.secret_key().to_bech32()?;
    let password = "keystore-test-password";
    let init = cli(
        &dir.0,
        &["init", "--credentials-stdin"],
        json!({"admin_password":"admin-test-password", "key_password":password, "nsec":old_nsec}),
    )?;
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    assert!(!dir.0.join("communication.vault").exists());
    assert_eq!(
        load_identity(&dir.0, password)?.public_key(),
        old.public_key()
    );
    let original = std::fs::read(dir.0.join("member.vault"))?;
    let input = json!({"key_password":password, "nsec":new_nsec});
    {
        let _running_node = lock_node(&dir.0)?;
        let response = cli(
            &dir.0,
            &["replace-key", "--credentials-stdin"],
            input.clone(),
        )?;
        assert!(!response.status.success());
        assert!(String::from_utf8_lossy(&response.stderr).contains("node is running"));
    }
    for invalid in [
        json!({"key_password":"wrong-password", "nsec":new_nsec}),
        json!({"key_password":password,"nsec":"invalid-nsec"}),
    ] {
        assert!(
            !cli(&dir.0, &["replace-key", "--credentials-stdin"], invalid)?
                .status
                .success()
        );
        assert_eq!(std::fs::read(dir.0.join("member.vault"))?, original);
    }
    let db = Storage::open(dir.0.join("chorus.sqlite"))?;
    let engine = Engine::new(
        db.clone(),
        FedVm::new()?,
        old.clone(),
        Transport::new(old.clone(), &[]).await?,
    )?;
    let wasm = include_bytes!("../../../examples/echo.wasm");
    let state = engine.create(
        FederationState {
            protocol_version: VERSION,
            federation_id: token(),
            epoch: 0,
            name: "Preserve membership".into(),
            description: String::new(),
            public: true,
            members: vec![old.public_key().to_hex()],
            communication_keys: vec![],
            threshold: ThresholdRule::Absolute(1),
            wasm_hash: hash(wasm),
            vm_version: VM_VERSION.into(),
            initial_state: None,
            previous_federation_state_hash: None,
            created_at: now(),
            relays: vec![],
            frost: None,
        },
        wasm,
    )?;
    db.reserve_signature(&old.public_key().to_hex(), "test-scope", "original-choice")?;
    let denied = cli(
        &dir.0,
        &["replace-key", "--credentials-stdin"],
        input.clone(),
    )?;
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("still a member"));
    assert_eq!(std::fs::read(dir.0.join("member.vault"))?, original);
    let changed = cli(
        &dir.0,
        &[
            "replace-key",
            "--credentials-stdin",
            "--allow-membership-loss",
        ],
        input,
    )?;
    assert!(
        changed.status.success(),
        "{}",
        String::from_utf8_lossy(&changed.stderr)
    );
    assert_eq!(
        load_identity(&dir.0, password)?.public_key(),
        new.public_key()
    );
    assert_eq!(
        db.setting::<String>("member_public_key")?,
        Some(new.public_key().to_hex())
    );
    assert_eq!(engine.at_epoch(&state.federation_id, 0)?, state);
    assert!(
        db.reserve_signature(
            &old.public_key().to_hex(),
            "test-scope",
            "conflicting-choice"
        )
        .is_err()
    );
    let backup = std::fs::read_dir(&dir.0)?
        .filter_map(|e| e.ok())
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("member-backup-")
        })
        .unwrap()
        .path();
    assert_eq!(std::fs::read(&backup)?, original);
    let restored = unseal(&std::fs::read(backup)?, password, "member")?;
    assert_eq!(
        Keys::new(SecretKey::from_slice(&restored)?).public_key(),
        old.public_key()
    );
    for output in [&init, &changed] {
        for secret in [&old_nsec, &new_nsec] {
            assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
            assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
        }
    }
    Ok(())
}
