# Native input: second latency pass

## Result and scope

Client-only follow-up to `0d42de5`: prepared native Ratatui cells replace repeated retained-cell JSON decoding; immediate presentation opportunities and brief 1 ms interaction pacing replace waiting for the next 8 ms frame. The daemon, API, PTY input authority, stock-Zsh adapter and OMP extension are unchanged. No synthetic local echo, input replay or shell-readiness suppression.

The new TUI attached to a private daemon running the first-pass build. This pass requires rebuilding and reattaching the TUI, not another daemon restart. Verification did not touch the user's default daemon or first-pass Herdr worktree.

## Diagnosis

`sample <actual-tui-pid> 3 1` during actual 500×163 Zsh typing captured 2,115 main-thread samples: 1,332 under Ratatui `try_draw` projection, dominated by retained-cell serde_json string indexing/BTreeMap comparisons; 111 under buffer diff, 573 under poll/kevent and 82 under `refresh_screen`. [Exact profiler output](immediate-input-sample.log).

The new client decodes only validated changed rows into one bounded styled `Buffer`. Full frames reset/rebuild it. Drawing copies prepared rows, applies selection to the destination only, clips wide glyphs and retains authoritative cursor/mode metadata. Stale/out-of-range deltas mutate neither representation. Input opens an immediate frame opportunity, uses 1 ms polling for 50 ms, then returns to 8 ms idle polling. Up to 32 already-queued events drain before rendering, bounding both burst overhead and presentation starvation.

## Actual attach measurements

macOS arm64 release build; native controlling PTY with continuous output draining. Timer starts when a real key is written into the attached TUI's PTY and stops when the application's native echo is emitted by the wrapper. No CLI-input substitution, decoding socket proxy or optimistic echo. Header occupies three rows. Every row below contains 100 observed single-key samples; none discarded. Percentiles were recorded during the run; individual timings were held in the profiling process rather than persisted.

| Foreground | Wrapper viewport | Separate noisy terminal | Median ms | p95 ms | Max ms |
|---|---|---|---:|---:|---:|
| Stock Zsh | 120×40 | No | 2.01 | 2.14 | 8.09 |
| Stock Zsh | 300×103 | No | 3.94 | 4.13 | 10.25 |
| Stock Zsh | 500×163 | No | 6.77 | 7.12 | 12.23 |
| Raw native echo | 120×40 | No | 1.96 | 2.24 | 2.38 |
| Raw native echo | 300×103 | No | 3.72 | 4.04 | 4.13 |
| Raw native echo | 500×163 | No | 6.74 | 7.18 | 133.58 |
| Raw native echo | 120×40 | Yes | 1.90 | 2.05 | 2.15 |
| Raw native echo | 300×103 | Yes | 3.69 | 4.02 | 4.10 |
| Raw native echo | 500×163 | Yes | 6.72 | 7.19 | 126.59 |
| Raw native echo after 75 ms idle | 120×40 | Yes | 2.16 | 2.92 | 42.22 |

1,000 single-key samples total. Noisy background filled the bounded 50,000-row scrollback and continued producing output every 10 ms. The 500×163 raw 24-byte burst completed in **3.98 ms**, versus the previous **25.61 ms**. Prior first-pass Zsh medians were 10.81 / 17.49 / 30.34 ms at these sizes; see [first-pass evidence and original-baseline caveats](input-latency-final.md).

The 133.58 / 126.59 ms raw maxima were both sample zero, immediately following explicit full-screen clearing. They are retained as cold/full-refresh costs, not filtered from the results. The idle run's 42.22 ms maximum is also retained; its cause was not isolated. Full-grid transfer/rebuild, full-surface buffer copying/diff, OS scheduling and the surrounding terminal remain limits. These timings do **not** measure physical monitor or Herdr compositor paint, and do not guarantee every key is instantaneous.

## Native behavior and actual Herdr surface

- Existing full/delta contract tests extended to prepared-cell rendering: changed Unicode/italic cells, unchanged bold/RGB cells, metadata-only updates, selection without cache mutation, style clearing, wide clipping, resize and stale/out-of-range rejection. No source-text or wiring tests added; Rust test count remains 66.
- Actual foreground raw program restored termios after exercising alternate screen, application cursor, SGR mouse, bracketed paste with Unicode/combining text, focus and literal Ctrl-B. Captured native bytes matched the intended sequences; CLI raw input reached that foreground program. Shell-source execution while foreground busy was refused; exiting restored confirmed shell readiness.
- Partial native Zsh input remained intact and source execution returned `partial-input`. Rapid 40×18 → 300×103 → 80×24 → 180×56 → 120×40 resizing kept the TUI alive.
- More than 200 queued native keystrokes produced the exact ordered command/output, including across wrapped lines. Closing the noisy terminal terminated its shell and foreground Python descendant; tracked PIDs were absent. Actual detach exited the TUI with status 0 while main/background processes remained alive.
- Separate owned Herdr tab `wZ:t4`, pane `wZ:p4`: actual RGB `(17,34,51)` and `界é` native rendering observed; shift-drag selection changed only displayed cells; Switch opened the full hierarchy and Escape returned. No clipboard-modifying mouse release was sent.
- Actual `exit 9` card → Close → No windows → N created a fresh confirmed-ready shell. Actual `exit 0` → Close → No windows → Q returned to the host prompt, without freezing. Owned tab closed; user's panes untouched.
- Private daemon snapshot ended with no terminals. Explicit shutdown acknowledged; owned process/socket cleanup checked afterward. First-pass agent/worktree and user files retained.

## Mechanical checks

`cargo fmt && cargo test -- --nocapture && cargo clippy --all-targets -- -D warnings && cargo build --release`: **66 Rust tests pass**, one intentionally ignored helper, Clippy and release build pass. [Exact output](immediate-input-preflight.log).

`bun test integrations/omp && zsh -n integrations/zsh/adapter.zsh`: **8 OMP tests pass**, 49 assertions, Zsh syntax passes. Adapters intentionally unchanged.

Linux runtime, Rust minimum version and physical-display latency remain unverified. The inherited independent-review cap remains exhausted (5/5); no new independent PASS, merge or release claim.

## Self-review and commit scope

- ✓ Correctness: validated delta boundaries, prepared-cell style clearing/wide clipping/selection isolation, native byte order and real empty-page recovery exercised.
- ✓ Security/supply chain: no new dependencies, authorization boundary, external API or shell execution path; credential/private-key scan of changed implementation/evidence found no matches. The cache consumes bounded local screen data rather than interpreting shell source.
- ✓ Performance: profiler-targeted retained-cell decoding removed; warm keys/bursts measured through real PTYs, cold outliers retained, idle cadence unchanged.
- ✓ Clarity/scope: one presentation cache inside the existing renderer; no new framework or runtime contract. Architecture, usage, changelog, bug registry and state updated.
- ✓ Test coverage: existing behavioral delta tests extended; native scheduling and UI verified with throwaway probes rather than permanent wiring/wording tests.
- Not applied: skill-generic per-function/file-length and naming-count refactors would reorganize the inherited TUI without improving this fix; local `CONVENTIONS.md` does not impose them. Churn script is unavailable (`scripts` is a user-owned empty file), and no replacement scaffold was created.
- Independent review remains blocked by its inherited cap; this self-review is not an independent PASS. Commit classification: `perf(tui)`; conventional default semantic-release interpretation is a patch, with no breaking API change. No retry/backoff/circuit-breaker behavior introduced.
