export const request = JSON.stringify({ sources: [{ name: 'memory:///ä😀.ili', text: '\uFEFF!! 😀\r\nINTERLIS 2.4;' }] });
export const limited = JSON.stringify({ sources: [{ name: 'a', text: 'ab' }], limits: { max_source_bytes: 1 } });
export const escapedRequest = request.replaceAll('😀', '\\ud83d\\ude00');
export const badRequests = ['{', '{"sources":[],"sources":[]}', '{"sources":[],"unknown":0}', '\uD800', '{"sources":[{"name":"a","text":"\\ud800"}]}', 42, null];
export function verify(report, expected = 'incomplete') {
  if (report.status !== expected || report.fullValidation !== false || report.validationScope !== 'i0-subset' || report.model !== null) throw new Error(`Unexpected analysis contract: ${JSON.stringify(report)}`);
  if (report.diagnostics.length !== 1) throw new Error('Expected one actual diagnostic');
  if (expected === 'incomplete') {
    const d = report.diagnostics[0];
    if (d.code !== 'ILI-UNSUPPORTED' || d.source !== 'memory:///ä😀.ili' || d.endLine !== 2 || d.endColumnUtf16 !== 14 || d.endByte !== 25) throw new Error(`Unexpected original location: ${JSON.stringify(d)}`);
  } else if (report.diagnostics[0].code !== 'ILI-LIMIT') throw new Error('Expected source limit diagnostic');
}
