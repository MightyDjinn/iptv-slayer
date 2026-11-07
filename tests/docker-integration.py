#!/usr/bin/env python3
"""Opt-in Linux Docker integration test; creates only uniquely named resources."""
import json
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parent.parent
TOKEN = uuid.uuid4().hex
PROJECT = f"iptv-integration-{TOKEN}"
IMAGE = f"iptv-integration-{TOKEN}:test"


def run(*args, **kwargs):
    print('+', ' '.join(map(str, args)), flush=True)
    return subprocess.run(args, check=True, text=True, **kwargs)


def output(*args):
    return run(*args, stdout=subprocess.PIPE).stdout.strip()


def main():
    # Host networking is intentional: the production service binds to loopback.
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    with tempfile.TemporaryDirectory(prefix=PROJECT + '-') as directory:
        override = Path(directory) / 'override.json'
        override.write_text(json.dumps({'services': {'iptv-slayer': {
            'image': IMAGE,
            'command': ['--web', '--foreground', '--port', str(port)]
        }}}))
        empty_env = Path(directory) / 'empty.env'
        empty_env.write_text('')
        # Explicit files/project isolate this test from user Compose settings and resources.
        compose = ['docker', 'compose', '--env-file', str(empty_env), '-p', PROJECT,
                   '-f', str(ROOT / 'compose.yaml'), '-f', str(override)]
        stale_id = None
        try:
            dockerfile = f'FROM scratch\nLABEL integration-test={TOKEN}\n'
            run('docker', 'build', '-t', IMAGE, '-', input=dockerfile, cwd=ROOT)
            stale_id = output('docker', 'image', 'inspect', IMAGE, '--format', '{{.Id}}')
            print('Stale image:', stale_id, flush=True)
            # Deliberately no --build: production pull_policy must trigger the rebuild.
            run(*compose, 'up', '-d', cwd=ROOT)
            rebuilt_id = output('docker', 'image', 'inspect', IMAGE, '--format', '{{.Id}}')
            if stale_id == rebuilt_id:
                raise AssertionError('Compose reused the stale image')
            container = output(*compose, 'ps', '-q', 'iptv-slayer')
            if not container:
                raise AssertionError('Compose did not start the service')
            actual_id = output('docker', 'inspect', container, '--format', '{{.Image}}')
            if actual_id != rebuilt_id:
                raise AssertionError('Container is not running the rebuilt image')
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            deadline = time.monotonic() + 30
            while True:
                running = output('docker', 'inspect', container, '--format', '{{.State.Running}}')
                if running != 'true':
                    raise AssertionError('Web container exited before responding')
                try:
                    with opener.open(f'http://127.0.0.1:{port}/', timeout=2) as response:
                        body = response.read().decode('utf-8')
                        if response.status != 200 or 'Playlist source' not in body:
                            raise AssertionError('Unexpected web response')
                    break
                except (urllib.error.URLError, TimeoutError, ConnectionError):
                    if time.monotonic() >= deadline:
                        raise AssertionError('Web server did not respond in time')
                    time.sleep(0.5)
            logs = output(*compose, 'logs', '--no-color', 'iptv-slayer')
            if f'Web UI: http://127.0.0.1:{port}' not in logs:
                raise AssertionError('Container did not report starting on the test port')
            print(f'PASS: rebuilt {rebuilt_id}; playlist-free HTTP 200 on port {port}', flush=True)
        except BaseException:
            subprocess.run([*compose, 'logs', '--no-color'], cwd=ROOT, check=False)
            raise
        finally:
            # Never prune or remove images/volumes belonging to other projects.
            failures = []
            for command in ([*compose, 'down', '--volumes', '--remove-orphans'],
                            ['docker', 'image', 'rm', IMAGE]):
                if subprocess.run(command, cwd=ROOT, check=False).returncode:
                    failures.append(' '.join(command))
            if stale_id and subprocess.run(['docker', 'image', 'inspect', stale_id],
                                           cwd=ROOT, stdout=subprocess.DEVNULL,
                                           stderr=subprocess.DEVNULL).returncode == 0:
                if subprocess.run(['docker', 'image', 'rm', stale_id],
                                  cwd=ROOT, check=False).returncode:
                    failures.append('remove stale image')
            if failures:
                print('Cleanup failed: ' + '; '.join(failures), file=sys.stderr)
                if sys.exc_info()[0] is None:
                    raise RuntimeError('Docker integration cleanup failed')


if __name__ == '__main__':
    main()
