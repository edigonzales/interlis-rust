# Development rules

Read the affected code, callers and tests. Check the branch and working tree
before editing. Preserve other contributors' changes. Push, merge and publication
require a specific user instruction.

## Design and repository structure

- Choose the smallest direct implementation that meets the current requirement.
  Reuse existing code and standard Cargo, Git and library features. Prefer
  concrete structs and enums with clear ownership.
- Justify each new crate, module boundary, trait or layer with an actual caller
  or constraint. Avoid trivial wrappers, speculative extension points, parallel
  implementations and compatibility layers on speculation. Remove replaced paths
  and their callers together.
- Keep assignment packages, run reports and completed reviews outside the product
  repository. Keep the normal build reproducible from a checkout.
- Keep test files, fixtures and test reports free of fingerprints and checksum
  inventories. For reproducibility record the code and upstream revisions, the
  relevant tool versions, commands and ordinary results. Add a hash only for a
  concrete integrity, identity or interoperability requirement that existing
  mechanisms do not cover. State the requirement next to the implementation.
- Use [simplify-change](.agents/skills/simplify-change/SKILL.md) for larger designs
  or structural changes.

## INTERLIS contracts

- Target INTERLIS 2.3 and 2.4. Native and Wasm share one language implementation.
- The core receives explicit unchanged sources. Filesystem, network, CLI and JS
  are host boundaries. Model checking is separate from transfer-object validation
  and geometry topology.
- Never report incomplete language support as full validation or silently skip
  constructs. For disputed rules, consult the INTERLIS reference manuals and
  `interlis-java`'s documented decisions and relevant commit messages. Use
  [interlis-conformance](.agents/skills/interlis-conformance/SKILL.md).

## Tests

- Use Cargo and Node test discovery. Test observable product behaviour directly
  with ordinary Rust or JS assertions and `.ili` fixtures. Avoid tests that audit
  other tests, inventory their fixtures, count evidence or validate report shape
  solely to certify a run. Avoid local test registries and norm coverage inventories.
- Test language rules broadly in Rust. Use a few real Node and browser-worker
  cases for the Wasm boundary and focused backend regressions. Run the smallest
  relevant checks while developing and the original Codeberg suite at the end of
  each compiler phase.
- Preserve meaningful assertions. Independent review should use targeted
  counterprobes and may rely on applicable successful runs for unchanged inputs
  and options. Record what actually ran and any remaining limits.
