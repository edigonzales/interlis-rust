# Development rules

- Target INTERLIS 2.3 and 2.4. Native and WASM share the language implementation.
- The core receives explicit unchanged sources. Filesystem, network, CLI and JS are host boundaries.
- Model checking is separate from transfer-object validation and geometry topology.
- Never report incomplete language support as full validation or silently skip constructs.
- Prefer concrete structs/enums and a few clear state owners. Avoid speculative frameworks, sessions and compatibility layers.
- Use Cargo and normal test discovery, direct Rust/JS tests and `.ili` fixtures. No local test registries or norm-coverage inventories.
- Test core semantics broadly natively; run a few actual Node/WASM/browser-worker cases and targeted backend regressions.
- Verify changes with relevant tests and independent counterprobes. Preserve meaningful assertions.
- Keep assignment packages and completed agent reports outside the product repository.
- Preserve foreign changes. Push, merge and publication require a specific user instruction.
