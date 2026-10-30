# e07s03 — Safe close and identifiable sessions

## 1. Type
feat / modified existing behavior

## 2. Status
passing; actual private runtime/cleanup, bg410 mechanical gates, fresh round4 A100/B100 and both exact ledger gates passed; local build only

## 3. Value
Cancel closes nothing; confirm closes only captured live target; exited removal has no second prompt; rename and labels preserve original tuple/PID.

## 4. Scope
src/tui.rs; src/tui/menu.rs; src/daemon.rs; src/acp.rs

## 5. Context
Approved UX review and modal navigation; see PLAN.md and ../../product/SCOPE_E07.yaml.

## 6. Requirements
### MODIFIED: Safe close and identifiable sessions
**Before:** Live native close has no second confirmation; generic session labels and paths are ambiguous.
**After:** Cancel closes nothing; confirm closes only captured live target; exited removal has no second prompt; rename and labels preserve original tuple/PID.

## 7. Actors
Human native attachment; same live native shell or ACP agent; existing local API consumers.

## 8. Preconditions
Isolated feature branch, preserved inherited changes, no global/provider mutations.

## 9. Entry
Explicit Menu/owned-dialog action; session application input otherwise unchanged.

## 10. Flow
Perform the scoped action deliberately, show immediate local feedback, preserve canonical identity, observe actual outcome.

## 11. Alternatives
Arrow/mouse/Tab navigation and existing accelerators remain.

## 12. Errors
Invalid/stale/unauthorized action is explicit, no replay, no silent fallback or partial configuration mutation.

## 13. State
Runtime stays in memory; existing project/config metadata only persists.

## 14. Interfaces
Use exactly the shared interfaces/file owners in PLAN.md; parent owns routing/integration.

## 15. Security
Preserve original controller/tuple/revision, ownership and explicit consent; no auth/provider content in ordinary metadata.

## 16. Resources
Private fixtures only; normal termios restoration, joined drainer/workers, owned PID/socket/root checks and unrelated sentinel.

## 17. Acceptance
Cancel closes nothing; confirm closes only captured live target; exited removal has no second prompt; rename and labels preserve original tuple/PID.

## 18. Implementation and verification
1. Implement the vertical behavior within named ownership boundaries → verify: `cargo test --test runtime && cargo test --test acp`.
2. Exercise all acceptance states using private real controlling PTY and deterministic ACP peer → verify: `bun -e 'const e=JSON.parse(await Bun.file("specs/verifications/e07-ux.json").text()); if(e.stories.e07s03.status!=="passing")process.exit(1)'`. Actual captures and cleanup receipts live in that artifact; story PASS also requires completed fresh independent AND review and this command's success.

## 19. Risk
P1; no new supported security finding in affected input/identity paths. No new dependencies.

## 20. Exclusions
No broker/PTY rewrite, native-settlement repair, auto answers/force/reacquisition, live-provider run, global changes or publication.
