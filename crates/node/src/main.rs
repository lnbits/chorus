use anyhow::{Context, Result, ensure};
use chorus_api::{Api, engine::Engine};
use chorus_nostr::{RelayConfig, Transport, default_relays};
use chorus_storage::{Storage, seal, unseal};
use clap::{Parser, Subcommand};
use nostr_sdk::prelude::*;
use serde::Deserialize;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

#[derive(Parser)]
#[command(
    name = "chorus-node",
    version,
    about = "Federated deterministic WASM attestations"
)]
struct Cli {
    #[arg(long, env = "CHORUS_DATA_DIR", default_value = "data")]
    data_dir: PathBuf,
    #[arg(long, env = "CHORUS_CONFIG")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Create independent admin, communication and member identities. Passwords are prompted without echo.
    Init {
        #[arg(long, default_value = "admin")]
        username: String,
        /// Read {"admin_password":...,"key_password":...} from a protected pipe.
        #[arg(long)]
        credentials_stdin: bool,
    },
    /// Start the headless node and optionally serve the built SvelteKit UI.
    Serve {
        #[arg(long, env = "CHORUS_BIND")]
        bind: Option<SocketAddr>,
        #[arg(long, env = "CHORUS_ORIGIN")]
        origin: Option<String>,
        #[arg(long, env = "CHORUS_WEB_ROOT")]
        web_root: Option<String>,
        #[arg(long, env = "CHORUS_KEY_PASSWORD_FILE")]
        key_password_file: Option<PathBuf>,
        #[arg(long, hide = true)]
        key_password_stdin: bool,
    },
    /// Print public identities; private material is never returned by this command.
    Identity,
}
#[derive(Default, Deserialize)]
struct Config {
    bind: Option<SocketAddr>,
    origin: Option<String>,
    web_root: Option<String>,
    key_password_file: Option<PathBuf>,
    session_ttl: Option<u64>,
}
fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
fn load_keys(dir: &Path, password: &str, purpose: &str) -> Result<Keys> {
    let blob = std::fs::read(dir.join(format!("{purpose}.vault")))?;
    let bytes = unseal(&blob, password, purpose)?;
    Ok(Keys::new(SecretKey::from_slice(&bytes)?))
}
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "chorus_node=info,chorus_api=info".into()),
        )
        .init();
    let cli = Cli::parse();
    let cfg: Config = cli
        .config
        .as_ref()
        .map(std::fs::read_to_string)
        .transpose()?
        .map(|v| toml::from_str(&v))
        .transpose()?
        .unwrap_or_default();
    std::fs::create_dir_all(&cli.data_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cli.data_dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let db = Storage::open(cli.data_dir.join("chorus.sqlite"))?;
    match cli.command {
        Command::Init {
            username,
            credentials_stdin,
        } => {
            ensure!(db.admin().is_err(), "node already initialized");
            ensure!(
                !username.is_empty() && username.len() <= 64,
                "invalid username"
            );
            let (password, key_password) = if credentials_stdin {
                use std::io::Read;
                #[derive(Deserialize)]
                struct Credentials {
                    admin_password: String,
                    key_password: String,
                }
                let mut input = Zeroizing::new(String::new());
                std::io::stdin().take(4097).read_to_string(&mut input)?;
                ensure!(input.len() <= 4096, "credential input exceeds limit");
                let c: Credentials = serde_json::from_str(&input)?;
                (
                    Zeroizing::new(c.admin_password),
                    Zeroizing::new(c.key_password),
                )
            } else {
                let password = Zeroizing::new(rpassword::prompt_password(
                    "Admin password (12+ characters): ",
                )?);
                let confirm =
                    Zeroizing::new(rpassword::prompt_password("Confirm admin password: ")?);
                ensure!(*password == *confirm, "passwords do not match");
                (
                    password,
                    Zeroizing::new(rpassword::prompt_password(
                        "Keystore password (12+ characters): ",
                    )?),
                )
            };
            chorus_storage::password_hash(&password)?;
            ensure!(key_password.len() >= 12, "keystore password too short");
            for purpose in ["communication", "member"] {
                let keys = Keys::generate();
                let secret = Zeroizing::new(keys.secret_key().to_secret_bytes());
                private_write(
                    &cli.data_dir.join(format!("{purpose}.vault")),
                    &seal(secret.as_ref(), &key_password, purpose)?,
                )?;
                db.set_setting(
                    &format!("{purpose}_public_key"),
                    &keys.public_key().to_hex(),
                )?;
            }
            db.init_admin(&username, &password)?;
            db.set_setting("relays", &default_relays())?;
            println!(
                "Node initialized with six read/write relays. Manage them in Settings after login."
            );
        }
        Command::Identity => {
            for purpose in ["communication", "member"] {
                let key: String = db
                    .setting(&format!("{purpose}_public_key"))?
                    .context("node is not initialized")?;
                println!("{purpose}: {}", PublicKey::from_hex(&key)?.to_bech32()?);
            }
        }
        Command::Serve {
            bind,
            origin,
            web_root,
            key_password_file,
            key_password_stdin,
        } => {
            db.admin()
                .context("initialize the node with `chorus-node init` first")?;
            let bind = bind
                .or(cfg.bind)
                .unwrap_or_else(|| "127.0.0.1:8787".parse().unwrap());
            let origin = origin
                .or(cfg.origin)
                .unwrap_or_else(|| format!("http://{bind}"));
            ensure!(
                origin.starts_with("https://") || bind.ip().is_loopback(),
                "remote listening requires an HTTPS origin"
            );
            let password = if key_password_stdin {
                use std::io::Read;
                let mut value = Zeroizing::new(String::new());
                std::io::stdin().take(1025).read_to_string(&mut value)?;
                ensure!(value.len() <= 1024, "keystore credential exceeds limit");
                value
            } else if let Some(file) = key_password_file.or(cfg.key_password_file) {
                Zeroizing::new(
                    std::fs::read_to_string(file)?
                        .trim_end_matches(['\n', '\r'])
                        .to_owned(),
                )
            } else {
                Zeroizing::new(rpassword::prompt_password("Keystore password: ")?)
            };
            let communication = load_keys(&cli.data_dir, &password, "communication")?;
            let member = load_keys(&cli.data_dir, &password, "member")?;
            drop(password);
            let relays = db
                .setting::<Vec<RelayConfig>>("relays")?
                .unwrap_or_else(default_relays);
            let transport = Transport::new(communication.clone(), &relays).await?;
            let engine = Engine::new(
                db.clone(),
                chorus_fedvm::FedVm::new()?,
                member,
                communication.public_key(),
                transport,
            );
            // Validate complete authenticated history on restart, not just a mutable head pointer.
            for state in engine.states()? {
                let invitation = engine.invitation(&state.federation_id)?;
                engine.join(&invitation, &invitation.genesis_hash)?;
            }
            engine.refresh_subscriptions().await?;
            let ingest = engine.clone();
            let mut notifications = engine.transport.client.notifications();
            tokio::spawn(async move {
                use futures_util::StreamExt;
                while let Some(notification) = notifications.next().await {
                    if let ClientNotification::Event { event, .. } = notification
                        && ingest.ingest(*event).await.is_err()
                    {
                        tracing::warn!(boundary = "nostr", "Rejected or deferred protocol event");
                    }
                }
            });
            let pump = engine.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
                loop {
                    interval.tick().await;
                    if let Ok(events) = pump.db.pending_events() {
                        for (event, recipients) in events {
                            if pump.transport.publish(&event, &recipients).await.is_ok() {
                                let _ = pump.db.mark_sent(&event.id.to_hex());
                            }
                        }
                    }
                }
            });
            let sync = engine.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    if let Ok(events) = sync.transport.private_backfill().await {
                        // Gift-wrap timestamps are intentionally randomized; retry dependency order.
                        for _ in 0..3 {
                            for event in &events {
                                let _ = sync.ingest(event.clone()).await;
                            }
                        }
                    }
                    if let Ok(states) = sync.states() {
                        for state in states {
                            if !state.public {
                                continue;
                            }
                            if let Ok(events) = sync.transport.backfill(&state.federation_id).await
                            {
                                for event in events {
                                    let _ = sync.ingest(event).await;
                                }
                            }
                        }
                    }
                }
            });
            let web_root = web_root.or(cfg.web_root);
            let ttl = cfg.session_ttl.unwrap_or(28800);
            ensure!(
                (300..=86400).contains(&ttl),
                "session TTL must be 300–86400 seconds"
            );
            let app = chorus_api::router(
                Api {
                    engine,
                    secure_cookies: origin.starts_with("https://"),
                    origin,
                    session_ttl: ttl,
                },
                web_root.as_deref(),
            );
            let listener = tokio::net::TcpListener::bind(bind).await?;
            tracing::info!(address=%bind,"Chorus node listening");
            axum_serve(listener, app).await?;
        }
    }
    Ok(())
}
async fn axum_serve(listener: tokio::net::TcpListener, app: axum::Router) -> Result<()> {
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
