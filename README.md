# interlis-rust

Experimental Rust foundation for an INTERLIS 2.3/2.4 compiler, shared by native
and WebAssembly hosts. A successful result is **`checked_subset`**, with
`validationScope: "i0-subset"` and `fullValidation: false`. It contains a read-only
provisional model graph. It is not a full INTERLIS validator.

The current subset includes simple data-model headers, ordinary explicit imports,
topics, classes within topics, structures, bounded TEXT/BOOLEAN/numeric domains
and ordered attributes. It checks model identities, missing imports, duplicate
names in supported scopes, closing names, supported domain binding/cycles,
mandatory flags, positive text lengths and exact numeric bounds/written precision.
Domain dependencies use `Alias EXTENDS Base = MANDATORY;`; direct domain
`Alias = Base;` definitions are invalid. Mandatory domains contribute to the
effective requirement of attributes; their declared references remain available.

General inheritance, structure-valued attributes, units, directions/reference
systems, views, constraints and other omitted constructs produce `incomplete`.
Broader namespace/visibility and declaration-order rules, issuer URI validation
and full FLOAT transfer semantics are outside this subset check. Metadata is
preserved without claiming those rules are validated. There is no transfer-object,
geometry or database validation. Mixed 2.3/2.4 input sets are explicitly incomplete.
Every supplied source is analyzed; an I-0 source without a model produces a normal
`ILI-MODEL-MISSING` report, not a malformed-request error.

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

Rust 1.99.1 is pinned. Local package tests use Node 22.13.0 and wasm-pack 0.15.1.
Build dependencies may be downloaded; analysis never downloads models or issuers.

```sh
cargo test --locked
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked
cargo run -- version
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
100 diagnostics, 4,096 numeric digits, absolute exponent 10,000, 1,000,000 semantic
work units. Limits must be positive. Hard caps respectively: 8 MiB, 32 MiB,
1,000,000, 256, 1,000, 16,384, 100,000, 10,000,000. Numeric digits include exponent
digits. Numeric/exponent budgets are checked before bigint allocation/alignment.
Semantic work counts declaration/binding steps, generated name/reference bytes,
numeric digits and decimal alignment distance. Depth counts active blocks,
including the outer comment or first topic/class/structure; the model header is
not a nested container. Limits are operating constraints, not language restrictions.
Unsupported diagnostics are warnings; resource-limit errors mean analysis could
not finish, without a language-invalidity verdict.

CLI exits: 0 checked subset, 1 rejected, 2 invalid request/usage, 3 operational
failure, 4 incomplete or limit exceeded. JSON stdout contains one report; host
errors go to stderr. No implicit repository lookup exists.

```sh
wasm-pack build --target nodejs --out-dir target/pkg-node --out-name interlis --no-opt --no-default-features --features wasm --locked
wasm-pack build --target web --out-dir target/pkg-web --out-name interlis --no-opt --no-default-features --features wasm --locked
cd tools/wasm-smoke
npm ci --ignore-scripts
npx playwright install chromium
npm run test:node
npm run test:browser
```

Generated packages export `compilerVersion()` and `analyzeJson(string)`. Malformed
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
and targeted ili2c comparisons. The `.g4` files are references, not product build
inputs; the geostandards grammar targets 2.4. This subset implementation does not
establish a parser-generator choice for the complete language.
