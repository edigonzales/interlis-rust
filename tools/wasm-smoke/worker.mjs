import init, { analyzeJson, compile, compilerVersion, parseJson } from '/pkg/interlis.js';
await init();
const operations = { probe: analyzeJson, parse: parseJson, compile };
self.onmessage = ({ data }) => {
  try {
    self.postMessage({ id: data.id, ok: true, response: operations[data.operation](data.request) });
  }
  catch (error) {
    const fatal = error instanceof WebAssembly.RuntimeError;
    self.postMessage({ id: data.id, ok: false, fatal, error: String(error) });
    if (fatal) self.close();
  }
};
self.postMessage({ type: 'ready', version: compilerVersion() });
