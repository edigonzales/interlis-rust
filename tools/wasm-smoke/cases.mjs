export const request = JSON.stringify({ sources: [{ name: 'memory:///ä😀.ili', text: '\uFEFF!! 😀\r\nINTERLIS 2.4; MODEL A AT "x" VERSION "1" = END A.' }] });
export const limited = JSON.stringify({ sources: [{ name: 'a', text: 'ab' }], limits: { max_source_bytes: 1 } });
export const escapedRequest = request.replaceAll('😀', '\\ud83d\\ude00');
export const badRequests = ['{', '{"sources":[],"sources":[]}', '{"sources":[],"unknown":0}', '\uD800', '{"sources":[{"name":"a","text":"\\ud800"}]}', 42, null];
const syntaxRequest = (text) => JSON.stringify({ sources: [{ name: 'syntax', text }] });
export const parseCases = [
  [request, 'parsed', 0],
  [syntaxRequest('INTERLIS 2.5; ? "foreign'), 'unsupported_version', 4],
  [syntaxRequest('INTERLIS 2.4; MODEL A AT "😀" VERSION "1" = DOMAIN A=; B=; END A.'), 'invalid', 1],
  [limited, 'limit_exceeded', 4],
  [syntaxRequest('INTERLIS 2.4; MODEL A AT "x" VERSION "1" = IMPORTS Missing; TOPIC T=VIEW V PROJECTION OF Missing.C; = ALL OF C; END V; END T; END A.'), 'parsed', 0],
  [request, 'parsed', 0],
];
export function verifyParse(report, status) {
  if (report.status !== status || report.validationScope !== 'syntax' || report.fullValidation !== false || 'model' in report) {
    throw new Error(`Unexpected syntax report: ${JSON.stringify(report)}`);
  }
  if (status === 'parsed') {
    if (report.diagnostics.length !== 0 || report.documents.length !== 1 || report.documents[0].languageVersion !== '2.4' || report.documents[0].models[0].name !== 'A') {
      throw new Error(`Unexpected parsed document: ${JSON.stringify(report)}`);
    }
  } else if (report.documents !== null || report.diagnostics.length === 0) {
    throw new Error(`Unexpected failure report: ${JSON.stringify(report)}`);
  }
}
export function verify(report, expected = 'checked_subset') {
  if (report.status !== expected || report.fullValidation !== false || report.validationScope !== 'i0-subset') throw new Error(`Unexpected analysis contract: ${JSON.stringify(report)}`);
  if (expected === 'checked_subset') {
    if (report.diagnostics.length !== 0 || report.model.models[0].name !== 'A') throw new Error('Expected checked model A');
    const d = report.model.declarations[0];
    if (d.source !== 'memory:///ä😀.ili' || d.startByte !== 26 || d.endByte !== 61 || d.startLine !== 2 || d.startColumnUtf16 !== 15 || d.endLine !== 2 || d.endColumnUtf16 !== 50) throw new Error(`Unexpected original model location: ${JSON.stringify(d)}`);
  } else if (report.model !== null || !report.diagnostics.some(d=>d.code==='ILI-LIMIT')) throw new Error('Expected source limit report without model');
}

