---
type: design-review
context: same-instance-agent-interaction
status: passed
---
# Scoped Elaborate design review

## Authorization and boundary

The user explicitly selected **Authorize design review** for a new specification-only cycle with two independent reviewers. This scope does not review or reopen the previously landed implementation, reset its exhausted five-round record, authorize runtime implementation, waive a gate, or permit merge/release.

The separate design scope uses the standard dual-blind AND gate: both reviewers must independently report no must-fix findings and quality at least 94 percent. Maximum five rounds; never extend an exhausted cycle silently. No score/PASS is claimed until actual reports are received. The historical `verification.review_rounds: 5` and prior cap status remain unchanged.

## Design under review

- User requirements, scope and all 32 scenarios: `specs/planning-context.yaml`.
- Ownership and capability choices: ADRs 0001/0002; first Codex local target: ADR 0003.
- Normative wire contract, YAML-wrapped JSON Schema and examples: `specs/contracts/pty-control*`.
- Native UX: `specs/tech-architecture/pty-control-native-ux.md`.
- Evidence boundaries: feasibility assessment and both private spike reports.

The user's native-buffer deferral changes investigation order only: revisit at final complete-broker validation if unresolved. It is not grounds to demand another pre-review spike or to waive safe handoff. Production behavior is unimplemented and belongs to implementation/Verify acceptance; this is a design review, not a fabricated runtime certification.

## Parent self-audit

| Area | Result and evidence |
|---|---|
| Correctness | PASS after fixing a schema omission: a public settlement event without call identity was accepted before the fix and rejected after. Required event-specific claim/progress/mode/outcome fields now agree with the contract. All 80 shape fixtures passed: 46 valid messages and 34 rejected messages. |
| Security / user data | PASS for the scoped draft: credential-pattern scan found no exposed credentials; public event schemas prohibit private capabilities; native labels are terminal-control-sanitized; local coordination is explicitly not a same-user security sandbox. No auth/runtime code changed. |
| Performance | PASS as design constraints: cached authority, no per-key handle registration, nonblocking handoff primitives, and no waiting for observers/hooks under ownership state locks. Runtime performance is not measured or certified here. |
| Clarity / types | PASS for review readiness: roles, phase distinctions, target identities, safe/force/cancel/withdraw/release semantics, semantic agent actions, and implementation-target-versus-runtime-PASS are explicit. Fresh native-first startup remains an implementation validation case, not a claimed probe result. |
| Conventions / scope | PASS: repository deliverables stay in specs/; no source, dependency, default daemon, binary, generic plugin/relay, retry policy or Git integration change. Existing OMP/Zsh defaults and native geometry are preserved. |
| Provenance | PASS: decisions and existing runtime observations link to ADRs, normative contracts and actual spike records. No implementation plan or code claim is substituted for review evidence. |
| Code-only checklist | NOT APPLICABLE: no runtime functions, imports, type casts, dependencies or permanent code tests were added. Compiled-code style/SOLID/churn ranking and Cargo tests/lint/build do not validate this documentation-only draft. No bundled bigpowers scripts exist in this repository, so none were guessed or invoked. |

This is a parent self-audit only, not an independent quality score. No code-test, new UI smoke or production ownership PASS is implied.

## Review rubric and verification

Each reviewer checks 40 items: planning-context acceptance scenarios 01–32 as **design coverage**, plus (33) evidence/phase boundaries, (34) closed capability/wire roles, (35) dispatch/reservation race rules, (36) identity/privacy and recovery, (37) native UX/input routing, (38) local first-harness operation/lifecycle fit, (39) scope/performance/observer independence, and (40) artifact consistency and testability.

Categorize failed checklist items as must-fix or should-fix; a single item counts once at its highest severity. Quality is `100 × (40 − failed_items) / 40`, with zero must-fix additionally required. Suggestions alone do not fail an item. Findings must cite concrete artifact sections and consumer-visible impact, not demand out-of-scope production code during Elaborate.

Reviewers are read-only and receive identical independent briefs, not each other's findings. They skip build/lint/tests/formatters and validation execution mid-flight; the integration owner performs one combined artifact verification after the reports/fixes. Existing schema reproduction is documented in `specs/verifications/agent-interaction-feasibility.md`; the final owner check also covers YAML duplicate keys, local references, SVG XML and preserved phase/review state.

## Round 1 — changes required

| Independent reviewer | Quality | Must-fix checks | Result |
|---|---:|---|---|
| DesignReviewA | 92.5% | 19, 37, 40 | FAIL |
| DesignReviewB | 92.5% | 19, 37, 40 | FAIL |

