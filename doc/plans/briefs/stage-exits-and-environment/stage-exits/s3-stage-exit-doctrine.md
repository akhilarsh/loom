# S3: BLOCK-F, and every surface that must agree with it

Stage `stage-exits`, tier sonnet. Read `../common.md` first. Stage `completion-gates` has merged:
`loom stage dispute-criteria --field acceptance|wiring|wiring-tests` exists, and
`loom stage complete` labels failures `[criterion n]`, `[wiring n]` and `[wiring_tests n]`.

You own `loom/src/orchestrator/signals/{helpers.rs, tests_doctrine_v2.rs,
tests_doctrine_blocks.rs, tests_size.rs, v2_section_review.rs, format/sandbox_section.rs,
format/helpers.rs}`, `loom/src/commands/stage/{acceptance_runner.rs, complete_verification.rs,
dispute_transport.rs, dispute_criteria_tests.rs}`, `CLAUDE.md.template`,
`loom-hooks/commit-guard.sh`, `skills/loom-orchestration/SKILL.md` and
`skills/loom-usage/SKILL.md`.

## Why

Stage agents avoided disputes ("filing it ends the session"), reverted correct work to dodge one,
and ended turns asking the operator to act. Today's surfaces frame a dispute as a cost, tell an
agent to "STOP and report it as a blocker" without naming a command, and never mention
`loom stage block`.

## BLOCK-F, verbatim

Every copy is these exact bytes (one line, no line breaks inside):

    **When the stage cannot finish.** Fix what the stage can fix. A criterion, wiring check, contract, review finding or test-integrity event that is wrong gets a dispute: `loom stage dispute-criteria` (with `--field` for wiring and wiring-tests entries), `dispute-contract`, `dispute-findings` or `dispute-integrity`. Commit your work first. Filing ends this session by design: the daemon starts a fresh session with the verdict, so waiting gains nothing. Never revert, weaken or postpone correct work to avoid a dispute. A need only a person can meet (a credential, a host install, a network domain or path the plan does not grant) gets `loom stage block <stage-id> "<what is needed and why>"`: the daemon retires the session and shows the reason to the operator. Never end a turn asking the operator to act while the stage is executing.

## 1. The stage signal

- `signals/helpers.rs`: a `const STAGE_EXIT_RULES: &str` holding BLOCK-F, pushed by
  `append_completion_rules` (about lines 165-170) after its bullet list, as its own paragraph
  followed by a blank line. `append_completion_rules` serves the standard, integration-verify and
  knowledge-distill prefixes (`cache.rs`); `cache.rs` is ledgered at 524 lines and is not
  edited. The knowledge prefix uses only `append_settled_completion_rules` and does not get it.
- `signals/tests_size.rs`: `generate_stable_prefix()` is 6,720 bytes and BLOCK-F adds about
  840, over the 7,168-byte ceiling. Raise `STABLE_PREFIX_MAX_BYTES` to `8_192` and extend its
  doc comment the way the BLOCK-D raise is documented: raised alongside BLOCK-F (stage exits),
  6,720 bytes before, the new actual after. The const line is not an assertion line.
- `signals/tests_doctrine_v2.rs`: add `const BLOCK_F` (the text above) and the test
  `block_f_agrees_across_every_surface` (exact name; acceptance runs it): the standard prefix
  (`super::cache::generate_stable_prefix()`), the integration-verify prefix
  (`super::cache::generate_integration_verify_stable_prefix()`) and `ORCHESTRATION_SKILL` each
  `contains(BLOCK_F)`, with a failure message like BLOCK-E's. Update the module doc comment.
- `signals/v2_section_review.rs::append_dispute_commands` (about lines 57-72): keep the
  batching advice (every dispute of one review round in one `dispute-findings` command) and
  replace "filing a dispute ends your turn and sends the stage to adjudication; loom retires
  your session when the verdict is applied, or when the dispute escalates to human review" with
  "each filing ends this session by design, and the daemon starts a fresh one with the
  verdict".
- `signals/format/sandbox_section.rs::format_missing_grants_note` (about lines 88-104): replace
  "STOP and report it as a blocker. The operator must create the path on the host and restart
  this stage's session." with "block the stage with `loom stage block <stage-id> \"<path> is
  missing on the host\"`: the operator creates the path and retries the stage." Change the test
  assertion at about line 134 to `assert!(content.contains("loom stage block"));` (this file's
  inline tests are not a test file to the integrity gate). `format_sandbox_section` is ledgered
  at 58 lines and is not touched.
