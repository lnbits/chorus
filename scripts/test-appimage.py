#!/usr/bin/env python3
"""Exercise AppImage CLI and headless startup without a display or public relays."""
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

image = str(Path(sys.argv[1]).resolve())
environment = {key: value for key, value in os.environ.items()
               if key not in ('DISPLAY', 'WAYLAND_DISPLAY')}
environment['APPIMAGE_EXTRACT_AND_RUN'] = '1'
base = [image, '--appimage-extract-and-run']
help_text = subprocess.check_output(base + ['--help'], env=environment, text=True)
assert all(flag in help_text for flag in ('--headless', '--host', '--port', '--origin'))
for invalid in (['--host', '0.0.0.0'], ['--port', '65536'], ['--init']):
    result = subprocess.run(base + invalid, env=environment, capture_output=True, text=True)
    assert result.returncode != 0, invalid
    assert 'cannot open display' not in result.stderr.lower(), result.stderr

with tempfile.TemporaryDirectory(prefix='chorus-appimage-') as temporary:
    data = Path(temporary) / 'data'
    key = secrets.token_urlsafe(32)
    admin = secrets.token_urlsafe(32)
    command = base + ['--headless', '--data-dir', str(data)]
    subprocess.run(command + ['--init', '--credentials-stdin'], env=environment,
                   input=json.dumps({'admin_password': admin, 'key_password': key}),
                   text=True, check=True)
    with sqlite3.connect(data / 'chorus.sqlite') as db:
        db.execute("UPDATE settings SET value = '[]' WHERE key = 'relays'")
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    local = f'http://127.0.0.1:{port}'
    public_origin = 'https://chorus.example.test'
    process = subprocess.Popen(command + ['--host', '0.0.0.0', '--port', str(port),
                                         '--origin', public_origin, '--key-password-stdin'],
                               env=environment, stdin=subprocess.PIPE, start_new_session=True)
    try:
        process.stdin.write(key.encode())
        process.stdin.close()
        for _ in range(300):
            if process.poll() is not None:
                raise RuntimeError('Headless AppImage exited during startup')
            try:
                with urllib.request.urlopen(local + '/healthz', timeout=1) as response:
                    if response.status == 200:
                        break
            except OSError:
                time.sleep(0.1)
        else:
            raise TimeoutError('Headless AppImage did not serve the requested port')
        with urllib.request.urlopen(local + '/login', timeout=5) as response:
            assert response.status == 200 and '<html' in response.read().decode()
        payload = json.dumps({'username': 'admin', 'password': admin}).encode()
        # The public HTTPS origin must reach the node's CSRF and cookie settings.
        wrong = urllib.request.Request(local + '/api/v1/login', data=payload,
                                       headers={'Content-Type': 'application/json', 'Origin': local})
        try:
            urllib.request.urlopen(wrong, timeout=5)
            raise AssertionError('Unexpectedly accepted the wrong browser origin')
        except urllib.error.HTTPError as error:
            assert error.code == 400, error.code
            assert json.load(error)['error'] == 'invalid request origin'
        login = urllib.request.Request(local + '/api/v1/login', data=payload,
                                       headers={'Content-Type': 'application/json', 'Origin': public_origin})
        with urllib.request.urlopen(login, timeout=5) as response:
            assert response.status == 200
            assert 'Secure' in response.headers.get('Set-Cookie', '')
        print('Headless AppImage: init, requested host/port, UI, origin and secure cookies passed.')
    finally:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