Both complete reports were read before editing the frozen draft. A-01 and B-01 independently identify the same consumer gap: the external claim's configurable condition/countdown was available only through the requester's private claim, while the owning native attachment must draw the warning. Neither report contains a should-fix finding. Both explicitly preserved the native-buffer deferral and design-versus-production boundary.

**Disposition: accept and fix both findings together.** Conditional reservations now require public, non-authorizing condition parameters, eligibility, consent readiness/state and one authoritative clock. The owner reads them through the existing control/state/event paths; private claim/offer handles remain requester-only. Requester reads no longer define a competing top-level countdown. Consent captures displayed activity and native view revisions so an intervening reported interaction cannot arm an obsolete warning. Owner-view examples include a nondefault nine-second warning and a distinct all-views-backgrounded condition. Parent artifact verification and fresh dual re-review are still required; no PASS is inferred from the edits.

## Corrected draft verification before round 2

The parent ran the combined artifact check after both round-1 reports and the fixes:

- Four YAML artifacts parsed with duplicate-key rejection; the Draft 2020-12 schema passed its meta-schema check.
- All **90 shape fixtures** matched their expected result: **49 valid messages, 41 rejected messages**. All 28 valid requests also remained valid after the current raw CLI's top-level null omission.
- All 83 local Markdown references across the 12 checked design/evidence documents resolved; the static themed SVG parsed with no active/external content.
- All 32 unique acceptance scenarios remain present. Phase remains Elaborate; the historical five-round/cap-exhausted record, explicit new-scope authorization and user-directed native-buffer deferral remain intact.

These checks exercise the documented wire shapes, not a runtime ownership implementation. The corrected draft is frozen for fresh independent round-2 review; neither a live UI result nor a quality PASS is claimed yet.

## Round 2 — scoped quality gate passed

| Independent reviewer | Quality | Must-fix checks | Should-fix checks | Verdict |
|---|---:|---|---|---|
| DesignReviewA2 | 100% | None | None | PASS |
| DesignReviewB2 | 97.5% | None | 40 — SPEC-01 | PASS |

Both complete reports were read before any further edit. Both independently traced all 40 checks and accepted the owner-visible conditional observation/consent path. Both preserved the final complete-broker native-fence obligation, unchanged existing launch defaults and explicit separation between specification review and production acceptance. Neither executed validation or runtime checks.

**SPEC-01 disposition: fixed.** The registration paragraph incorrectly named the result discriminator `controller`; it now says `controller-created`, matching the existing schema, positive response fixture and response-variant list. The returned ControllerHandle still has nested `kind: controller`. This is a localized prose correction, not a changed wire design. The recorded scores are the actual independent verdicts, not retroactively increased after that correction.

**AND gate: PASS.** Both scores exceed 94 percent and both have zero must-fix findings. All round-1 findings and the single round-2 should-fix finding are addressed. The historical implementation review's exhausted five-round record is untouched; this separately authorized design scope used two rounds.

**Remaining gate: explicit phase confirmation.** Stay in Elaborate until the user confirms these architectural decisions are locked and authorizes proceeding to Plan (`scope-work` → `slice-tasks` → `plan-work`). No implementation, runtime ownership PASS, release or merge is authorized by this review.

## Final checkpoint verification

After recording both actual verdicts, correcting SPEC-01 and synchronizing the handoff documents, the parent reran the combined artifact check successfully: four duplicate-key-checked YAML artifacts, valid Draft 2020-12 schema, all 90 expected fixture outcomes (49 positive/41 negative), all 28 valid requests through raw-CLI null omission, 85 resolved local Markdown references across 12 documents, and one self-contained themed SVG.

The check also confirmed the registration result spelling against the authoritative schema, the recorded review scores/finding disposition, all 32 unique acceptance scenarios, unchanged historical review cap, retained user deferral, and phase 2 with explicit confirmation still pending. No runtime implementation, live ownership UI or production handoff acceptance was exercised or claimed. No throwaway files or live probe processes were created by these in-memory checks.

## Subsequent phase confirmation

The user answered yes to locking these decisions and proceeding to Plan. State now records phase3 with Elaborate completed; the e03 capsule is implementation planning, not runtime acceptance or Build authorization. The review scores, two-round usage and historical implementation cap above are unchanged.

## Subsequent Plan quality checkpoint

The user-confirmed Plan phase produced the e03 capsule and passed its [separate dual Plan gate](e03-plan-review.md). This Elaborate review's scores and two-round history are unchanged; combined specification usage is now three rounds. Explicit Plan-to-Build confirmation and all runtime acceptance remain outstanding.