- `signals/format/helpers.rs::append_package_cache_note` (about line 247): keep the
  `**Package-manager caches:**` opening (`tests_commit_timing.rs` matches it) and replace "STOP
  and report it as a blocker (it needs a plan-level `sandbox.filesystem.allow_write` entry); do
  not work around it." with "block the stage with `loom stage block <stage-id> \"<cache path>
  needs a plan-level sandbox.filesystem.allow_write entry\"`; do not work around it."
- `signals/tests_doctrine_blocks.rs::RETIRED_PHRASES`: add, split with `concat!` as the others
  are, with a comment naming BLOCK-F: `"report a needed sandbox block as a blocker"`,
  `"STOP and report it as a blocker"`, and `"ends your turn and sends the stage to adjudication;
  loom retires"`. Confirm BLOCK-F itself contains no retired phrase.

## 2. `loom stage complete`'s failure guidance

- `commands/stage/acceptance_runner.rs::print_acceptance_failure_guidance` (about lines
  211-231): keep "Fix the issues and run ..." first. Then: a check no correct implementation
  could pass gets a dispute, never a weakened fix, with the three forms

      loom stage dispute-criteria <id> --criterion-index <n> --reason "<why>"                       ([criterion n])
      loom stage dispute-criteria <id> --field wiring --criterion-index <n> --reason "<why>"        ([wiring n])
      loom stage dispute-criteria <id> --field wiring-tests --criterion-index <n> --reason "<why>"  ([wiring_tests n])

  (with the real stage id interpolated); commit first, and filing ends the session by design;
  a failure only a person can fix (a credential, a host install, a network domain or path the
  plan does not grant) gets `loom stage block <id> "<what is needed and why>"`. Keep the
  `--failure-output`, adjudicator and `--no-verify` sentences. `resolve_acceptance_dir` is
  ledgered (60 lines) and not touched; keep the file under 400 lines. Add a unit test that the
  guidance names `--field wiring-tests` and `loom stage block` (capture it by factoring the
  text into a `fn failure_guidance(stage_id) -> String` that the print function writes).
- `commands/stage/complete_verification.rs::run_goal_checks` (about lines 57-84): before the
  final `bail!`, call `crate::commands::stage::acceptance_runner::print_acceptance_failure_guidance(checks.stage_id);`
  so a wiring or wiring-test failure shows the same routes.

## 3. A relayed dispute ends the turn

`commands/stage/dispute_transport.rs::send_via_relay` passes `false` as `end_turn` to
`context.emit`, so a sandboxed agent's relayed dispute notice never says "End your turn after the
confirmation." (a relayed block's does: `commands/stage/state_relay.rs`). Pass `true`. Test in
`dispute_criteria_tests.rs`: `relay_mode_dispute_ends_the_turn` (mirror
`state_relay.rs::relay_mode_writes_exactly_one_ticket_with_the_end_turn_reminder`).

## 4. Text surfaces

- `CLAUDE.md.template` Rule 13 (about line 181): replace "report a needed sandbox block as a
  blocker" with a sentence that in a stage, a need only a person can meet gets `loom stage block
  <stage-id> "<what is needed and why>"`, and a wrong check gets a dispute. The template is
  18,734 bytes against a 20,480 cap (`tests_size.rs`); add at most about 200 bytes.
- `loom-hooks/commit-guard.sh` (about line 629): "Do NOT end this session until all steps are
  complete." becomes "Do NOT end this session until all steps are complete, or a dispute or
  block is filed." Keep "LOOM WORKTREE COMPLETION CHECKLIST" (tests pin it).
- `skills/loom-orchestration/SKILL.md`: in the review loop (about lines 263-267) keep the
  batching advice and the 3-per-kind budget, and replace "Filing a dispute ends your turn and
  sends the stage to adjudication; loom retires your session when the verdict is applied, or
  when the dispute escalates to human review." with "Filing ends this session by design; the
  daemon starts a fresh session with the verdict." Add BLOCK-F verbatim as its own paragraph
  right after the "Review order (plan v2)" paragraph.
- `skills/loom-usage/SKILL.md` "Force Complete" (about lines 371-379): replace
  `# Request human review (preferred)` and the wrong
  `loom stage dispute-criteria <stage-id> "criteria X is incorrect because..."` with the real
  syntax and a comment that it routes to the adjudicator:

      # Dispute a wrong check; routes to the adjudicator, which can amend it
      loom stage dispute-criteria <stage-id> --criterion-index <n> --reason "<why it is wrong>"
      loom stage dispute-criteria <stage-id> --field wiring --criterion-index <n> --reason "<why>"

  and add a line for `loom stage block <stage-id> "<what is needed and why>"`. Keep the
  `--force-unsafe --assume-merged` lines.

## Check

`cargo test --lib orchestrator::signals::tests_doctrine`, once.
