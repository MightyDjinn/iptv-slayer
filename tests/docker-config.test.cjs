const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');

const root = path.join(__dirname, '..');

test('Pages deployment waits for the Docker Compose integration job', () => {
  const workflow = fs.readFileSync(path.join(root, '.github/workflows/pr-tests.yml'), 'utf8');
  const deploy = workflow.match(/^  deploy:\s*\n([\s\S]*)/m);
  assert.ok(deploy, 'Pages deploy job must exist');
  const needs = deploy[1].match(/^    needs:\s*\[([^\]]+)\]/m);
  assert.ok(needs, 'Pages deploy job must declare dependencies');
  assert.ok(needs[1].split(',').map((job) => job.trim()).includes('docker-integration'),
    'Pages must not deploy if Docker Compose integration fails');
});

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
