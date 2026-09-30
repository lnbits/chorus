use anyhow::{Context, Result, ensure};
use chorus_api::{Api, engine::Engine};
use chorus_nostr::{RelayConfig, Transport, default_relays};
use chorus_storage::{
    Storage, initialize_identity, load_identity, load_legacy_inbox, lock_node, replace_identity,
};
use clap::{Parser, Subcommand};
use nostr_sdk::prelude::*;
use serde::Deserialize;
use std::{net::SocketAddr, path::PathBuf};
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
    /// Create an admin account and one Nostr identity. Passwords are prompted without echo.
    Init {
        #[arg(long, default_value = "admin")]
        username: String,
        /// Read {"admin_password":...,"key_password":...} from a protected pipe.
        #[arg(long)]
        credentials_stdin: bool,
        /// Import an existing nsec using a non-echoed terminal prompt.
        #[arg(long, conflicts_with = "credentials_stdin")]
        import_nsec: bool,
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
    /// Replace the local identity after stopping the node; retains an encrypted backup.
    ReplaceKey {
        /// Read {"key_password":...,"nsec":...} from a protected pipe.
        #[arg(long)]
        credentials_stdin: bool,
        /// Proceed even if federations still list the old identity as a member.
        #[arg(long)]
        allow_membership_loss: bool,
    },
    /// Print the public Nostr identity; private material is never returned by this command.
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
    let _node_lock = if matches!(cli.command, Command::Identity) {
        None
    } else {
        Some(lock_node(&cli.data_dir)?)
    };
    let db = Storage::open(cli.data_dir.join("chorus.sqlite"))?;
    match cli.command {
        Command::Init {
            username,
            credentials_stdin,
            import_nsec,
        } => {
            ensure!(db.admin().is_err(), "node already initialized");
            ensure!(
                !username.is_empty() && username.len() <= 64,
                "invalid username"
            );
            let (password, key_password, nsec) = if credentials_stdin {
                use std::io::Read;
                #[derive(Deserialize)]
                struct Credentials {
                    admin_password: String,
                    key_password: String,
                    #[serde(default)]
                    nsec: Option<String>,
                }
                let mut input = Zeroizing::new(String::new());
                std::io::stdin().take(4097).read_to_string(&mut input)?;
                ensure!(input.len() <= 4096, "credential input exceeds limit");
                let c: Credentials = serde_json::from_str(&input)?;
                (
                    Zeroizing::new(c.admin_password),
                    Zeroizing::new(c.key_password),
                    c.nsec.map(Zeroizing::new),
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
                    if import_nsec {
                        Some(Zeroizing::new(rpassword::prompt_password("Nostr nsec: ")?))
                    } else {
                        None
                    },
                )
            };
            chorus_storage::password_hash(&password)?;
            ensure!(key_password.len() >= 12, "keystore password too short");
            let identity = initialize_identity(
                &cli.data_dir,
                &key_password,
                nsec.as_ref().map(|s| s.as_str()),
            )?;
            db.set_setting("member_public_key", &identity.public_key().to_hex())?;
            db.init_admin(&username, &password)?;
            db.set_setting("relays", &default_relays())?;
            println!(
                "Node initialized with six read/write relays. Manage them in Settings after login."
            );
        }
        Command::ReplaceKey {
            credentials_stdin,
            allow_membership_loss,
        } => {
            db.admin()
                .context("initialize this node before replacing its key")?;
            let (password, nsec) = if credentials_stdin {
                use std::io::Read;
                #[derive(Deserialize)]
                struct Credentials {
                    key_password: String,
                    nsec: String,
                }
                let mut input = Zeroizing::new(String::new());
                std::io::stdin().take(4097).read_to_string(&mut input)?;
                ensure!(input.len() <= 4096, "credential input exceeds limit");
                let c: Credentials = serde_json::from_str(&input)?;
                (Zeroizing::new(c.key_password), Zeroizing::new(c.nsec))
            } else {
                (
                    Zeroizing::new(rpassword::prompt_password("Keystore password: ")?),
                    Zeroizing::new(rpassword::prompt_password("Replacement Nostr nsec: ")?),
                )
            };
            let old = load_identity(&cli.data_dir, &password)?;
            let replacement = SecretKey::from_bech32(nsec.trim())
                .map_err(|_| anyhow::anyhow!("invalid Nostr nsec"))?;
            let new_public = Keys::new(replacement).public_key();
            ensure!(
                new_public != old.public_key(),
                "this nsec is already the active identity"
            );
            let mut latest = std::collections::BTreeMap::new();
            for record in db.list::<chorus_api::engine::EpochRecord>("epoch", None)? {
                let entry = latest
                    .entry(record.state.federation_id.clone())
                    .or_insert(record.state.clone());
                if record.state.epoch > entry.epoch {
                    *entry = record.state;
                }
            }
            let affected = latest
                .values()
                .filter(|s| s.members.contains(&old.public_key().to_hex()))
                .count();
            ensure!(
                affected == 0 || allow_membership_loss,
                "the current identity is still a member of {affected} federation(s). Arrange admission of the new npub and removal of the old npub through governance, or explicitly use --allow-membership-loss to switch anyway. Proposed npub: {}",
                new_public.to_bech32()?
            );
            let (identity, backup) = replace_identity(&cli.data_dir, &password, &nsec)?;
            db.set_setting("member_public_key", &identity.public_key().to_hex())?;
            db.activity("identity_changed", None, "settings", "Local Nostr identity replaced; encrypted backup retained and federation history preserved")?;
            println!("Identity: {}", identity.public_key().to_bech32()?);
            println!("Encrypted previous-key backup: {}", backup.display());
            println!("Federation membership and signing history were not changed.");
        }
        Command::Identity => {
            let key: String = db
                .setting("member_public_key")?
                .context("node is not initialized")?;
            println!("identity: {}", PublicKey::from_hex(&key)?.to_bech32()?);
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
            let identity = load_identity(&cli.data_dir, &password)?;
            // Recover public metadata if a previous key replacement was interrupted after rename.
            db.set_setting("member_public_key", &identity.public_key().to_hex())?;
            let legacy_inbox = load_legacy_inbox(&cli.data_dir, &password)?;
            drop(password);
            let relays = db
                .setting::<Vec<RelayConfig>>("relays")?
                .unwrap_or_else(default_relays);
            let transport =
                Transport::with_legacy_inbox(identity.clone(), legacy_inbox, &relays).await?;
            let engine = Engine::new(db.clone(), chorus_fedvm::FedVm::new()?, identity, transport)?;
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
