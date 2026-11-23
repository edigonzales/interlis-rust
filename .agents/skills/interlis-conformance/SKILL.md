---
name: interlis-conformance
description: Resolve INTERLIS 2.3 or 2.4 model-semantics questions and run the pinned Codeberg compiler suite for interlis-rust phase acceptance.
---

Read the full rule in the applicable [INTERLIS reference manual](https://www.interlis.ch/dokumentation/interlis-2).
Compare with `docs/contracts/norm-decisions.md` and the related implementation,
tests and commit messages in [interlis-java](https://github.com/edigonzales/interlis-java).
Use a small valid and invalid `.ili` example to distinguish a compiler bug from
a deliberate compatibility decision. Keep the approved interpretation in the
direct Rust regression test or relevant product documentation.

At compiler phase boundaries, use the original [Codeberg runner](https://codeberg.org/edigonzales/interlis-compiler-testsuite)
at revision `88576ffc4cd491dd72d2375eb897ea9f3f55d2c3` with the unchanged ili2c
fixtures at `4ea207f388ef7d91b03d968cc1636f4de84e36ea`. Use external runner
configuration and reports. Run `probe --json` until the strict `compile` entry
point is available. Use the runner's `{files}` argument and supply any required
dependency sources explicitly. Select the 2.3 and 2.4 cases by actual header when
the catalogue and fixture disagree.

Read the native JSON report for inconclusive runner results. A `checked_subset`
result is intermediate. Incomplete analysis, missing test inputs, timeouts,
limits and internal errors give no evidence of a correct language rejection.
Compare outcomes with the previous phase. Verify the intended diagnostic for
negative cases and schema values for positive cases with direct Cargo tests.
Record the revisions, command and the remaining differences alongside the
runner's own reports in the external work package.
