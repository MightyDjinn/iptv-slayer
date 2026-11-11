const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const {execFileSync} = require('node:child_process');
const {pathToFileURL} = require('node:url');

test('a selected file can be loaded when the URL field contains an invalid URL', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'iptv-browser-'));
  try {
    const html = fs.readFileSync('src/index.html', 'utf8');
    const app = fs.readFileSync('src/app.js', 'utf8');
    const fixture = html.replace('<script src="/app.js"></script>', `<script>
      window.fetch = async (url, options) => {
        if (url === '/api/playlist') {
          document.body.dataset.payload = options.body;
          return {ok:true, json:async()=>[]};
        }
        return {ok:true, json:async()=>url === '/api/source' ? null : []};
      };
    </script><script>${app}</script><script>
      const input = document.querySelector('#source-file');
      const files = new DataTransfer();
      files.items.add(new File(['#EXTM3U\\n'], 'channels.m3u', {type:'audio/x-mpegurl'}));
      input.files = files.files;
      input.dispatchEvent(new Event('input', {bubbles:true}));
      document.querySelector('#source-url').value = 'not-a-url';
      new MutationObserver(() => {
        if (document.querySelector('#source-status').textContent.startsWith('Loaded ')) {
          document.body.dataset.validationRestored = String(!document.querySelector('#source-form').noValidate);
        }
      }).observe(document.querySelector('#source-status'), {childList:true});
      document.querySelector('#source-form').requestSubmit();
    </script>`);
    const file = path.join(dir, 'fixture.html');
    fs.writeFileSync(file, fixture);
    const browser = process.env.CHROME_BIN || 'chromium-browser';
    const output = execFileSync(browser, ['--headless', '--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage', '--virtual-time-budget=5000', '--dump-dom', pathToFileURL(file).href], {encoding:'utf8', stdio:['ignore','pipe','ignore'], timeout:30000});
    assert.match(output, /data-payload="\{&quot;content&quot;:&quot;#EXTM3U\\n&quot;\}"/);
    assert.ok(output.includes('data-validation-restored="true"'), 'native URL validation should be restored after upload');
  } finally {
    fs.rmSync(dir, {recursive:true, force:true});
  }
});
