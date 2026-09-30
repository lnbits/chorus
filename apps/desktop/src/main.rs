#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use anyhow::{Context, Result, ensure};
use chorus_storage::{Storage, seal, unseal};
use nostr_sdk::prelude::*;
use std::{
    io::Write,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Mutex,
};
use tauri::Manager;
use zeroize::Zeroizing;
#[derive(Default)]
struct Runtime {
    child: Mutex<Option<Child>>,
    starting: std::sync::atomic::AtomicBool,
}
struct StartGuard<'a>(&'a std::sync::atomic::AtomicBool);
impl Drop for StartGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}
// A failed launch, health check or navigation must never orphan a signing node.
struct PendingChild(Option<Child>);
impl Drop for PendingChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn directory(app: &tauri::AppHandle) -> Result<PathBuf> {
    let p = app.path().app_data_dir()?;
    std::fs::create_dir_all(&p)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(p)
}
#[tauri::command]
fn initialized(app: tauri::AppHandle) -> std::result::Result<bool, String> {
    fn run(app: &tauri::AppHandle) -> Result<bool> {
        let p = directory(app)?;
        Ok(Storage::open(p.join("chorus.sqlite"))?.admin().is_ok())
    }
    run(&app).map_err(|e| e.to_string())
}
#[tauri::command]
async fn initialize(
    app: tauri::AppHandle,
    username: String,
    admin_password: String,
    key_password: String,
) -> std::result::Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<()> {
        let admin = Zeroizing::new(admin_password);
        let password = Zeroizing::new(key_password);
        let p = directory(&app)?;
        ensure!(
            !username.is_empty() && username.len() <= 64,
            "invalid username"
        );
        let db = Storage::open(p.join("chorus.sqlite"))?;
        ensure!(db.admin().is_err(), "node already initialized");
        chorus_storage::password_hash(&admin)?;
        ensure!(
            password.len() >= 12,
            "keystore password requires at least 12 characters"
        );
        for purpose in ["communication", "member"] {
            let keys = Keys::generate();
            let secret = Zeroizing::new(keys.secret_key().to_secret_bytes());
            let data = seal(secret.as_ref(), &password, purpose)?;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut f = options.open(p.join(format!("{purpose}.vault")))?;
            f.write_all(&data)?;
            f.sync_all()?;
            db.set_setting(
                &format!("{purpose}_public_key"),
                &keys.public_key().to_hex(),
            )?;
        }
        db.init_admin(&username, &admin)?;
        db.set_setting("relays", &chorus_nostr::default_relays())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
#[tauri::command]
async fn start_local(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    key_password: String,
) -> std::result::Result<(), String> {
    async fn run(
        app: tauri::AppHandle,
        window: tauri::WebviewWindow,
        key_password: String,
    ) -> Result<()> {
        let runtime = app.state::<Runtime>();
        ensure!(
            runtime
                .starting
                .compare_exchange(
                    false,
                    true,
                    std::sync::atomic::Ordering::AcqRel,
                    std::sync::atomic::Ordering::Acquire
                )
                .is_ok(),
            "node startup already in progress"
        );
        let _startup = StartGuard(&runtime.starting);
        {
            let mut handle = runtime
                .child
                .lock()
                .map_err(|_| anyhow::anyhow!("runtime unavailable"))?;
            if let Some(child) = handle.as_mut() {
                ensure!(child.try_wait()?.is_some(), "node already running");
                *handle = None;
            }
        }
        let password = Zeroizing::new(key_password);
        let p = directory(&app)?;
        // Check credentials before process launch; errors never contain secret material.
        unseal(&std::fs::read(p.join("member.vault"))?, &password, "member")?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        let exe = std::env::current_exe()?
            .parent()
            .context("executable directory unavailable")?
            .join(if cfg!(windows) {
                "chorus-node.exe"
            } else {
                "chorus-node"
            });
        let url = format!("http://127.0.0.1:{port}");
        let child = Command::new(exe)
            .arg("--data-dir")
            .arg(&p)
            .arg("serve")
            .arg("--bind")
            .arg(format!("127.0.0.1:{port}"))
            .arg("--origin")
            .arg(&url)
            .arg("--web-root")
            .arg(app.path().resource_dir()?.join("web"))
            .arg("--key-password-stdin")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let mut pending = PendingChild(Some(child));
        pending
            .0
            .as_mut()
            .context("missing child")?
            .stdin
            .take()
            .context("cannot open credential pipe")?
            .write_all(password.as_bytes())?;
        drop(password);
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(1))
            .build()?;
        for _ in 0..100 {
            if http
                .get(format!("{url}/healthz"))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                window.navigate(url.parse()?)?;
                *runtime
                    .child
                    .lock()
                    .map_err(|_| anyhow::anyhow!("runtime unavailable"))? = pending.0.take();
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        anyhow::bail!(
            "Local node failed to start. Check the packaged node binary and data directory."
        )
    }
    run(app, window, key_password)
        .await
        .map_err(|e| e.to_string())
}
fn main() {
    tauri::Builder::default()
        .manage(Runtime::default())
        .invoke_handler(tauri::generate_handler![
            initialized,
            initialize,
            start_local
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Chorus desktop")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit)
                && let Ok(mut child) = app.state::<Runtime>().child.lock()
                && let Some(mut process) = child.take()
            {
                let _ = process.kill();
                let _ = process.wait();
            }
        });
}
