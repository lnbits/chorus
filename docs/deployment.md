# Server and desktop deployment

This is a pre-production build. Read [security-review.md](security-review.md) before exposing a node or relying on its attestations for valuable actions.

## VPS without Docker

1. Build `chorus-node` and `fedwasm` with `cargo build --locked --release -p chorus-node -p chorus-verifier`.
2. Run `npm ci --prefix apps/web && npm run build --prefix apps/web`.
3. Install binaries in `/usr/local/bin`, web assets in `/opt/chorus/web`, and create an unprivileged `chorus` service account.
4. As that account, initialize a mode-0700 data directory with `chorus-node --data-dir /var/lib/chorus init`.
5. Store the keystore unlock password as an encrypted systemd credential (for example using `systemd-creds encrypt`) at `/etc/credstore.encrypted/chorus-keystore-password`. The password is not the admin password.
6. Install the examples in `deploy/chorus.toml`, `deploy/chorus.service` and `deploy/Caddyfile`, adjusting the real HTTPS hostname.
7. Start the service, sign in over HTTPS, and review the six default read/write relays in Settings.

The node defaults to loopback. A non-loopback bind requires an HTTPS origin configuration, but the node itself does **not** terminate TLS. Only expose it behind a trusted reverse proxy. Exact Origin validation includes scheme, hostname and port. Do not include a path or trailing slash. HTTPS origins make the session cookie Secure. Requests use HttpOnly, SameSite=Strict cookies and a per-session CSRF header. No CORS wildcard is enabled. The browser and API must share an origin.

The static UI is served by the Rust process, including SPA fallback. The frontend build externalizes its bootstrap to satisfy `script-src 'self'` without allowing arbitrary inline scripts. Do not replace that CSP with `unsafe-inline` for scripts.

### Configuration

| Variable / CLI option | Purpose |
| --- | --- |
| `CHORUS_DATA_DIR` / `--data-dir` | Database and encrypted key-vault directory |
| `CHORUS_CONFIG` / `--config` | Optional TOML configuration file |
| `CHORUS_BIND` / `serve --bind` | Socket address, default `127.0.0.1:8787` |
| `CHORUS_ORIGIN` / `serve --origin` | Exact browser origin |
| `CHORUS_WEB_ROOT` / `serve --web-root` | Built SvelteKit asset directory |
| `CHORUS_KEY_PASSWORD_FILE` / `serve --key-password-file` | Protected credential file path |
| TOML `session_ttl` | Session lifetime, 300–86400 seconds |
| `RUST_LOG` | Structured log filter; never add private payload logging |

CLI/environment values override TOML. Key material is not accepted through the HTTP API. The desktop launcher uses `serve --key-password-stdin` and closes the pipe after sending the credential. Automation can use `init --credentials-stdin` with JSON containing `admin_password` and `key_password` from a protected pipe. Do not put secret literals in shell history, command arguments or a committed config.

## Docker

`Dockerfile` builds the UI and Rust binaries, then runs as uid 10001 with only certificate authorities in the runtime image. `compose.yaml` binds its port to host loopback, drops capabilities and mounts a credential secret.

Before starting the daemon:

```sh
docker compose build
docker compose run --rm chorus init --username admin
# Create keystore-password.txt securely, set restrictive permissions,
# and use the exact unlock password selected at initialization.
docker compose up -d
```

Set the real HTTPS origin and proxy it through Caddy. Keep `keystore-password.txt` out of version control. These image/service examples are included but container deployment has not been exercised in this workspace.

## Nix

The pinned flake provides `packages.<system>.node`, `.web`, a development shell, and a NixOS module. It supports x86_64/aarch64 Linux and Darwin package definitions. Native deployment support must be checked on the intended platform.

```sh
nix develop
nix build .#node
nix build .#web
```

Import `inputs.chorus.nixosModules.default` and set `services.chorus.enable`, `.origin` and `.credentialFile`. The credential is a runtime path to an encrypted systemd credential, never a secret embedded in the Nix store. Initialize the data directory as the service user before startup. The Nix shell was exercised on Linux; a full Nix package build has not been certified.

## Tauri desktop

Install the Tauri CLI, native WebKit/GTK prerequisites on Linux, Xcode tools on macOS, or the Windows C++ toolchain and WebView2. `nix develop` supplies the Linux native build environment.

```sh
cargo install tauri-cli --locked
npm ci --prefix apps/web
./scripts/build-desktop.sh
```

The script builds the standalone node, names the sidecar for the current target triple, builds the web app and bundles it with Tauri. The local bootstrap screen creates admin credentials and independent communication/member identities, encrypts the vaults and starts the sidecar through a pipe. The UI then navigates to the node's ephemeral loopback port, using ordinary same-origin session authentication. Remote pages receive no native IPC capability. No shell/file-access plugin is exposed to those pages.

The Linux Rust desktop target has been compile-checked using GTK/WebKit/DBus from Nix. Installed application behavior, packaging, code signing/notarization and Windows/macOS execution still require platform testing. A manual GitHub workflow contains the three-platform build matrix. It does not publish installers or run automatically on untrusted pull requests.

The current desktop keystore is encrypted on disk. OS-keychain-backed unlock storage and hardware-backed federation signers are future release work; do not describe them as implemented.
