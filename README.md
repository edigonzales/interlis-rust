# interlis-rust

INTERLIS 2.3 and 2.4 syntax parsing and experimental model analysis, shared by
native and WebAssembly hosts. `parse(CompileRequest)` returns an owned syntax
report. `analyze(CompileRequest)` checks the implemented model subset and returns
`checked_subset` with a read-only provisional model graph on success.

The syntax report uses `validationScope: "syntax"` and `fullValidation: false`.
Its statuses in descending priority are `limit_exceeded`, `invalid`,
`unsupported_version`, and `parsed`. JSON includes document and model summaries
only for `parsed`. A header-only file has an empty `models` list. A model summary
contains `name`, `kind`, `version`, and original source positions. The `kind`
values are `data`, `type`, `refsystem`, and `symbology`. Each document uses the
language version in its own header. Name and import binding belong to model analysis.

The current subset includes simple data-model headers, ordinary explicit imports,
topics, classes within topics, structures, scalar text, Boolean, alignment,
date/time, blackbox and bounded numeric types, and ordered attributes. It checks
model identities, missing imports, duplicate
names in supported scopes, closing names, supported domain binding/cycles,
mandatory flags, positive text lengths and exact numeric bounds/written precision.
Domain dependencies use `Alias EXTENDS Base = MANDATORY;`; direct domain
`Alias = Base;` definitions are invalid. Mandatory domains contribute to the
effective requirement of attributes; their declared references remain available.

The private syntax tree also preserves model kinds, contracted/transfer options,
explanations, translation headers, charset/XML namespace declarations and ordered
qualified/unqualified imports. It represents topic inheritance/dependencies/OIDs,
2.4 deferred generics and contexts, container properties/inheritance/OIDs, attribute
properties/subdivisions, parameter sections and associations with ordered roles,
restricted target alternatives and exact cardinalities. Anonymous associations
retain absent names. Mandatory-only attribute/parameter uses retain an absent
concrete type. Class references preserve the reserved predefined classes in their
allowed contexts. These constructs pass through the same bounded frontend;
the analyzer explicitly returns `incomplete` before binding their unimplemented
semantics. An optional ATTRIBUTE section marker is semantically inert.

Type syntax covers text families, alignments, 2.4 temporal values, nested
and dotted enumerations, numeric units/directions/reference systems, formatted
parts and bounds, coordinates/rotation/CRS, geometry variants and line options,
OID carriers, blackboxes, class and attribute-path values, references and
version-specific BAG/LIST elements. Units retain ordered factors/operators,
symbolic PI/LNBASE and descriptive FUNCTION explanations; no conversion runs.
Line-form declarations and domain properties/inheritance/mandatory-only forms
remain structured. Model binding, type compatibility, coordinate interpretation,
unit evaluation and geometry topology are separate.

Expression syntax preserves constants, paths, calls, object sets, inspections,
runtime parameters and edition-specific operator precedence in a document-local
arena. Function declarations, domain/attribute/role derivations, all five
constraint kinds and external constraint blocks are structured syntax. Long flat
chains use indices, including during error recovery and destruction. View
formations, filters, attributes, metadata basket groups and graphic assignments
are structured syntax. Expressions, constraints and graphic rules are not evaluated.

General inheritance, structure-valued attributes, units, directions/reference
systems, views, constraints and other omitted constructs produce `incomplete`.
Broader namespace/visibility and declaration-order rules, issuer URI validation
and full FLOAT transfer semantics are outside this subset check. Metadata is
preserved without claiming those rules are validated. There is no transfer-object,
geometry or database validation. The subset supports mixed 2.3/2.4 model sources.
The compiler binds imports by declared model name, detects import cycles, and
uses the referring model's edition for references to predefined domains.
`IMPORTS UNQUALIFIED` exposes direct model-level types after local scope lookup.
An INTERLIS header alone is a valid document with no model declarations.
`TRANSLATION OF` adds the original model at the exact stated version to the
selected closure. Its name space is available through a separate `IMPORTS`
declaration. Models may import a later declaration in the same source.
Direct declaration and visibility errors can be diagnosed in an otherwise
incomplete model. References which need effective inherited names are deferred
to the corresponding semantic checks.

The library takes owned, explicit UTF-8 sources and never reads files or accesses
the network. Logical source names are exact identifiers (1–1024 UTF-8 bytes,
no control characters), with 1–4096 sources per request. Text is not normalized.
Ranges use half-open UTF-8 bytes; positions use one-based lines and UTF-16 columns.
A leading BOM is invisible in columns. CRLF counts as one newline.

`analyze(CompileRequest)` returns an owned report. `graph()` exists only for
`CheckedSubset`; its `models`, `declarations`, exact qualified-name lookup,
`members` and `effective_type` methods need no parser access or public arena IDs.
Members preserve declaration order; an existing empty scope returns `Some(empty)`.
Other outcomes expose diagnostics and original sources, with no public partial
graph. Numeric bounds retain original spelling, exact decimal coefficient,
fractional digits and written exponent. JSON numeric bounds and text lengths are
strings, including values beyond JavaScript's safe integer range.

`analyze_roots(request, roots)` checks the named models and their transitive
imports. All supplied documents must parse successfully. Semantic checks run
only on the selected model closure. Pass `roots` as a nonempty array of distinct
model names to `analyzeJson` or `probe --request`, or repeat `--root MODEL` in
`probe` file mode. Omit the selection to analyze all supplied models.

