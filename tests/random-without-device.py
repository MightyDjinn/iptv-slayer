#!/usr/bin/env python3
"""Verify secure client IDs work without a /dev/urandom device (Linux/bubblewrap)."""
import json
import subprocess

result = subprocess.run(
    ['cargo', 'test', '--locked', '--bin', 'iptv-slayer', '--no-run', '--message-format=json'],
    check=True, capture_output=True, text=True,
)
executables = [
    item['executable']
    for line in result.stdout.splitlines()
    if (item := json.loads(line)).get('executable') and item.get('profile', {}).get('test')
]
if len(executables) != 1:
    raise RuntimeError(f'Expected one test executable, found {len(executables)}')
subprocess.run([
    'bwrap', '--ro-bind', '/', '/', '--tmpfs', '/dev', '--', executables[0],
    '--exact', 'storage::id_tests::generates_distinct_valid_ids', '--nocapture',
], check=True)
