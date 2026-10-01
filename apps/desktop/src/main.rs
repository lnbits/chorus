#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod launch;
use anyhow::{Context, Result, ensure};
use chorus_storage::{Storage, initialize_identity, load_identity};
use clap::Parser;
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
    let p = match &app.state::<launch::Options>().data_dir {
        Some(path) => path.clone(),
        None => app.path().app_data_dir()?,
    };
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
    nsec: Option<String>,
) -> std::result::Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<()> {
        let nsec = nsec.map(Zeroizing::new);
        let admin = Zeroizing::new(admin_password);
        let password = Zeroizing::new(key_password);
        let p = directory(&app)?;
        ensure!(
            !username.is_empty() && username.len() <= 64,
            "invalid username"
        );
        let _lock = chorus_storage::lock_node(&p)?;
        let db = Storage::open(p.join("chorus.sqlite"))?;
        ensure!(db.admin().is_err(), "node already initialized");
        chorus_storage::password_hash(&admin)?;
        ensure!(
            password.len() >= 12,
            "keystore password requires at least 12 characters"
        );
        let identity = initialize_identity(&p, &password, nsec.as_ref().map(|s| s.as_str()))?;
        db.set_setting("member_public_key", &identity.public_key().to_hex())?;
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
        load_identity(&p, &password)?;
        let endpoint = app.state::<launch::Options>().endpoint()?;
        let child = Command::new(sidecar()?)
            .arg("--data-dir")
            .arg(&p)
            .arg("serve")
            .arg("--bind")
            .arg(endpoint.bind.to_string())
            .arg("--origin")
            .arg(&endpoint.origin)
            .arg("--web-root")
            .arg(app.path().resource_dir()?.join("web"))
            .arg("--key-password-stdin")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
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
            if let Some(status) = pending.0.as_mut().context("missing child")?.try_wait()? {
                anyhow::bail!(
                    "Local node exited during startup ({status}). Check the terminal for details."
                );
            }
            if http
                .get(format!("{}/healthz", endpoint.health_url))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                window.navigate(endpoint.origin.parse()?)?;
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
fn sidecar() -> Result<PathBuf> {
    Ok(std::env::current_exe()?
        .parent()
        .context("executable directory unavailable")?
        .join(if cfg!(windows) {
            "chorus-node.exe"
        } else {
            "chorus-node"
        }))
}

fn run_headless(options: &launch::Options, resources: PathBuf, identifier: &str) -> Result<()> {
    let data = match &options.data_dir {
        Some(path) => path.clone(),
        None => dirs::data_dir()
            .context("application data directory unavailable")?
            .join(identifier),
    };
    let mut command = Command::new(sidecar()?);
    command.arg("--data-dir").arg(data);
    if options.init {
        command
            .arg("init")
            .arg("--username")
            .arg(options.username.as_deref().unwrap_or("admin"));
        if options.import_nsec {
            command.arg("--import-nsec");
        }
        if options.credentials_stdin {
            command.arg("--credentials-stdin");
        }
    } else {
        let endpoint = options.endpoint()?;
        let web = resources.join("web");
        ensure!(
            web.join("200.html").is_file(),
            "packaged web assets missing at {}",
            web.display()
        );
        command
            .arg("serve")
            .arg("--bind")
            .arg(endpoint.bind.to_string())
            .arg("--origin")
            .arg(endpoint.origin)
            .arg("--web-root")
            .arg(web);
        if let Some(file) = &options.key_password_file {
            command.arg("--key-password-file").arg(file);
        }
        if options.key_password_stdin {
            command.arg("--key-password-stdin");
        }
    }
    // On Unix the node replaces the launcher, preserving terminal signals and exit status.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec()).context("cannot launch bundled node")
    }
    #[cfg(not(unix))]
    {
        let status = command.status().context("cannot launch bundled node")?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn main() {
    let options = launch::Options::parse();
    if let Err(error) = options.validate() {
        eprintln!("{error:#}");
        std::process::exit(2);
    }
    let context = tauri::generate_context!();
    if options.headless {
        let result =
            tauri::utils::platform::resource_dir(context.package_info(), &tauri::Env::default())
                .map_err(anyhow::Error::from)
                .and_then(|resources| {
                    run_headless(&options, resources, &context.config().identifier)
                });
        if let Err(error) = result {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return;
    }
    tauri::Builder::default()
        .manage(options)
        .manage(Runtime::default())
        .invoke_handler(tauri::generate_handler![
            initialized,
            initialize,
            start_local
        ])
        .build(context)
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
