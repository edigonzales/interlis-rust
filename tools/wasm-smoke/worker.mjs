import init, { analyzeJson, compilerVersion, parseJson } from '/pkg/interlis.js';
await init();
self.onmessage = ({ data }) => {
  try {
    const operation = data.syntaxOnly ? parseJson : analyzeJson;
    self.postMessage({ id: data.id, ok: true, response: operation(data.request) });
  }
  catch (error) {
    const fatal = error instanceof WebAssembly.RuntimeError;
    self.postMessage({ id: data.id, ok: false, fatal, error: String(error) });
    if (fatal) self.close();
  }
};
self.postMessage({ type: 'ready', version: compilerVersion() });
