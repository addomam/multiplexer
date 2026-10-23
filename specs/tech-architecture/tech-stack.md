# Stack and prior art
Rust 2024; alacritty_terminal 0.26.0; portable-pty 0.9.0; crossterm 0.29; clap 4; serde/serde_json; uuid; libc; anyhow.
Stock Zsh 5.9 and OMP 18.8.7 (tagged extension API). No opensrc cache was found locally; primary documentation is used.
Sources: https://docs.rs/alacritty_terminal/0.26.0/ ; https://docs.rs/portable-pty/0.9.0/ ; https://github.com/can1357/oh-my-pi/tree/v18.8.7 ; https://zsh.sourceforge.io/Doc/Release/Zsh-Line-Editor.html

## Language: PTY interaction ownership

**PTY interaction ownership**: The daemon-authoritative exclusive right of one controller to send interaction mutations to a live PTY.
_Avoid_: Agent-only lock, input mutex

**Controller**: An external orchestrator or native client attachment that can hold PTY interaction authority.
_Avoid_: Socket connection, focused window

**Handoff**: A daemon-arbitrated transfer from the current controller to a reserved requester through cooperative yield and requester acceptance, or explicit force.
_Avoid_: Notification, automatic cancellation

**Ownership generation**: The PTY-lifetime authority epoch that prevents a superseded controller token from becoming valid again.
_Avoid_: Input epoch, agent turn generation, screen generation

**ControlHandle**: A runtime capability authorizing its controller's current interaction with one live PTY.
_Avoid_: Pending claim, ready offer

**ClaimHandle**: A runtime capability representing a controller's acquisition intent and its inspect/cancel/force lifecycle.
_Avoid_: Acquired lock, input authority

**OfferHandle**: A runtime capability accepting a safely ready transfer reserved for its requesting controller.
_Avoid_: Automatic grant, current control

## Relationships

- Each live PTY has at most one owning **Controller** and may have multiple observing attachments.
- A cooperative **Handoff** retains the current owner's authority until yield; force or cancellation can invalidate it sooner.
- A completed acquisition issues fresh **Ownership generation** authority; a cancelled handoff grants none.
- Agent instance, conversation and pending-request identity additionally constrain structured agent calls without replacing **PTY interaction ownership**.
- A pending **ClaimHandle** can expose a ready **OfferHandle**; explicit acceptance commits fresh **ControlHandle** authority.
- Serialized handles retain their daemon-memory identity across one-shot calls; socket closure or a client wait timeout does not cancel them.

## Example dialogue

> **Dev:** Does requesting a **Handoff** revoke the owner's token?
> **Domain expert:** No. The owner can reach a suitable point and yield. Force or cancellation can invalidate it before then.

## Flagged ambiguities

- PTY ownership versus agent lifetime: the lock belongs to the PTY; agent-targeted operations also validate their current harness/request identity.
- Cancel takeover versus cancel turn: withdrawing the transfer invalidates pre-transfer authority and leaves the PTY unowned, but does not interrupt agent work.
- Controller versus connection: a registered controller capability survives external one-shot calls; native view provenance is bound to its original attachment connection. See the [wire contract](../contracts/pty-control.md).
- Successful raw request versus acquired control: a pending claim can be a successful API result, but only granted current **ControlHandle** authority means acquisition completed.

## e04 planned ACP boundary

Official `agent-client-protocol-schema = "=1.12.0"` [OK], `default-features = false`, Rust MSRV 1.88, stable wire v1. This is a reviewed-Plan candidate, not an installed/runtime-accepted dependency. Concrete bounded owned stdio uses existing threads/process cleanup and ratatui; the full SDK's unbounded transport runtime/process helper is not selected. Verify serde_json preserve_order/raw_value feature unification and actual lockfile/MSRV with workspace regressions during Build. No async executor, generic provider/plugin SDK or persisted ACP session state. See e04 PLAN, integration contract and ADR0004.
