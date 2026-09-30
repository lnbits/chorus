#!/usr/bin/env python3
"""Smoke-test the actual server archive without touching user data or public relays."""
import json
from pathlib import Path
import secrets
import socket
import sqlite3
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.request

with tempfile.TemporaryDirectory(prefix='chorus-package-') as temporary:
    root = Path(temporary)
    with tarfile.open(sys.argv[1]) as archive:
        archive.extractall(root, filter='data')
    packages = list(root.glob('chorus-*-linux-x86_64'))
    if len(packages) != 1:
        raise RuntimeError('Expected exactly one server package')
    package = packages[0]
    node = package / 'bin/chorus-node'
    subprocess.run([node, '--version'], check=True)
    subprocess.run([package / 'bin/fedwasm', '--version'], check=True)
    data = root / 'data'
    unlock = secrets.token_urlsafe(32)
    subprocess.run([node, '--data-dir', data, 'init', '--credentials-stdin'],
                   input=json.dumps({'admin_password': secrets.token_urlsafe(32),
                                     'key_password': unlock}), text=True, check=True)
    with sqlite3.connect(data / 'chorus.sqlite') as db:
        db.execute("UPDATE settings SET value = '[]' WHERE key = 'relays'")
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    origin = f'http://127.0.0.1:{port}'
    process = subprocess.Popen([node, '--data-dir', data, 'serve', '--bind',
                                f'127.0.0.1:{port}', '--origin', origin,
                                '--web-root', package / 'web', '--key-password-stdin'],
                               stdin=subprocess.PIPE)
    try:
        process.stdin.write(unlock.encode())
        process.stdin.close()
        for _ in range(200):
            if process.poll() is not None:
                raise RuntimeError('Packaged node exited during startup')
            try:
                with urllib.request.urlopen(origin + '/healthz', timeout=1) as response:
                    if response.status == 200:
                        break
            except OSError:
                time.sleep(0.1)
        else:
            raise TimeoutError('Packaged node did not become healthy')
        for route in ('/', '/login', '/federations'):
            with urllib.request.urlopen(origin + route, timeout=5) as response:
                html = response.read().decode()
                assert response.status == 200 and '<html' in html, route
        # Verify the JS/CSS referred to by the packaged bootstrap are served too.
        assets = list((package / 'web/_app/immutable').rglob('*.js'))
        assert assets, 'Missing frontend JavaScript'
        asset = assets[0].relative_to(package / 'web').as_posix()
        with urllib.request.urlopen(origin + '/' + asset, timeout=5) as response:
            assert response.read() == assets[0].read_bytes(), 'Asset/SPA fallback mismatch'
        print('Packaged node initialized, unlocked, and served its UI successfully.')
    finally:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
