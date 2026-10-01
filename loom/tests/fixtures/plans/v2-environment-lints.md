# PLAN: Version 2 Environment Lints Fixture

A `version: 2` plan that `loom plan verify` must reject with all three environment lints
when it is verified from this repository: the registry lint (the plan-level sandbox allows
only `crates.io`, and the standard stage runs `bunx`), the pre-commit hook lint (the
repository's hook runs `bunx`), and the JS provision lint (`web/` runs vitest and the plan
has no `provision` entry). The hook finding depends on this checkout's local
`core.hooksPath=loom/.githooks`; a clone without that setting reports no hook.

---

<!-- loom METADATA -->

```yaml
loom:
  version: 2
  sandbox:
    network:
      allowed_domains: ["crates.io"]
  stages:
    - id: knowledge-bootstrap
      name: "Knowledge bootstrap"
      stage_type: knowledge
      working_dir: "."
      dependencies: []
      summary: "Re-verify the knowledge topics."
      description: "Re-verify the knowledge topics the later stages are briefed from."
      acceptance:
        - "loom knowledge check --strict --baseline doc/loom/knowledge/check-baseline.txt"

    - id: add-greeting
      name: "Add a greeting"
      stage_type: standard
      working_dir: "."
      dependencies: ["knowledge-bootstrap"]
      summary: "Greet a user by name."
      description: "Greet a user by name."
      contracts:
        - id: greets-by-name
          file: "loom/tests/greeting.rs"
          test: "greets_by_name"
          runner: cargo-test
          scenario: "a greeting is built for the user named Ada"
          rejects: "a greeting that ignores the name and always says hello world"
      acceptance:
        - "cargo test --manifest-path loom/Cargo.toml --test greeting"
        - "bunx tsc --noEmit"

    - id: integration-verify
      name: "Integration verification"
      stage_type: integration-verify
      working_dir: "."
      dependencies: ["add-greeting"]
      summary: "Verify the merged tree."
      description: "Verify the merged tree."
      acceptance:
        - "cargo test --manifest-path loom/Cargo.toml --all-targets"

    - id: knowledge-distill
      name: "Knowledge distillation"
      stage_type: knowledge-distill
      working_dir: "."
      dependencies: ["integration-verify"]
      summary: "Curate the stage memories."
      description: "Curate the stage memories into knowledge."
      acceptance:
        - "loom knowledge check --strict --baseline doc/loom/knowledge/check-baseline.txt"
```

<!-- END loom METADATA -->