Rust 1.99.1 is pinned. Local package tests use Node 22.13.0 and wasm-pack 0.15.1.
Build dependencies may be downloaded; analysis never downloads models or issuers.

```sh
cargo test --locked
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked
cargo run -- version
cargo run -- parse --json model.ili
cargo run -- parse --request request.json --json
cargo run -- probe --json model.ili
cargo run -- probe --request request.json --json
```

Requests have `{"sources":[{"name":"memory:///a.ili","text":"INTERLIS 2.3; MODEL A AT \"https://example.invalid\" VERSION \"1\" = END A."}]}`;
optional `limits` fields use the snake_case Rust field names. Unknown and duplicate
JSON fields are rejected. JSON input is limited to 16 MiB; encoded reports to
32 MiB. Report-size failure is a controlled `REPORT_WIRE_SIZE` boundary error;
no partial JSON is returned. The native Rust graph remains readable if encoding
fails. The CLI reads at most 32 MiB of source files. Repeated physical files,
including symlinks and hardlinks, are usage errors; equal contents in distinct
files remain separate sources. Native file identities use `same-file`, confined
to the native CLI feature. Identity handles stay open for the read set; OS handle
exhaustion is an operational error. Source paths must be valid Unicode and
contents strict UTF-8.

File-mode locations use `cli:///<source-number>/<percent-encoded-basename>`.
Source numbers start at one and follow sorted canonical paths, so reordering the
same file set preserves names and diagnostic order. Equal basenames remain
distinct. Very long labels end with `~`, fitting the 1024-byte logical-name limit
without splitting an encoded Unicode character; source numbers retain uniqueness.
Absolute and relative spellings produce the same logical name. Local directory
paths do not enter the analysis JSON. Request mode
preserves the caller's exact logical names. Plain diagnostics show logical source,
line and UTF-16 column; file I/O errors go to stderr.

Default budgets: 2 MiB/source, 8 MiB total, 250,000 tokens, depth 128,
500,000 syntax values, 10,000,000 frontend work units,
100 diagnostics, 4,096 numeric digits, absolute exponent 10,000, 1,000,000 semantic
work units. Limits must be positive. Hard caps respectively: 8 MiB, 32 MiB,
1,000,000, 256, 2,000,000, 100,000,000, 1,000, 16,384, 100,000, 10,000,000. Numeric digits include exponent
digits. Numeric/exponent budgets are checked before bigint allocation/alignment.
Semantic work counts syntax-support checks, declaration/binding steps, generated name/reference bytes,
numeric digits and decimal alignment distance. Depth counts active blocks and recursive types/enumerations,
including the outer comment or first topic/class/structure; the model header is
not a nested container. Limits are operating constraints, not language restrictions.
Frontend work counts original lexer bytes, grammar entries, loop/recovery steps,
consumed parser tokens and syntax allocations. Charges precede the corresponding
work. Syntax values count documents, declarations, type/name/literal values and
preserved comments, including values later discarded by error recovery; vectors
and spans are not extra nodes. Limits are shared across the whole request.
Identifiers are limited to 255 ASCII characters. Strings use INTERLIS escapes;
comments and explanations retain their original source ranges. Missing or damaged
language headers are syntax errors; other well-formed versions remain incomplete.
Version-Dec is recognized by exact value (for example `+2.3`, `2.30`, `0.23E1`);
the original spelling and all source ranges remain unchanged. Proven syntax errors
take precedence over incomplete analysis; resource limits take precedence over both.
Unsupported diagnostics are warnings; resource-limit errors mean analysis could
not finish, without a language-invalidity verdict.

CLI success exits 0 for `parsed` or `checked_subset`. Exit 1 means invalid syntax
or a rejected model, 2 an invalid request or usage, 3 an operational failure, and
4 an unsupported version, incomplete analysis, or exceeded limit. JSON stdout
contains one report. Text diagnostics and host errors go to stderr. All model
files must be provided explicitly.

```sh
wasm-pack build --target nodejs --out-dir target/pkg-node --out-name interlis --no-opt --no-default-features --features wasm --locked
wasm-pack build --target web --out-dir target/pkg-web --out-name interlis --no-opt --no-default-features --features wasm --locked
cd tools/wasm-smoke
npm ci --ignore-scripts
npx playwright install chromium
npm run test:node
npm run test:browser
```

Generated packages export `compilerVersion()`, `parseJson(string)`, and
`analyzeJson(string)`. Malformed
requests (including unpaired JavaScript surrogates) throw controlled boundary
errors. Language outcomes are normal JSON reports. The browser test loads the
real web package in a module worker over a local HTTP server; it has no UI.
Smokes check actual model/type/origin data, big numbers, error recovery and native
parity. Missing browsers/packages fail. Broad language tests run natively.

CI defines native Linux/macOS/Windows checks and one Linux Node/Chromium-worker
job. These hosted jobs have not been run for this candidate; local verification
currently covers macOS, Node and Chromium. There are no release or publishing jobs.

The core uses one Logos lexer and a private Recursive Descent parser, followed by
scope/type analysis and an owned model graph. CLI and WASM share the same request
decoder and report projection. Syntax follows the original 2.3/2.4 reference
manuals, cross-checked against the
[geostandards ANTLR4 grammar](https://github.com/geostandards-ch/interlis-antlr4/tree/f8f97238c2540a49915f8bb97ee99bbf434b74da)
and targeted ili2c comparisons. The `.g4` files serve as grammar references. The
geostandards grammar targets 2.4.