// A few actual parser/position counterprobes; broad syntax assertions live in Rust.
const syntaxDocument = 'INTERLIS 2.4; MODEL A AT "x" VERSION "1" = UNIT FullTurn = 2*PI [INTERLIS.rad]; DOMAIN N=-1..2; D EXTENDS N = MANDATORY; Angle=0..360 CIRCULAR [INTERLIS.rad]; Calendar=FORMAT INTERLIS.XMLDate "2005-02-03".."2030-12-31"; END A. MODEL B NOINCREMENTALTRANSFER AT "x" VERSION "1" // notes // = CHARSET "csUTF8"; IMPORTS A; FUNCTION gate(value:BOOLEAN):BOOLEAN; TOPIC T=OID AS INTERLIS.I32OID; CLASS C=active:BOOLEAN; MANDATORY CONSTRAINT requiresActive: gate(active); END C; END T; END B.';
const classValues = 'INTERLIS 2.3; MODEL M AT "x" VERSION "1" = DOMAIN D=CLASS RESTRICTION (INTERLIS.METAOBJECT); END M. MODEL N AT "x" VERSION "1" = END N.';
const population = 'INTERLIS 2.4; MODEL M AT "x" VERSION "1" = TOPIC T=CLASS C=flag:BOOLEAN; SET CONSTRAINT (BASKET) nonempty: WHERE flag: INTERLIS.objectCount(ALL) > 0; END C; END T; END M.';
export const syntaxCases = [
  ['INTERLIS 2.4; MODEL M AT "x" VERSION "1" = SIGN BASKET B ~ S.T OBJECTS OF Mark: A; VIEW TOPIC T=VIEW V PROJECTION OF b ~ C; = ALL OF b; END V; GRAPHIC G BASED ON V=r:WHERE ready (Sign:={B.A}; Level:=ACCORDING b->kind (1 WHEN IN #a .. #b)); END G; END T; END M.', 'rejected', 'ILI-NAME-NOT-FOUND'],
  ['INTERLIS 2.3; MODEL M AT "x" VERSION "1" = TOPIC T=VIEW V JOIN OF C; = END V; END T; END M.', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 2.3; MODEL M AT "x" VERSION "1" = FUNCTION f(x:BOOLEAN):BOOLEAN; CLASS C=a:BOOLEAN := Missing(THIS); MANDATORY CONSTRAINT a OR b AND c == d; END C; END M.', 'rejected', 'ILI-FUNCTION-CONTRACTED'],
  ['INTERLIS 2.4; MODEL M AT "x" VERSION "1" = FUNCTION f():BOOLEAN; CLASS C=MANDATORY CONSTRAINT named: a == b AND c OR d + e * f => g; UNIQUE(BASKET) key: WHERE a: parts[FIRST]->name; SET CONSTRAINT(BASKET) all: WHERE a: Missing(ALL(ANYCLASS)); END C; END M.', 'rejected', 'ILI-PATH-NAME'],
  ['INTERLIS 2.3; MODEL M AT "x" VERSION "1" = CLASS C=MANDATORY CONSTRAINT f(); END C; END M.', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 2.3; MODEL M AT "x" VERSION "1" = TOPIC T=CLASS C=a:BOOLEAN := b AND c; END C; END T; END M.', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 2.4; MODEL M AT "x" VERSION "1" = TOPIC T=CLASS C=a:BOOLEAN := b AND c; END C; END T; END M.', 'incomplete', 'ILI-UNSUPPORTED'],
  ['INTERLIS 2.4; MODEL M AT "x" VERSION "1" = CLASS C=MANDATORY CONSTRAINT a AND; MANDATORY CONSTRAINT b ==; MANDATORY CONSTRAINT #true; END C; END M.', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 2.4; MODEL M AT "x" VERSION "1" = DOMAIN D=BOOLEAN CONSTRAINTS c: Missing(THIS); END M.', 'rejected', 'ILI-FUNCTION-NAME'],
  ['INTERLIS 2.3; MODEL M AT "x" VERSION "1" = UNIT Turn=2*PI[INTERLIS.rad]; DOMAIN N=-0.1200E3..0.1201E3 CIRCULAR [INTERLIS.m] {M.T.B.System[2]}; END M.', 'incomplete', 'ILI-UNSUPPORTED'],
  ['INTERLIS 2.4; MODEL M AT "x" VERSION "1" = DOMAIN C=MULTICOORD NUMERIC,NUMERIC REFSYS "EPSG:2056"; L=DIRECTED MULTIPOLYLINE WITH (ARCS) VERTEX C WITHOUT OVERLAPS; STRUCTURE S=names:LIST OF TEXT*20; END S; END M.', 'rejected', 'ILI-DOMAIN-ABSTRACT'],
  ['INTERLIS 2.3; MODEL M AT "x" VERSION "1" = DOMAIN G=SURFACE LINE ATTRIBUTES INTERLIS.SurfaceEdge; END M.', 'incomplete', 'ILI-UNSUPPORTED'],
  ['INTERLIS 2.4; MODEL M AT "x" VERSION "1" = DOMAIN F=FORMAT BASED ON INTERLIS.GregorianDate ("😀" Year/4 "-" Month/INTERLIS.XMLDate); END M.', 'incomplete', 'ILI-UNSUPPORTED'],
  [classValues, 'checked_subset'],
  [population, 'checked_subset'],
  ['INTERLIS 2.4; MODEL M AT "x" VERSION "1" = CLASS C EXTENDS INTERLIS.BOOLEAN=END C; END M.', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 2.3; MODEL M AT "x" VERSION "1" = STRUCTURE S=x:MANDATORY; PARAMETER p:MANDATORY; END S; END M.', 'incomplete', 'ILI-UNSUPPORTED'],
  ['INTERLIS 2.4; MODEL M AT "x" VERSION "1" = DOMAIN G=BOOLEAN; X=BOOLEAN; CONTEXT first=G=X; empty= END M.', 'rejected', 'ILI-CONTEXT-GENERIC'],
  ['INTERLIS 2.4; SYMBOLOGY MODEL M AT "x" VERSION "1" = CLASS C (ABSTRACT) EXTENDS INTERLIS.SIGN=PARAMETER p:METAOBJECT OF INTERLIS.METAOBJECT; END C; END M.', 'incomplete', 'ILI-UNSUPPORTED'],
  ['INTERLIS 2.4; MODEL Original AT "x" VERSION "old" = END Original. CONTRACTED TYPE MODEL M (de) NOINCREMENTALTRANSFER AT "x" VERSION "1" // details // TRANSLATION OF Original ["old"] = CHARSET "UTF-8"; XMLNS "urn:x"; IMPORTS UNQUALIFIED Missing, INTERLIS; END M.', 'rejected', 'ILI-IMPORT-NOT-FOUND'],
  ['INTERLIS 2.3; MODEL M AT "x" VERSION "1" = TOPIC T=CLASS C=END C; ASSOCIATION = first -- {9007199254740993..*} C OR ANYCLASS; second -<#> {1} C; ATTRIBUTE flag:BOOLEAN; END; END T; END M.', 'rejected', 'ILI-ROLE-ALTERNATIVES'],
  ['INTERLIS 2.4; MODEL M AT "x" VERSION "1" = DOMAIN G=BOOLEAN; X=BOOLEAN; Y=BOOLEAN; CONTEXT first=G=X OR Y; TOPIC T=DEFERRED GENERICS G; CLASS C (ABSTRACT) EXTENDS INTERLIS.METAOBJECT = NO OID; ATTRIBUTE SUBDIVISION n (EXTENDED):BOOLEAN; PARAMETER p:METAOBJECT; END C; END T; END M.', 'incomplete', 'ILI-UNSUPPORTED'],
  ['INTERLIS 2.4; MODEL M AT "x" VERSION "1" = TOPIC T=ASSOCIATION = a -- {1..} C; END; END T; END M.', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 2.4; MODEL M AT "x" VERSION "1" = DOMAIN D=BOOLEAN; ? END M.', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 2.4; MODEL M AT "x" VERSION "1" // open', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 2.4; MODEL M AT "x" VERSION "1" = DOMAIN A=BOOLEAN DOMAIN B=; DOMAIN Good=BOOLEAN; END M.', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 0.23E1; MODEL M AT "x" VERSION "1" = END M. MODEL B AT "x" VERSION "1" = END B.', 'checked_subset', null],
  ['\uFEFF!!@ author="😀"\r\n/* outer /* inner */ tail */\r\nINTERLIS 2.4; MODEL A AT "x" VERSION "1" = DOMAIN N=0.10E-2..0.99E+2; END A. MODEL B AT "x" VERSION "1" = IMPORTS A; END B.', 'checked_subset', null],
  ['INTERLIS 2.4; MODEL A AT "x" VERSION "1" = DOMAIN D=1E2..2E2; END A.', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 2.4; MODEL '+ 'A'.repeat(256)+' AT "x" VERSION "1" = END A.', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS ;', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 2.5;', 'incomplete', 'ILI-UNSUPPORTED'],
  ['INTERLIS +2.5;', 'incomplete', 'ILI-UNSUPPORTED'],
  ['INTERLIS 2.5; ? 😀 \"open /*', 'incomplete', 'ILI-UNSUPPORTED'],
  [syntaxDocument, 'checked_subset', null],
  ['INTERLIS 2.4; MODEL A AT "https://example.invalid" VERSION "1" END A.', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 2.4; MODEL A AT \"https://example.invalid\" VERSION \"1\" = DOMAIN ; END A.', 'rejected', 'ILI-SYNTAX'],
  [syntaxDocument+' GARBAGE', 'rejected', 'ILI-SYNTAX'],
  [syntaxDocument+' /* outer /* inner */', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 2.4; MODEL A AT "x" VERSION "1" = TOPIC T=CLASS C=-> END C; END T; END A.', 'rejected', 'ILI-SYNTAX'],
  ['INTERLIS 2.4; MODEL A AT "😀" VERSION "1" = DOMAIN X=; Y=; END A.', 'rejected', 'ILI-SYNTAX'],
  [syntaxDocument, 'checked_subset', null],
];
export function verifySyntax(report, text, status, code) {
  if(report.status!==status || report.fullValidation!==false || (status==='checked_subset') !== (report.model!==null)) throw new Error('Unexpected syntax contract: '+JSON.stringify(report));
  if(code && !report.diagnostics.some(d=>d.code===code)) throw new Error('Missing actual syntax diagnostic');
  if(status==='checked_subset' && (report.model.models.length!==2 || report.diagnostics.length!==0)) throw new Error('Expected both checked models');
  if(text===syntaxDocument) {
    const declaration = name => report.model.declarations.find(d => d.qualifiedName === name);
    const angle = declaration('A.Angle').effectiveType;
    const oid = declaration('B.T.C').oidRules.object;
    if (!angle.circular || angle.unit.targetName !== 'INTERLIS.rad' || oid.domain.targetName !== 'INTERLIS.I32OID') throw new Error('Lost a bound unit or inherited OID rule');
    const calendar = declaration('A.Calendar').effectiveType;
    if (calendar.kind !== 'temporal_range' || calendar.base.targetName !== 'INTERLIS.XMLDate' || calendar.lower.components.join('-') !== '2005-2-3') throw new Error('Lost the bound date interval');
    const constraint = declaration('B.T.C').constraints[0];
    const expression = report.model.expressions[constraint.expression];
    const argument = report.model.expressions[expression.arguments[0]];
    if (constraint.name !== 'requiresActive' || expression.valueType !== 'boolean' || expression.function.targetName !== 'B.gate' || argument.steps[0].reference.targetName !== 'B.T.C.active') throw new Error('Lost the bound constraint expression');
    const model = report.model.models[1];
    if (!model.noIncrementalTransfer || model.charset !== 'UTF-8' || model.declaredCharset !== 'csUTF8' || model.effectiveXmlNamespace !== 'http://www.interlis.ch/xtf/2.4/B') throw new Error('Lost model options');
  }
  if (text === classValues) {
    const value = report.model.declarations.find(d => d.qualifiedName === 'M.D').effectiveType;
    const target = value.restrictions[0];
    if (value.kind !== 'class_value' || value.valueKind !== 'class' || target.predefinedLanguage !== '2.3' || text.slice(target.startByte, target.endByte) !== 'INTERLIS.METAOBJECT') throw new Error('Lost the class-value restriction');
  }
  if (text === population) {
    const { declarations, expressions } = report.model;
    const constraint = declarations.find(d => d.qualifiedName === 'M.T.C').constraints[0];
    const count = expressions[expressions[constraint.expression].left];
    const objects = expressions[count.arguments[0]];
    const target = objects.effectiveType.targets[0].target;
    if (!constraint.basket || expressions[constraint.filter].steps[0].reference.targetName !== 'M.T.C.flag' || objects.explicit || target.targetName !== 'M.T.C' || text.slice(target.startByte, target.endByte) !== 'ALL') throw new Error('Lost the bound SET filter or population');
  }
  if(text.includes('X=;')) {
    const errors=report.diagnostics.filter(d=>d.code==='ILI-SYNTAX');
    if(errors.length!==2 || errors[0].startByte!==55 || errors[0].startColumnUtf16!==54 || errors[1].startByte!==59 || errors[1].startColumnUtf16!==58) throw new Error('Lost Unicode syntax ranges: '+JSON.stringify(errors));
  }
}

function pair(version) {
  return {sources:[
    {name:'memory:///a-ä😀.ili',text:`INTERLIS ${version}; MODEL Base (en) AT "https://example.invalid" VERSION "v1" = DOMAIN Label=TEXT*40; Count=9007199254740992..9007199254740993; Scaled=0.10e-2..0.99e+2; END Base.`},
    {name:'memory:///z-model.ili',text:`\uFEFF!! 😀\r\nINTERLIS ${version};\r\nMODEL Data AT "https://example.invalid" VERSION "v2😀" =\r\n IMPORTS Base; DOMAIN Required EXTENDS Base.Label=MANDATORY;\r\n TOPIC Items=CLASS Item=label:Required; count:Base.Count; END Item; END Items; END Data.`},
  ]};
}
const valid23=pair('2.3'),valid24=pair('2.4');
const selected=pair('2.4');
selected.roots=['Data'];
selected.sources[0].text=selected.sources[0].text.replace('2.4','2.3');
selected.sources.push({name:'unselected',text:'INTERLIS 2.4; MODEL Other AT "x" VERSION "1" = IMPORTS Missing; END Other.'});
const missing=pair('2.4');missing.sources.shift();
const wrong=pair('2.4');wrong.sources[0].text=wrong.sources[0].text.replaceAll('Base','Other');
const prefix=pair('2.4');prefix.sources[1].text=prefix.sources[1].text.replace('Base.Count','Wrong.Base.Count');
const unsupported=pair('2.4');unsupported.sources[1].text=unsupported.sources[1].text.replace('END Item;','SET CONSTRAINT INTERLIS.areAreas(ALL, UNDEFINED, >>shape); END Item;');
const numericLimit=pair('2.4');numericLimit.limits={max_numeric_digits:4};
const syntaxLimit=pair('2.4');syntaxLimit.limits={max_syntax_nodes:5};
const workLimit=pair('2.4');workLimit.limits={max_parser_steps:1};
const deepEnumeration=pair('2.4');deepEnumeration.sources=[{name:'memory:///deep-type.ili',text:'INTERLIS 2.4; MODEL M AT "x" VERSION "1" = DOMAIN E='+('(a'.repeat(24))+'leaf'+(')'.repeat(24))+'; END M.'}];deepEnumeration.limits={max_nesting:8};
const flatExpression = Array(20000).fill('a').join(' AND ');
const expressionRequest = (tail = '', limits) => JSON.stringify({sources:[{name:'memory:///expression-😀.ili',text:'INTERLIS 2.4; MODEL M AT "x" VERSION "1" = TOPIC T=CLASS C=SET CONSTRAINT '+flatExpression+tail+'; END C; END T; END M.'}],...(limits?{limits}:{})});
export const semanticCases = [
  [expressionRequest(),'rejected','ILI-SET-OBJECT-VALUE'],
  [expressionRequest(' AND'),'rejected','ILI-SYNTAX'],
  [expressionRequest('',{max_syntax_nodes:100}),'limit_exceeded','ILI-LIMIT'],
  [JSON.stringify(valid24),'checked_subset','2.4'],
  [JSON.stringify(deepEnumeration),'limit_exceeded','ILI-LIMIT'],
  [JSON.stringify(valid23),'checked_subset','2.3'],
  [JSON.stringify(selected),'checked_subset','2.3/2.4'],
  [JSON.stringify(valid24),'checked_subset','2.4'],
  [JSON.stringify(missing),'rejected','ILI-IMPORT-NOT-FOUND'],
  [JSON.stringify(wrong),'rejected','ILI-IMPORT-NOT-FOUND'],
  [JSON.stringify(prefix),'rejected','ILI-TYPE-NOT-FOUND'],
  [JSON.stringify(unsupported),'incomplete','ILI-UNSUPPORTED'],
  [JSON.stringify(numericLimit),'limit_exceeded','ILI-LIMIT'],
  [JSON.stringify(syntaxLimit),'limit_exceeded','ILI-LIMIT'],
  [JSON.stringify(workLimit),'limit_exceeded','ILI-LIMIT'],
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
  const versions=detail.includes('/')?detail.split('/'):[detail,detail];
  const graph=report.model;if(!graph || report.diagnostics.length!==0 || graph.models.length!==2 || graph.models[0].name!=='Base' || graph.models[1].name!=='Data' || graph.models[1].imports.join(',')!=='Base' || graph.models.some((m,i)=>m.languageVersion!==versions[i])) throw new Error('Incorrect bound model headers');
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
