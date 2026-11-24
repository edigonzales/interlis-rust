import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { request, escapedRequest, limited, badRequests, verify, syntaxCases, verifySyntax, semanticCases, verifySemantic, parseCases, verifyParse, compileCases } from './cases.mjs';
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
    for (const [text, status] of [[request, 'checked_subset'], [escapedRequest, 'checked_subset'], [limited, 'limit_exceeded'], [request, 'checked_subset']]) {
      const result = await page.evaluate((text) => window.callWorker(text), text);
      assert.equal(result.ok, true); verify(JSON.parse(result.response), status);
    }
    for(const [text,status,code] of syntaxCases) {
      const r=await page.evaluate(input=>window.callWorker(input),JSON.stringify({sources:[{name:'syntax',text}]}));
      assert.equal(r.ok,true);verifySyntax(JSON.parse(r.response),text,status,code);
    }
    for(const [input,status,detail] of semanticCases) {
      const r=await page.evaluate(input=>window.callWorker(input),input);assert.equal(r.ok,true);
      verifySemantic(JSON.parse(r.response),input,status,detail);
    }
    const badSyntaxInput = await page.evaluate(() => window.callWorker('\uD800', 'parse'));
    assert.equal(badSyntaxInput.ok, false);
    assert.equal(badSyntaxInput.fatal, false);
    assert.match(badSyntaxInput.error, /REQUEST_UTF16/);
    for (const [input, status] of parseCases) {
      const result = await page.evaluate((text) => window.callWorker(text, 'parse'), input);
      assert.equal(result.ok, true);
      verifyParse(JSON.parse(result.response), status);
    }
    const directory = await mkdtemp(join(tmpdir(), 'interlis-compile-browser-'));
    try {
      const cli = fileURLToPath(new URL(`../../target/debug/interlis-compiler${process.platform === 'win32' ? '.exe' : ''}`, import.meta.url));
      const path = join(directory, 'request.json');
      for (const [input, status, exit] of compileCases) {
        await writeFile(path, input);
        const native = spawnSync(cli, ['compile', '--request', path, '--json'], { encoding: 'utf8', timeout: 10_000 });
        assert.ifError(native.error);
        assert.equal(native.status, exit);
        assert.equal(native.stderr, '');
        const result = await page.evaluate((request) => window.callWorker(request, 'compile'), JSON.parse(input));
        assert.equal(result.ok, true);
        assert.equal(result.response.status, status);
        assert.deepEqual(result.response, JSON.parse(native.stdout));
      }
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
    assert.deepEqual(errors, []); assert.equal(requests.length, initialRequests, 'No network after initialization');
  } finally {
    if (browser) await browser.close(); server.closeAllConnections(); await new Promise((resolve) => server.close(resolve));
  }
});
