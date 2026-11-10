import init, { analyzeJson, compilerVersion } from '/pkg/interlis.js';
await init();
self.onmessage = ({ data }) => {
  try { self.postMessage({ id: data.id, ok: true, response: analyzeJson(data.request) }); }
  catch (error) {
    const fatal = error instanceof WebAssembly.RuntimeError;
    self.postMessage({ id: data.id, ok: false, fatal, error: String(error) });
    if (fatal) self.close();
  }
};
self.postMessage({ type: 'ready', version: compilerVersion() });
