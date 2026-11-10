export const request = JSON.stringify({ sources: [{ name: 'memory:///ä😀.ili', text: '\uFEFF!! 😀\r\nINTERLIS 2.4; MODEL A AT "x" VERSION "1" = END A.' }] });
export const limited = JSON.stringify({ sources: [{ name: 'a', text: 'ab' }], limits: { max_source_bytes: 1 } });
export const escapedRequest = request.replaceAll('😀', '\\ud83d\\ude00');
export const badRequests = ['{', '{"sources":[],"sources":[]}', '{"sources":[],"unknown":0}', '\uD800', '{"sources":[{"name":"a","text":"\\ud800"}]}', 42, null];
export function verify(report, expected = 'checked_subset') {
  if (report.status !== expected || report.fullValidation !== false || report.validationScope !== 'i0-subset') throw new Error(`Unexpected analysis contract: ${JSON.stringify(report)}`);
  if (expected === 'checked_subset') {
    if (report.diagnostics.length !== 0 || report.model.models[0].name !== 'A') throw new Error('Expected checked model A');
    const d = report.model.declarations[0];
    if (d.source !== 'memory:///ä😀.ili' || d.startByte !== 26 || d.endByte !== 61 || d.startLine !== 2 || d.startColumnUtf16 !== 15 || d.endLine !== 2 || d.endColumnUtf16 !== 50) throw new Error(`Unexpected original model location: ${JSON.stringify(d)}`);
  } else if (report.model !== null || !report.diagnostics.some(d=>d.code==='ILI-LIMIT')) throw new Error('Expected source limit report without model');
}

