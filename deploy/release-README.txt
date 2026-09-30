Chorus Linux server package (x86_64, Ubuntu 22.04 or newer)

This is pre-production software. Read docs/security-review.md before use.
Rust, Cargo, Node.js and npm are not required to run this package.
Install your distribution's CA certificates package for secure relay connections.

From this extracted directory, as your normal user:

  ./bin/chorus-node --data-dir "$HOME/.local/share/chorus-server" init --username admin
  ./bin/chorus-node --data-dir "$HOME/.local/share/chorus-server" serve --web-root "$PWD/web"

Open http://127.0.0.1:8787 and sign in. Initialization prompts for passwords
without echoing them. Use init --import-nsec to import an existing identity.
Data lives outside this package, so extracting a later release preserves it.
Stop the old node before starting an updated binary; back up your data first.

For a VPS, install bin/* into /usr/local/bin and web/ into /opt/chorus/web.
Follow docs/deployment.md for the service account, initialization, encrypted
systemd credential and HTTPS reverse proxy. Examples are in deploy/.
The package does not install or start a service automatically.

The independent proof verifier is ./bin/fedwasm. A sample contract is in examples/.
