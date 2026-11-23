---
name: simplify-change
description: Reduce unnecessary complexity in interlis-rust designs, diffs, crate layouts, build logic and tests. Use when a change adds a structural boundary, abstraction or verification layer.
---

Compare the proposal or diff with the shortest implementation a maintainer can
follow from a real entry point. Find the current callers and constraints before
deciding which boundaries belong in the design.

- Trace data and ownership. Look for one clear owner, direct calls and existing
  concrete Rust types. Keep a crate, layer, trait or adapter when it serves a
  real caller or isolates a concrete boundary such as host I/O.
- Inspect the build and tests. Prefer Cargo and Node discovery, direct product
  assertions, and the original upstream runner where applicable. Remove test
  inventories, redundant execution and evidence-processing layers when they no
  longer serve a concrete need.
- For each extra layer, checksum or fingerprint, name the failure it prevents
  and check whether the existing tool already prevents it. Prefer removing the
  extra mechanism when its only output is proof that another check ran.

Preserve requested behaviour and regression assertions. In a review, describe
the removable piece, its actual caller, and the simpler option. Leave product
files untouched for review-only requests.