// A few actual parser/position counterprobes; broad syntax assertions live in Rust.
const syntaxDocument = 'INTERLIS 2.4; MODEL A AT "x" VERSION "1" = DOMAIN N=-1..2; D EXTENDS N = MANDATORY; END A. MODEL B AT "x" VERSION "1" = IMPORTS A; END B.';
export const syntaxCases = [
  [syntaxDocument, 'checked_subset', null],
  ['INTERLIS 2.4; MODEL A AT "https://example.invalid" VERSION "1" = DOMAIN Angle=0..360 CIRCULAR; END A.', 'incomplete', 'ILI-UNSUPPORTED'],
  ['INTERLIS 2.4; MODEL A (en) NOINCREMENTALTRANSFER AT "https://example.invalid" VERSION "1" = END A.', 'incomplete', 'ILI-UNSUPPORTED'],
  ['INTERLIS 2.4; MODEL A AT "https://example.invalid" VERSION "1" END A.', 'rejected', 'ILI-SYNTAX'],
  [syntaxDocument+' GARBAGE', 'rejected', 'ILI-SYNTAX'],
  [syntaxDocument+' /* outer /* inner */', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 2.4; MODEL A AT "😀" VERSION "1" = DOMAIN X=; Y=; END A.', 'rejected', 'ILI-SYNTAX'],
  [syntaxDocument, 'checked_subset', null],
];
export function verifySyntax(report, text, status, code) {
  if(report.status!==status || report.fullValidation!==false || (status==='checked_subset') !== (report.model!==null)) throw new Error('Unexpected syntax contract: '+JSON.stringify(report));
  if(code && !report.diagnostics.some(d=>d.code===code)) throw new Error('Missing actual syntax diagnostic');
  if(status==='checked_subset' && (report.model.models.length!==2 || report.diagnostics.length!==0)) throw new Error('Expected both checked models');
  if(text.includes('X=;')) {
    const errors=report.diagnostics.filter(d=>d.code==='ILI-SYNTAX');
    if(errors.length!==2 || errors[0].startByte!==55 || errors[0].startColumnUtf16!==54 || errors[1].startByte!==59 || errors[1].startColumnUtf16!==58) throw new Error('Lost Unicode syntax ranges: '+JSON.stringify(errors));
  }
}

function pair(version) {
  return {sources:[
    {name:'memory:///a-ä😀.ili',text:`INTERLIS ${version}; MODEL Base (en) AT "https://example.invalid" VERSION "v1" = DOMAIN Label=TEXT*40; Count=9007199254740992..9007199254740993; Scaled=0.10e-2..0.99e+2; END Base.`},
    {name:'memory:///z-model.ili',text:`\uFEFF!! 😀\r\nINTERLIS ${version};\r\nMODEL Data AT "https://example.invalid/😀" VERSION "v2" =\r\n IMPORTS Base; DOMAIN Required EXTENDS Base.Label=MANDATORY;\r\n TOPIC Items=CLASS Item=label:Required; count:Base.Count; END Item; END Items; END Data.`},
  ]};
}
const valid23=pair('2.3'),valid24=pair('2.4');
const missing=pair('2.4');missing.sources.shift();
const wrong=pair('2.4');wrong.sources[0].text=wrong.sources[0].text.replaceAll('Base','Other');
const prefix=pair('2.4');prefix.sources[1].text=prefix.sources[1].text.replace('Base.Count','Wrong.Base.Count');
const unsupported=pair('2.4');unsupported.sources[1].text=unsupported.sources[1].text.replace('END Item;','MANDATORY CONSTRAINT count > 0; END Item;');
const numericLimit=pair('2.4');numericLimit.limits={max_numeric_digits:4};
export const semanticCases = [
  [JSON.stringify(valid23),'checked_subset','2.3'],
  [JSON.stringify(valid24),'checked_subset','2.4'],
  [JSON.stringify(missing),'rejected','ILI-IMPORT-NOT-FOUND'],
  [JSON.stringify(wrong),'rejected','ILI-IMPORT-NOT-FOUND'],
  [JSON.stringify(prefix),'rejected','ILI-TYPE-NOT-FOUND'],
  [JSON.stringify(unsupported),'incomplete','ILI-UNSUPPORTED'],
  [JSON.stringify(numericLimit),'limit_exceeded','ILI-LIMIT'],
  [JSON.stringify(valid24),'checked_subset','2.4'],
];
export function verifySemantic(report,input,status,detail) {
  if(report.status!==status || report.fullValidation!==false || report.validationScope!=='i0-subset') throw new Error('Incorrect semantic status: '+JSON.stringify(report));
  if(status!=='checked_subset') {
    if(report.model!==null || !report.diagnostics.some(d=>d.code===detail)) throw new Error('Missing semantic failure without model');
    if(detail==='ILI-TYPE-NOT-FOUND') {
      const source=JSON.parse(input).sources[1].text;const index=source.indexOf('Wrong.Base.Count');const prefix=source.slice(0,index);const d=report.diagnostics.find(d=>d.code===detail);
      if(d.source!=='memory:///z-model.ili' || d.startByte!==new TextEncoder().encode(prefix).length || d.endByte!==d.startByte+16 || d.startLine!==prefix.split(/\r\n|\r|\n/).length || d.startColumnUtf16!==prefix.split(/\r\n|\r|\n/).at(-1).length+1) throw new Error('Incorrect original semantic range: '+JSON.stringify(d));
    }
    return;
  }
  const graph=report.model;if(!graph || report.diagnostics.length!==0 || graph.models.length!==2 || graph.models[0].name!=='Base' || graph.models[1].name!=='Data' || graph.models[1].imports.join(',')!=='Base' || graph.models.some(m=>m.languageVersion!==detail)) throw new Error('Incorrect bound model headers');
  const declaration=name=>graph.declarations.find(d=>d.qualifiedName===name);
  const members=graph.declarations.filter(d=>d.kind==='attribute' && d.qualifiedName.startsWith('Data.Items.Item.'));
  if(members.map(d=>d.name).join(',')!=='label,count') throw new Error('Incorrect member order');
  const label=declaration('Data.Items.Item.label');
  if(label.mandatory!==true || label.declaredMandatory!==false || label.effectiveType.kind!=='text' || label.effectiveType.maxLength!=='40' || label.typeReference.targetName!=='Data.Required' || declaration('Data.Required').typeReference.targetName!=='Base.Label') throw new Error('Lost derivation, mandatory or text type');
  const scaled=declaration('Base.Scaled').effectiveType;
  if(scaled.kind!=='numeric' || scaled.lower.fractionalDigits!==2 || scaled.lower.writtenExponent!==-2 || scaled.lower.scale!==4 || scaled.upper.writtenExponent!==2 || scaled.upper.scale!==0 || scaled.upper.original!=='0.99e+2') throw new Error('Lost scaled precision metadata');
  const count=declaration('Data.Items.Item.count');const t=count.effectiveType;
  if(count.mandatory!==false || t.kind!=='numeric' || t.lower.original!=='9007199254740992' || t.upper.original!=='9007199254740993' || t.upper.coefficient!=='9007199254740993' || t.upper.fractionalDigits!==0 || t.upper.writtenExponent!==0 || typeof t.upper.original!=='string') throw new Error('Lost exact numeric metadata');
}
