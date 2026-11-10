# interlis-rust

Experimental Rust foundation for an INTERLIS 2.3/2.4 compiler, shared by native
and WebAssembly hosts. **Language analysis is not implemented yet.** Model
requests return `incomplete`, `validationScope: "i0-subset"`,
`fullValidation: false`, and `model: null`. This is not a full model validator.

The library takes owned, explicit UTF-8 sources and never reads files or accesses
the network. Logical source names are exact identifiers (1–1024 UTF-8 bytes,
no control characters), with 1–4096 sources per request. Text is not normalized.
Ranges use half-open UTF-8 bytes; positions use one-based lines and UTF-16 columns.
A leading BOM is invisible in columns. CRLF counts as one newline.

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

Requests have `{"sources":[{"name":"memory:///a.ili","text":"INTERLIS 2.3;"}]}`;
optional `limits` fields use the snake_case Rust field names. Unknown and duplicate
JSON fields are rejected. JSON input is limited to 16 MiB. The CLI reads at most
32 MiB of source files; duplicate physical paths are usage errors. Source paths
must be valid Unicode and file contents must be strict UTF-8.

Default budgets: 2 MiB/source, 8 MiB total, 250,000 tokens, depth 128,
100 diagnostics, 4,096 numeric digits, absolute exponent 10,000, 1,000,000 semantic
steps. Limits must be positive. Hard caps respectively: 8 MiB, 32 MiB, 1,000,000,
256, 1,000, 16,384, 100,000, 10,000,000. Only source budgets are consumed by the
current foundation; later language processing will consume the other budgets.
Limits are operational constraints, not language restrictions.

CLI exits: 0 checked subset, 1 rejected, 2 invalid request/usage, 3 operational
failure, 4 incomplete or limit exceeded. Currently only incomplete and source
limit reports are produced. JSON stdout contains one report; host errors go to
stderr. No implicit repository lookup exists.

```sh
wasm-pack build --target nodejs --out-dir target/pkg-node --out-name interlis --no-opt --no-default-features --features wasm --locked
wasm-pack build --target web --out-dir target/pkg-web --out-name interlis --no-opt --no-default-features --features wasm --locked
cd tools/wasm-smoke
npm ci
npx playwright install chromium
npm run test:node
npm run test:browser
```

Generated packages export `compilerVersion()` and `analyzeJson(string)`. Invalid
requests (including unpaired JavaScript surrogates) throw controlled boundary
errors. Analysis outcomes are normal JSON reports. The browser test loads the
real web package in a module worker over a local HTTP server; it has no UI.
Tests require built native and WASM artifacts; missing browsers/packages fail.
