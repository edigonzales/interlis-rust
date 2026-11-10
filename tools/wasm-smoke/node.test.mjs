import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { request, escapedRequest, limited, badRequests, verify, syntaxCases, verifySyntax, semanticCases, verifySemantic } from './cases.mjs';
const root = fileURLToPath(new URL('../../', import.meta.url));
const wasm = createRequire(import.meta.url)(join(root, 'target/pkg-node/interlis.js'));
test('generated Node package: boundary, Unicode, budget and subsequent calls', () => {
  assert.equal(wasm.compilerVersion(), '0.1.0');
  for (const input of badRequests) assert.throws(() => wasm.analyzeJson(input), /REQUEST_/);
  const result = JSON.parse(wasm.analyzeJson(request)); verify(result);
  assert.deepEqual(JSON.parse(wasm.analyzeJson(escapedRequest)), result);
  verify(JSON.parse(wasm.analyzeJson(limited)), 'limit_exceeded'); verify(JSON.parse(wasm.analyzeJson(request)));
  for(const [text,status,code] of syntaxCases) verifySyntax(JSON.parse(wasm.analyzeJson(JSON.stringify({sources:[{name:'syntax',text}]}))),text,status,code);
  const temporary = mkdtempSync(join(tmpdir(), 'interlis-parity-'));
  try {
    const path = join(temporary, 'request.json'); writeFileSync(path, request);
    const native = spawnSync(resolve(root, `target/debug/interlis-compiler${process.platform==='win32'?'.exe':''}`), ['probe', '--request', path, '--json'], { encoding: 'utf8', timeout: 10_000 });
    assert.ifError(native.error); assert.equal(native.status, 0); assert.equal(native.stderr, ''); assert.deepEqual(JSON.parse(native.stdout), result);
    for(const [input,status,detail] of semanticCases) {
      const report=JSON.parse(wasm.analyzeJson(input));verifySemantic(report,input,status,detail);
      writeFileSync(path,input);
      const cli=spawnSync(resolve(root,`target/debug/interlis-compiler${process.platform==='win32'?'.exe':''}`),['probe','--request',path,'--json'],{encoding:'utf8',timeout:10000});
      assert.ifError(cli.error);assert.equal(cli.status,status==='checked_subset'?0:status==='rejected'?1:4);assert.equal(cli.stderr,'');assert.deepEqual(JSON.parse(cli.stdout),report);
    }
  } finally { rmSync(temporary, { recursive: true, force: true }); }
});
