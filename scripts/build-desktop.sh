#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
target_triple="$(rustc -vV | sed -n 's/^host: //p')"
cargo build --release -p chorus-node
mkdir -p apps/desktop/binaries
extension=''
if [[ "$target_triple" == *windows* ]]; then extension='.exe'; fi
cp "target/release/chorus-node${extension}" "apps/desktop/binaries/chorus-node-${target_triple}${extension}"
cd apps/desktop
cargo tauri build
