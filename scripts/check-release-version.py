#!/usr/bin/env python3
"""Validate release versions before building; requires Python 3.11+."""
import json
from pathlib import Path
import re
import sys
import tomllib

root = Path(__file__).resolve().parent.parent

def toml(path):
    return tomllib.loads((root / path).read_text())

def json_file(path):
    return json.loads((root / path).read_text())

version = toml('Cargo.toml')['workspace']['package']['version']
tag = sys.argv[1] if len(sys.argv) == 2 else f'v{version}'
# Build metadata is deliberately excluded from release filenames/tags.
number = r'(?:0|[1-9][0-9]*)'
identifier = rf'(?:{number}|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
if len(sys.argv) > 2 or not re.fullmatch(
    rf'v{number}\.{number}\.{number}(?:-{identifier}(?:\.{identifier})*)?', tag
):
    sys.exit('Expected a release tag such as v0.1.0 or v0.1.0-rc.1')
expected = tag[1:]
versions = {
    'Cargo.toml': version,
    'apps/desktop/Cargo.toml': toml('apps/desktop/Cargo.toml')['package']['version'],
    'apps/desktop/tauri.conf.json': json_file('apps/desktop/tauri.conf.json')['version'],
    'apps/web/package.json': json_file('apps/web/package.json')['version'],
    'apps/web/package-lock.json': json_file('apps/web/package-lock.json')['version'],
    'apps/web/package-lock.json root': json_file('apps/web/package-lock.json')['packages']['']['version'],
}
for lock in ('Cargo.lock', 'apps/desktop/Cargo.lock'):
    for package in toml(lock)['package']:
        if package['name'].startswith('chorus-') and 'source' not in package:
            versions[f"{lock}: {package['name']}"] = package['version']
errors = [f'{path}: {actual} (expected {expected})' for path, actual in versions.items() if actual != expected]
if errors:
    sys.exit('Release tag and package versions differ:\n' + '\n'.join(errors))
print(expected)
