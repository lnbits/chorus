//! SQLite durability and an authenticated encrypted keystore. No keys leave this crate as DTOs.
use anyhow::{Context, Result, ensure};
use argon2::{
    Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version,
    password_hash::{PasswordHash, SaltString},
};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use chorus_protocol::hash;
use rand::{RngCore, rngs::OsRng};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Storage(Arc<Mutex<Connection>>);
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn token() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    hex::encode(b)
}
fn argon() -> Argon2<'static> {
    Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(65536, 3, 1, Some(32)).expect("constant Argon2 parameters"),
    )
}
pub fn password_hash(password: &str) -> Result<String> {
    ensure!(
        (12..=1024).contains(&password.len()),
        "password must contain 12–1024 bytes"
    );
    argon()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|h| h.to_string())
        .map_err(|_| anyhow::anyhow!("password hashing failed"))
}
pub fn password_matches(password: &str, encoded: &str) -> bool {
    password.len() <= 1024
        && PasswordHash::new(encoded)
            .is_ok_and(|h| argon().verify_password(password.as_bytes(), &h).is_ok())
}
const MIGRATION: &str = include_str!("../migrations/001.sql");
impl Storage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let c = Connection::open(path)?;
        c.busy_timeout(std::time::Duration::from_secs(10))?;
        c.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;",
        )?;
        c.execute_batch(MIGRATION)?;
        Ok(Self(Arc::new(Mutex::new(c))))
    }
    pub fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.0
            .lock()
            .map_err(|_| anyhow::anyhow!("database unavailable"))
    }
    pub fn put<T: Serialize>(
        &self,
        kind: &str,
        id: &str,
        federation: Option<&str>,
        value: &T,
    ) -> Result<bool> {
        let json = serde_json::to_string(value)?;
        let c = self.connection()?;
        if let Some(existing) = c
            .query_row(
                "SELECT value FROM records WHERE kind=?1 AND id=?2",
                params![kind, id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            ensure!(existing == json, "immutable record conflict");
            return Ok(false);
        }
        Ok(c.execute(
            "INSERT INTO records(kind,id,federation,value,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![kind, id, federation, json, now()],
        )? == 1)
    }
    pub fn get<T: DeserializeOwned>(&self, kind: &str, id: &str) -> Result<Option<T>> {
        let json: Option<String> = self
            .connection()?
            .query_row(
                "SELECT value FROM records WHERE kind=?1 AND id=?2",
                params![kind, id],
                |r| r.get(0),
            )
            .optional()?;
        json.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub fn list<T: DeserializeOwned>(
        &self,
        kind: &str,
        federation: Option<&str>,
    ) -> Result<Vec<T>> {
        let c = self.connection()?;
        let mut s=c.prepare("SELECT value FROM records WHERE kind=?1 AND (?2 IS NULL OR federation=?2) ORDER BY created_at,id")?;
        let rows = s.query_map(params![kind, federation], |r| r.get::<_, String>(0))?;
        rows.map(|s| Ok(serde_json::from_str(&s?)?)).collect()
    }
    pub fn store_blob(&self, bytes: &[u8]) -> Result<String> {
        let id = hash(bytes);
        self.connection()?.execute(
            "INSERT OR IGNORE INTO blobs(hash,bytes) VALUES(?1,?2)",
            params![id, bytes],
        )?;
        Ok(id)
    }
    pub fn blob(&self, id: &str) -> Result<Vec<u8>> {
        self.connection()?
            .query_row("SELECT bytes FROM blobs WHERE hash=?1", [id], |r| r.get(0))
            .context("blob not found")
    }
    /// Persist the choice BEFORE signing. Retrying identical content is safe; a conflicting choice is never signed.
    pub fn reserve_signature(&self, identity: &str, scope: &str, commitment: &str) -> Result<()> {
        let mut c = self.connection()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old: Option<String> = tx
            .query_row(
                "SELECT commitment FROM signing_guards WHERE identity=?1 AND scope=?2",
                params![identity, scope],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = old {
            ensure!(
                old == commitment,
                "refusing to sign a conflicting commitment"
            );
        } else {
            tx.execute(
                "INSERT INTO signing_guards(identity,scope,commitment) VALUES(?1,?2,?3)",
                params![identity, scope, commitment],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn activity(
        &self,
        kind: &str,
        federation: Option<&str>,
        target: &str,
        message: &str,
    ) -> Result<()> {
        self.connection()?.execute("INSERT INTO activity(kind,federation,target,message,created_at) VALUES(?1,?2,?3,?4,?5)",params![kind,federation,target,message,now()])?;
        Ok(())
    }
    pub fn activities(&self) -> Result<Vec<serde_json::Value>> {
        let c = self.connection()?;
        let mut s=c.prepare("SELECT id,kind,federation,target,message,created_at FROM activity ORDER BY id DESC LIMIT 500")?;
        Ok(s.query_map([],|r|Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"kind":r.get::<_,String>(1)?,"federation_id":r.get::<_,Option<String>>(2)?,"target":r.get::<_,String>(3)?,"message":r.get::<_,String>(4)?,"created_at":r.get::<_,u64>(5)?})))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.connection()?.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,serde_json::to_string(value)?])?;
        Ok(())
    }
    pub fn setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let j: Option<String> = self
            .connection()?
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()?;
        j.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub fn init_admin(&self, user: &str, password: &str) -> Result<()> {
        ensure!(!user.is_empty() && user.len() <= 64, "invalid username");
        let h = password_hash(password)?;
        self.connection()?.execute(
            "INSERT INTO admin(id,username,password_hash) VALUES(1,?1,?2)",
            params![user, h],
        )?;
        Ok(())
    }
    pub fn admin(&self) -> Result<(String, String)> {
        Ok(self.connection()?.query_row(
            "SELECT username,password_hash FROM admin WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }
    /// Global account backoff is persistent, so restarting cannot reset it. Proxy headers are not trusted.
    pub fn begin_login(&self) -> Result<bool> {
        let mut c = self.connection()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (failures, until): (u32, u64) = tx.query_row(
            "SELECT failures,blocked_until FROM admin WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if now() < until {
            return Ok(false);
        }
        let delay = 2u64.pow(failures.min(9)).min(900);
        tx.execute(
            "UPDATE admin SET failures=failures+1,blocked_until=?1 WHERE id=1",
            [now() + delay],
        )?;
        tx.commit()?;
        Ok(true)
    }
    pub fn create_session(&self, ttl: u64) -> Result<(String, String)> {
        let t = token();
        let csrf = token();
        let mut c = self.connection()?;
        let tx = c.transaction()?;
        tx.execute("UPDATE admin SET failures=0,blocked_until=0 WHERE id=1", [])?;
        tx.execute("DELETE FROM sessions WHERE expires_at<=?1", [now()])?;
        tx.execute(
            "INSERT INTO sessions(token_hash,csrf,created_at,expires_at) VALUES(?1,?2,?3,?4)",
            params![hash(t.as_bytes()), csrf, now(), now() + ttl],
        )?;
        tx.commit()?;
        Ok((t, csrf))
    }
    pub fn session(&self, t: &str) -> Result<Option<String>> {
        Ok(self
            .connection()?
            .query_row(
                "SELECT csrf FROM sessions WHERE token_hash=?1 AND expires_at>?2",
                params![hash(t.as_bytes()), now()],
                |r| r.get(0),
            )
            .optional()?)
    }
    pub fn logout(&self, t: &str) -> Result<()> {
        self.connection()?.execute(
            "DELETE FROM sessions WHERE token_hash=?1",
            [hash(t.as_bytes())],
        )?;
        Ok(())
    }
    pub fn change_password(&self, old: &str, new: &str) -> Result<()> {
        let (_, stored) = self.admin()?;
        ensure!(
            password_matches(old, &stored),
            "current password is incorrect"
        );
        let encoded = password_hash(new)?;
        let mut c = self.connection()?;
        let tx = c.transaction()?;
        tx.execute("UPDATE admin SET password_hash=?1 WHERE id=1", [encoded])?;
        tx.execute("DELETE FROM sessions", [])?;
        tx.commit()?;
        Ok(())
    }
    pub fn sessions(&self) -> Result<Vec<serde_json::Value>> {
        let c = self.connection()?;
        let mut s=c.prepare("SELECT created_at,expires_at FROM sessions WHERE expires_at>?1 ORDER BY created_at DESC")?;
        Ok(s.query_map([now()], |r| {
            Ok(serde_json::json!({"created_at":r.get::<_,u64>(0)?,"expires_at":r.get::<_,u64>(1)?}))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn queue_event(
        &self,
        event: &nostr_sdk::prelude::Event,
        recipients: &[String],
    ) -> Result<()> {
        self.connection()?.execute(
            "INSERT OR IGNORE INTO outbox(event_id,event,recipients) VALUES(?1,?2,?3)",
            params![
                event.id.to_hex(),
                serde_json::to_string(event)?,
                serde_json::to_string(recipients)?
            ],
        )?;
        Ok(())
    }
    pub fn pending_events(&self) -> Result<Vec<(nostr_sdk::prelude::Event, Vec<String>)>> {
        let c = self.connection()?;
        let mut s = c.prepare("SELECT event,recipients FROM outbox WHERE sent=0 LIMIT 100")?;
        let rows = s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.map(|r| {
            let (a, b) = r?;
            Ok((serde_json::from_str(&a)?, serde_json::from_str(&b)?))
        })
        .collect()
    }
    pub fn mark_sent(&self, id: &str) -> Result<()> {
        self.connection()?
            .execute("UPDATE outbox SET sent=1 WHERE event_id=?1", [id])?;
        Ok(())
    }
}
/// Held for the lifetime of a node or keystore mutation to prevent replacing a live key.
pub fn lock_node(dir: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(dir.join("node.lock"))?;
    file.try_lock().map_err(|_| {
        anyhow::anyhow!("node is running or the keystore is busy; stop it before changing keys")
    })?;
    Ok(file)
}
fn private_write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
/// Caller must hold lock_node. Keeps the old encrypted vault and all database history.
pub fn replace_identity(
    dir: &Path,
    password: &str,
    nsec: &str,
) -> Result<(nostr_sdk::prelude::Keys, std::path::PathBuf)> {
    use nostr_sdk::prelude::{FromBech32, Keys, SecretKey};
    let old = load_identity(dir, password)?;
    let keys = Keys::new(
        SecretKey::from_bech32(nsec.trim()).map_err(|_| anyhow::anyhow!("invalid Nostr nsec"))?,
    );
    ensure!(
        keys.public_key() != old.public_key(),
        "this nsec is already the active identity"
    );
    let bytes = Zeroizing::new(keys.secret_key().to_secret_bytes());
    let encrypted = seal(bytes.as_ref(), password, "member")?;
    let backup = dir.join(format!(
        "member-backup-{}-{}.vault",
        old.public_key().to_hex(),
        token()
    ));
    private_write_new(&backup, &std::fs::read(dir.join("member.vault"))?)?;
    let temporary = dir.join(format!(".member-{}.tmp", token()));
    private_write_new(&temporary, &encrypted)?;
    // Persist the backup before committing the atomic replacement.
    #[cfg(unix)]
    std::fs::File::open(dir)?.sync_all()?;
    if let Err(error) = std::fs::rename(&temporary, dir.join("member.vault")) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.into());
    }
    #[cfg(unix)]
    std::fs::File::open(dir)?.sync_all()?;
    Ok((keys, backup))
}
/// One persistent Nostr identity for signing and messaging. The legacy filename/AAD
/// preserve existing membership and signing reservations without rewriting history.
pub fn initialize_identity(
    dir: &Path,
    password: &str,
    nsec: Option<&str>,
) -> Result<nostr_sdk::prelude::Keys> {
    use nostr_sdk::prelude::{FromBech32, Keys, SecretKey};
    use std::io::Write;
    let keys = match nsec {
        Some(value) => Keys::new(
            SecretKey::from_bech32(value.trim())
                .map_err(|_| anyhow::anyhow!("invalid Nostr nsec"))?,
        ),
        None => Keys::generate(),
    };
    let secret = Zeroizing::new(keys.secret_key().to_secret_bytes());
    let encrypted = seal(secret.as_ref(), password, "member")?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(dir.join("member.vault"))?;
    file.write_all(&encrypted)?;
    file.sync_all()?;
    Ok(keys)
}
pub fn load_identity(dir: &Path, password: &str) -> Result<nostr_sdk::prelude::Keys> {
    use nostr_sdk::prelude::{Keys, SecretKey};
    let bytes = unseal(
        &std::fs::read(dir.join("member.vault"))?,
        password,
        "member",
    )?;
    Ok(Keys::new(SecretKey::from_slice(&bytes)?))
}
/// Existing two-key nodes retain their old inbox only for decrypting historical
/// messages. It is never used to sign or encrypt a new outgoing message.
pub fn load_legacy_inbox(dir: &Path, password: &str) -> Result<Option<nostr_sdk::prelude::Keys>> {
    use nostr_sdk::prelude::{Keys, SecretKey};
    let path = dir.join("communication.vault");
    let blob = match std::fs::read(path) {
        Ok(blob) => blob,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let bytes = unseal(&blob, password, "communication")?;
    Ok(Some(Keys::new(SecretKey::from_slice(&bytes)?)))
}
/// The master password is read from the terminal or a systemd credential file, never from HTTP.
/// Keys are distinct by purpose and encrypted with random XChaCha20 nonce + Argon2id salt.
pub fn seal(secret: &[u8], password: &str, purpose: &str) -> Result<Vec<u8>> {
    ensure!(
        password.len() >= 12,
        "keystore password must contain at least 12 bytes"
    );
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);
    let mut key = Zeroizing::new([0u8; 32]);
    argon()
        .hash_password_into(password.as_bytes(), &salt, key.as_mut())
        .map_err(|_| anyhow::anyhow!("key derivation failed"))?;
    let ciphertext = XChaCha20Poly1305::new_from_slice(key.as_ref())
        .map_err(|_| anyhow::anyhow!("invalid key"))?
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: secret,
                aad: purpose.as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("key encryption failed"))?;
    let mut out = b"CHKEY1".to_vec();
    out.extend(salt);
    out.extend(nonce);
    out.extend(ciphertext);
    Ok(out)
}
pub fn unseal(blob: &[u8], password: &str, purpose: &str) -> Result<Zeroizing<Vec<u8>>> {
    ensure!(
        blob.len() >= 62 && blob.starts_with(b"CHKEY1"),
        "invalid keystore"
    );
    let mut key = Zeroizing::new([0u8; 32]);
    argon()
        .hash_password_into(password.as_bytes(), &blob[6..22], key.as_mut())
        .map_err(|_| anyhow::anyhow!("key derivation failed"))?;
    let plain = XChaCha20Poly1305::new_from_slice(key.as_ref())
        .map_err(|_| anyhow::anyhow!("invalid key"))?
        .decrypt(
            XNonce::from_slice(&blob[22..46]),
            Payload {
                msg: &blob[46..],
                aad: purpose.as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("keystore unlock failed"))?;
    Ok(Zeroizing::new(plain))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn immutable_and_signing_guard() {
        let db = Storage::open(":memory:").unwrap();
        db.put("epoch", "one", None, &1).unwrap();
        assert!(db.put("epoch", "one", None, &2).is_err());
        db.reserve_signature("key", "scope", "a").unwrap();
        db.reserve_signature("key", "scope", "a").unwrap();
        assert!(db.reserve_signature("key", "scope", "b").is_err());
    }
    #[test]
    fn signing_reservations_and_history_survive_restart() {
        let dir = std::env::temp_dir().join(format!("chorus-storage-test-{}", token()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("test.sqlite");
        {
            let db = Storage::open(&path).unwrap();
            db.reserve_signature("member", "federation:sequence:1", "commitment-a")
                .unwrap();
            db.put("epoch", "epoch-zero", Some("federation"), &42)
                .unwrap();
        }
        {
            let db = Storage::open(&path).unwrap();
            assert_eq!(db.get::<u32>("epoch", "epoch-zero").unwrap(), Some(42));
            assert!(
                db.put("epoch", "epoch-zero", Some("federation"), &43)
                    .is_err()
            );
            db.reserve_signature("member", "federation:sequence:1", "commitment-a")
                .unwrap();
            assert!(
                db.reserve_signature("member", "federation:sequence:1", "commitment-b")
                    .is_err()
            );
            assert!(
                db.connection()
                    .unwrap()
                    .execute("UPDATE records SET value='43' WHERE kind='epoch'", [])
                    .is_err()
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn encrypted_keys_are_bound_to_purpose() {
        let b = seal(b"secret", "correct horse battery", "federation").unwrap();
        assert_eq!(
            &*unseal(&b, "correct horse battery", "federation").unwrap(),
            b"secret"
        );
        assert!(unseal(&b, "wrong password", "federation").is_err());
        assert!(unseal(&b, "correct horse battery", "communication").is_err());
    }
}
