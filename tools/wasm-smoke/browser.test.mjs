import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { request, escapedRequest, limited, badRequests, verify } from './cases.mjs';
test('Chromium module worker loads generated web package and shares the core', { timeout: 45_000 }, async () => {
  const assets = new Map([
    ['/', [new URL('./index.html', import.meta.url), 'text/html']],
    ['/host.mjs', [new URL('./host.mjs', import.meta.url), 'text/javascript']],
    ['/worker.mjs', [new URL('./worker.mjs', import.meta.url), 'text/javascript']],
    ['/pkg/interlis.js', [new URL('../../target/pkg-web/interlis.js', import.meta.url), 'text/javascript']],
    ['/pkg/interlis_bg.wasm', [new URL('../../target/pkg-web/interlis_bg.wasm', import.meta.url), 'application/wasm']],
  ]);
  const server = createServer(async (req, res) => {
    const asset = assets.get(req.url);
    if (req.method !== 'GET' || !asset) { res.writeHead(404).end(); return; }
    try { res.writeHead(200, { 'content-type': asset[1] }); res.end(await readFile(fileURLToPath(asset[0]))); }
    catch { res.destroy(new Error('Missing required smoke asset')); }
  });
  let browser;
  try {
    await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
    browser = await chromium.launch({ headless: true }); console.log(`Chromium ${browser.version()}`);
    const page = await browser.newPage(); const errors = []; const requests = [];
    page.on('pageerror', (error) => errors.push(String(error))); page.on('request', (request) => requests.push(request.url()));
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    assert.equal(await page.evaluate(() => window.hostReady), '0.1.0'); await page.waitForLoadState('networkidle');
    const initialRequests = requests.length;
    for (const input of badRequests) {
      const result = await page.evaluate((text) => window.callWorker(text), input);
      assert.equal(result.ok, false); assert.equal(result.fatal, false); assert.match(result.error, /REQUEST_/);
    }
    for (const [text, status] of [[request, 'incomplete'], [escapedRequest, 'incomplete'], [limited, 'limit_exceeded'], [request, 'incomplete']]) {
      const result = await page.evaluate((text) => window.callWorker(text), text);
      assert.equal(result.ok, true); verify(JSON.parse(result.response), status);
    }
    assert.deepEqual(errors, []); assert.equal(requests.length, initialRequests, 'No network after initialization');
  } finally {
    if (browser) await browser.close(); server.closeAllConnections(); await new Promise((resolve) => server.close(resolve));
  }
});
