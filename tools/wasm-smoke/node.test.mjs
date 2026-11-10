import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { request, escapedRequest, limited, badRequests, verify } from './cases.mjs';
const root = fileURLToPath(new URL('../../', import.meta.url));
const wasm = createRequire(import.meta.url)(join(root, 'target/pkg-node/interlis.js'));
test('generated Node package: boundary, Unicode, budget and subsequent calls', () => {
  assert.equal(wasm.compilerVersion(), '0.1.0');
  for (const input of badRequests) assert.throws(() => wasm.analyzeJson(input), /REQUEST_/);
  const result = JSON.parse(wasm.analyzeJson(request)); verify(result);
  assert.deepEqual(JSON.parse(wasm.analyzeJson(escapedRequest)), result);
  verify(JSON.parse(wasm.analyzeJson(limited)), 'limit_exceeded'); verify(JSON.parse(wasm.analyzeJson(request)));
  const temporary = mkdtempSync(join(tmpdir(), 'interlis-parity-'));
  try {
    const path = join(temporary, 'request.json'); writeFileSync(path, request);
    const native = spawnSync(resolve(root, 'target/debug/interlis-compiler'), ['probe', '--request', path, '--json'], { encoding: 'utf8', timeout: 10_000 });
    assert.ifError(native.error); assert.equal(native.status, 4); assert.equal(native.stderr, ''); assert.deepEqual(JSON.parse(native.stdout), result);
  } finally { rmSync(temporary, { recursive: true, force: true }); }
});
