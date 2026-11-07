const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');

const root = path.join(__dirname, '..');

test('Docker image defaults to foreground web mode', () => {
  const dockerfile = fs.readFileSync(path.join(root, 'Dockerfile'), 'utf8');
  const commands = [...dockerfile.matchAll(/^CMD\s+(.+)$/gim)];
  assert.ok(commands.length, 'Dockerfile must specify a default command');
  assert.deepEqual(JSON.parse(commands.at(-1)[1]), ['--web', '--foreground']);
});

test('Compose builds from source instead of reusing a stale image', () => {
  const config = JSON.parse(execFileSync('docker', ['compose', 'config', '--format', 'json'], { cwd: root, encoding: 'utf8' }));
  const service = config.services['iptv-slayer'];
  assert.equal(service.pull_policy, 'build');
  assert.equal(service.build.context, root);
  assert.deepEqual(service.command, ['--web', '--foreground']);
});
