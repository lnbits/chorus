#!/usr/bin/env bash
# Build on Ubuntu 22.04 x86_64 for the release's glibc baseline.
set -euo pipefail
cd "$(dirname "$0")/.."
version="$(python3 scripts/check-release-version.py "${1:?usage: package-linux.sh vX.Y.Z}")"
[[ "$(rustc -vV | sed -n 's/^host: //p')" == x86_64-unknown-linux-gnu ]] || {
  echo 'Linux release packaging currently requires x86_64-unknown-linux-gnu.' >&2
  exit 1
}
cargo build --locked --release -p chorus-node -p chorus-verifier
bash scripts/build-desktop.sh --bundles appimage

mkdir -p dist
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
name="chorus-${version}-linux-x86_64"
mkdir -p "$staging/$name/bin" "$staging/$name/web" "$staging/$name/deploy" "$staging/$name/examples"
install -m 755 target/release/chorus-node target/release/fedwasm "$staging/$name/bin/"
cp -R apps/web/build/. "$staging/$name/web/"
cp deploy/chorus.service deploy/chorus.toml deploy/Caddyfile "$staging/$name/deploy/"
cp examples/echo.wasm examples/echo.wat "$staging/$name/examples/"
cp -R docs "$staging/$name/docs"
cp deploy/release-README.txt "$staging/$name/README.txt"
# An explicit allowlist above keeps local vaults, databases and credentials out.
tar -C "$staging" -czf "dist/$name.tar.gz" "$name"
appimage="apps/desktop/target/release/bundle/appimage/Chorus_${version}_amd64.AppImage"
test -s "$appimage"
install -m 755 "$appimage" "dist/Chorus_${version}_amd64.AppImage"
(
  cd dist
  sha256sum "Chorus_${version}_amd64.AppImage" "$name.tar.gz" > SHA256SUMS
)
