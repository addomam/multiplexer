---
bug_id: time-rfc2822-dos
type: bug
context: e03 landing dependency security
status: fixed
severity: medium
scope: dependency-resolution
---

# Vulnerable transitive time dependency

## Problem and reproduction

GitHub Dependabot alert 1 reports `time 0.3.45` in the original Cargo.lock, affected by GHSA-r6v5-fh4h-64xc / RUSTSEC-2026-0009 / CVE-2026-25727. The affected range is `>=0.3.6, <0.3.47`; first patched is `0.3.47`. The initial locked dependency graph confirmed `amux -> ratatui 0.30.0 -> ratatui-widgets 0.3.0 -> time 0.3.45`.

Security impact: MEDIUM dependency advisory. The exploit is malicious deeply nested obsolete RFC2822 syntax reaching the crate's date parser and exhausting the stack. Current observed features are alloc/std/local-offset, not parsing; no currently reachable application exploit path was identified. This does not dismiss the vulnerable resolution or resolve the alert.

## Verified root cause

The pinned terminal facade activates widget defaults/calendar even though the application disables its own facade defaults. Its required standard-library feature also strongly activates the optional time dependency. Cargo feature unification is additive, so an application-level feature setting cannot subtract either dependency activation. amux uses ordinary text/list/border widgets, not the calendar.

The first patched time release and its time-core dependency require Rust 1.88; amux initially declared Rust 1.86. Later terminal facade/widget patch releases fix the feature propagation but also require Rust 1.88. A normal Rust-1.86-compatible resolution therefore retained the vulnerable version.

## Falsification checks already executed

- `cargo tree --locked --workspace -i time -e features`: confirms both calendar and std/local-offset activation.
- `cargo update --package time --precise 0.3.47 --dry-run`: proposes only time 0.3.45 -> 0.3.47, time-core 0.1.7 -> 0.1.8 and num-conv 0.1.0 -> 0.2.2; reports Rust 1.88 required. Lockfile unchanged.
- `cargo update --package time --dry-run`: resolves zero updates under the declared Rust 1.86 minimum. Lockfile unchanged.

## Fix plan and acceptance

1. Decide whether to raise the declared minimum to Rust 1.88. This changes a published compatibility promise; do not hide the higher transitive minimum behind a 1.86 manifest.
2. If approved, update the declared minimum and resolve exactly the first patched time release, without refreshing unrelated dependencies or replacing the terminal backend.
3. Verify the locked dependency graph excludes vulnerable time versions; run the workspace tests, Clippy, check/build and the actual isolated native terminal/showcase/stock-Zsh/OMP regression surfaces.
4. Update minimum-toolchain documentation with only exercised verification. No permanent test should pin incidental dependency text or exercise an upstream parser that the application does not expose.

No dependency, toolchain installation, source patch, build, test, live inference or global configuration change was performed during this diagnosis. The dependency alert and merge gate remain open.

## Authorized remediation

The user explicitly approved raising amux's minimum to Rust 1.88 and prefers upstream updates to maintained backports. Cargo.toml now declares 1.88. `cargo update --package time --precise 0.3.47` changed only time 0.3.45 -> 0.3.47, time-core 0.1.7 -> 0.1.8 and num-conv 0.1.0 -> 0.2.2. Ratatui/Crossterm and the independent showcase's Rust 1.86 declaration are unchanged.

Locked Cargo metadata verified exactly one time package, version 0.3.47, and the explicit package minimums. No vulnerable time remains. Equivalent let chains fixed 19 sites newly rejected by the raised-MSRV Clippy lint. Final verification passed: 301 Rust tests (10 ignored), strict workspace Clippy/check/build/format, one exact independent terminal showcase, 8 OMP lifecycle tests and Zsh syntax. A separate private actual-binary PTY smoke proved drawn native control, stock-Zsh input, resize, detach preserving the same shell/daemon, shutdown and birth-qualified owned shell cleanup; its root was removed. See `specs/verifications/e03-fix-runtime.json` and `specs/verifications/e03-integration-build.md`. Rust 1.95.0 was tested; Rust 1.88 execution and coverage were not measured. No upstream backport, global configuration change or new live-model run was introduced. GitHub alert closure is not claimed before default-branch publication and ingestion.

## Sources

- [Dependabot alert](https://github.com/addomam/multiplexer/security/dependabot/1)
- [RustSec advisory](https://rustsec.org/advisories/RUSTSEC-2026-0009.html)
- [Upstream advisory](https://github.com/time-rs/time/security/advisories/GHSA-r6v5-fh4h-64xc)
- Installed Cargo registry manifests and crates.io records for time 0.3.47, ratatui 0.30.0–0.30.2 and ratatui-widgets 0.3.0–0.3.1.
