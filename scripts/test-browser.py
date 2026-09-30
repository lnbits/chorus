#!/usr/bin/env python3
"""Exercise the built web app against a real, isolated disposable node."""
import json
import os
from pathlib import Path
import secrets
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.request

root = Path(__file__).resolve().parent.parent
binary = root / 'target/debug/chorus-node'
with tempfile.TemporaryDirectory(prefix='chorus-browser-') as data:
    base = [str(binary), '--data-dir', data]
    admin = secrets.token_urlsafe(32)
    unlock = secrets.token_urlsafe(32)
    subprocess.run(base + ['init', '--credentials-stdin'], input=json.dumps({
        'admin_password': admin, 'key_password': unlock
    }), text=True, check=True)
    # Keep disposable test federations off the public default relays.
    with sqlite3.connect(Path(data) / 'chorus.sqlite') as db:
        db.execute("UPDATE settings SET value = '[]' WHERE key = 'relays'")
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    origin = f'http://127.0.0.1:{port}'
    node = subprocess.Popen(base + [
        'serve', '--bind', f'127.0.0.1:{port}', '--origin', origin,
        '--web-root', str(root / 'apps/web/build'), '--key-password-stdin'
    ], stdin=subprocess.PIPE)
    try:
        node.stdin.write(unlock.encode())
        node.stdin.close()
        for _ in range(200):
            if node.poll() is not None:
                raise RuntimeError('Test node exited before becoming healthy')
            try:
                with urllib.request.urlopen(origin + '/healthz', timeout=1) as response:
                    if response.status == 200:
                        break
            except OSError:
                time.sleep(0.1)
        else:
            raise TimeoutError('Test node failed to start')
        environment = dict(os.environ, CHORUS_E2E_URL=origin,
                           CHORUS_E2E_USERNAME='admin', CHORUS_E2E_PASSWORD=admin)
        subprocess.run(['npm', 'run', 'test:e2e'], cwd=root / 'apps/web',
                       env=environment, check=True)
    finally:
        node.terminate()
        try:
            node.wait(timeout=10)
        except subprocess.TimeoutExpired:
            node.kill()
            node.wait()
