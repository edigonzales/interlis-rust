export const request = JSON.stringify({ sources: [{ name: 'memory:///ä😀.ili', text: '\uFEFF!! 😀\r\nINTERLIS 2.4; MODEL A AT \"x\" VERSION \"1\" = END A.' }] });
export const limited = JSON.stringify({ sources: [{ name: 'a', text: 'ab' }], limits: { max_source_bytes: 1 } });
export const escapedRequest = request.replaceAll('😀', '\\ud83d\\ude00');
export const badRequests = ['{', '{"sources":[],"sources":[]}', '{"sources":[],"unknown":0}', '\uD800', '{"sources":[{"name":"a","text":"\\ud800"}]}', 42, null];
export function verify(report, expected = 'incomplete') {
  if (report.status !== expected || report.fullValidation !== false || report.validationScope !== 'i0-subset' || report.model !== null) throw new Error(`Unexpected analysis contract: ${JSON.stringify(report)}`);
  if (report.diagnostics.length !== 1) throw new Error('Expected one actual diagnostic');
  if (expected === 'incomplete') {
    const d = report.diagnostics[0];
    if (d.code !== 'ILI-UNSUPPORTED' || d.source !== 'memory:///ä😀.ili' || d.endLine !== 2 || d.startByte !== 12 || d.startLine !== 2 || d.startColumnUtf16 !== 1 || d.endColumnUtf16 !== 50 || d.endByte !== 61) throw new Error(`Unexpected original location: ${JSON.stringify(d)}`);
  } else if (report.diagnostics[0].code !== 'ILI-LIMIT') throw new Error('Expected source limit diagnostic');
}

// A few actual parser/position counterprobes; broad syntax assertions live in Rust.
const syntaxDocument = 'INTERLIS 2.4; MODEL A AT "x" VERSION "1" = DOMAIN N=-1..2; D EXTENDS N = MANDATORY; END A. MODEL B AT "x" VERSION "1" = IMPORTS A; END B.';
export const syntaxCases = [
  [syntaxDocument, 'incomplete', 'ILI-UNSUPPORTED'],
  ['INTERLIS 2.4; MODEL A AT \"https://example.invalid\" VERSION \"1\" = DOMAIN Angle=0..360 CIRCULAR; END A.', 'incomplete', 'ILI-UNSUPPORTED'],
  [syntaxDocument+' GARBAGE', 'rejected', 'ILI-SYNTAX'],
  [syntaxDocument+' /* outer /* inner */', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 2.4; MODEL A AT "😀" VERSION "1" = DOMAIN X=; Y=; END A.', 'rejected', 'ILI-SYNTAX'],
  [syntaxDocument, 'incomplete', 'ILI-UNSUPPORTED'],
];
export function verifySyntax(report, text, status, code) {
  if(report.status!==status || report.fullValidation!==false || report.model!==null || !report.diagnostics.some(d=>d.code===code)) throw new Error('Unexpected syntax contract: '+JSON.stringify(report));
  if(text.includes('X=;')) {
    const errors=report.diagnostics.filter(d=>d.code==='ILI-SYNTAX');
    if(errors.length!==2 || errors[0].startByte!==55 || errors[0].startColumnUtf16!==54 || errors[1].startByte!==59 || errors[1].startColumnUtf16!==58) throw new Error('Lost Unicode syntax ranges: '+JSON.stringify(errors));
  }
}
