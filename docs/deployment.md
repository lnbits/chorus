# Server and desktop deployment

This is a pre-production build. Read [security-review.md](security-review.md) before exposing a node or relying on its attestations for valuable actions.

## Linux release packages

The `Linux release` GitHub Actions workflow runs when a `v*` tag is pushed. It builds on Ubuntu 22.04 x86_64, following [Tauri's AppImage baseline guidance](https://v2.tauri.app/distribute/appimage/). A successful run creates a GitHub release containing:

- `Chorus_<version>_amd64.AppImage`: desktop UI and local node.
- `chorus-<version>-linux-x86_64.tar.gz`: `chorus-node`, `fedwasm`, built web assets, an example contract, documentation and service/proxy examples.
- `SHA256SUMS`: checksums for both packages.

Download from [GitHub Releases](https://github.com/lnbits/chorus/releases). These are Linux x86_64 builds; ARM64, Windows and macOS are not published by this workflow. The AppImage needs a graphical desktop. Use the server archive on a VPS. Users do not need Rust, Cargo, Node.js or npm.

With both packages and `SHA256SUMS` in the same directory:

```sh
sha256sum --check SHA256SUMS
chmod +x Chorus_0.1.0_amd64.AppImage
./Chorus_0.1.0_amd64.AppImage
```

Replace `0.1.0` with the downloaded version. If FUSE is unavailable, run `./Chorus_0.1.0_amd64.AppImage --appimage-extract-and-run`. NixOS users may need `appimage-run`. AppImage compatibility still depends on the host's graphics stack and system libraries; the Ubuntu build baseline is not a guarantee for every distribution.

For the server archive:

```sh
tar -xzf chorus-0.1.0-linux-x86_64.tar.gz
cd chorus-0.1.0-linux-x86_64
./bin/chorus-node --data-dir "$HOME/.local/share/chorus-server" init --username admin
./bin/chorus-node --data-dir "$HOME/.local/share/chorus-server" serve --web-root "$PWD/web"
```

Keep the data directory outside the extracted release directory. On an existing installation, stop the old process, back up the data and start the new binary with the same data directory; do not initialize it again. For an always-running VPS service, use the service account, credential and HTTPS setup below, starting at installation step 3. Distribution CA certificates must be installed for secure relay connections.

### Publishing a release

Commit the workflow and application changes before tagging. Tags must use `vMAJOR.MINOR.PATCH`, optionally with a prerelease suffix such as `-rc.1`. The workflow rejects a tag that differs from the versions in `Cargo.toml`, `apps/desktop/Cargo.toml`, `apps/desktop/tauri.conf.json`, `apps/web/package.json` and their lockfiles. For a version bump, update those manifests, refresh both Cargo lockfiles with `cargo check --workspace` and `cargo check --manifest-path apps/desktop/Cargo.toml` (with native build dependencies installed), and refresh the npm lockfile with `npm install --package-lock-only --prefix apps/web`.

For the currently configured version, after committing:

```sh
python3 scripts/check-release-version.py v0.1.0
git push origin main
git tag -a v0.1.0 -m "Chorus v0.1.0"
git push origin v0.1.0
```

The version check requires Python 3.11+. The workflow runs Rust tests and frontend checks, builds both packages, initializes and starts a disposable node from the server archive, verifies its UI assets, and extracts the AppImage to check the sidecar and resource layout. Only then can its separate publication job write a release. Prerelease tags create GitHub prereleases. It uses the repository's automatic `GITHUB_TOKEN`; no personal token or signing secret is required. The packages currently have checksums, not publisher signatures or an automatic update feed.

You can also run `Linux release` manually from Actions to build downloadable workflow artifacts without publishing a release. Use a new tag for each published version; the workflow deliberately fails if a GitHub release already exists for that tag instead of replacing published assets. The workflow file must be present in the tagged commit. Automated tests do not replace desktop testing on the intended distribution.

To build the same packages locally on Ubuntu 22.04 x86_64, install the desktop prerequisites below and Python 3.11+, run `npm ci --prefix apps/web`, then `bash scripts/package-linux.sh v0.1.0`. Outputs go to `dist/`.

## VPS without Docker

On a fresh Ubuntu VPS, install the build tools as your normal login user. Chorus requires Rust 1.98+ and Node.js 24; npm comes with Node.js. The headless node does not need the Tauri desktop's GTK/WebKit packages.

```sh
sudo apt update
sudo apt install -y build-essential curl ca-certificates pkg-config libssl-dev git xz-utils

# Rust and Cargo, installed for your user.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.98.0
. "$HOME/.cargo/env"

# Node.js 24 and npm, installed for your user.
export NVM_DIR="$HOME/.nvm"
curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.8/install.sh | bash
. "$NVM_DIR/nvm.sh"
nvm install 24
nvm alias default 24
nvm use 24

rustc --version
cargo --version
node --version
npm --version
```

Installer references: [Rust](https://rust-lang.org/tools/install/), [nvm](https://github.com/nvm-sh/nvm#installing-and-updating). Run Cargo and npm as your normal user; only the apt commands need sudo. If the repository lives at `~/chorus`, run `cd ~/chorus` before the following build commands.

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

Install native WebKit/GTK prerequisites on Linux, Xcode tools on macOS, or the Windows C++ toolchain and WebView2. `nix develop` supplies the Linux native build environment.

```sh
npm ci --prefix apps/web
./scripts/build-desktop.sh
```

The script uses the pinned Tauri CLI through npm, builds the standalone node, names the sidecar for the current target triple, builds the web app and bundles it with Tauri. The local bootstrap screen creates admin credentials and one generated or imported Nostr identity, encrypts its vault and starts the sidecar through a pipe. The UI then navigates to the node's ephemeral loopback port, using ordinary same-origin session authentication. Remote pages receive no native IPC capability. No shell/file-access plugin is exposed to those pages.

The Linux release workflow publishes the AppImage and server archive described above. A separate manual `Desktop build` workflow retains the three-platform build matrix and stores build artifacts without publishing installers. Neither packaging workflow runs automatically on pull requests. Interactive desktop behavior, code signing/notarization and Windows/macOS execution still require platform testing.

The current desktop keystore is encrypted on disk. OS-keychain-backed unlock storage and hardware-backed federation signers are future release work; do not describe them as implemented.
