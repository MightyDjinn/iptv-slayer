const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

test('an earlier playback rejection does not overwrite the current channel status', async () => {
  const pending = [];
  const element = () => ({
    children: [], listeners: {}, value: '', textContent: '',
    addEventListener(type, listener) { this.listeners[type] = listener; },
    append(child) { this.children.push(child); },
    replaceChildren() { this.children = []; },
  });
  const list = element(), search = element(), video = element(), status = element();
  video.play = () => new Promise((resolve, reject) => pending.push({ resolve, reject }));
  const elements = { '#channels': list, '#search': search, video, '#status': status };
  for (const selector of ['#source-form', '#source-url', '#source-user', '#source-password', '#source-file', '#source-status']) elements[selector] = element();
  const document = { querySelector: selector => elements[selector], createElement: element };
  const fetch = async () => ({ ok: true, json: async () => [
    { name: 'First', url: 'https://example.com/first' },
    { name: 'Second', url: 'https://example.com/second' },
  ] });
  const html = fs.readFileSync('src/index.html', 'utf8');
  vm.runInNewContext(html.match(/<script>([\s\S]*?)<\/script>/)[1], { document, fetch });
  await new Promise(resolve => setImmediate(resolve));
  list.children[0].children[0].listeners.click();
  list.children[1].children[0].listeners.click();
  pending[0].reject(new Error('interrupted'));
  pending[1].resolve();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(status.textContent, 'Playing: Second');
});
