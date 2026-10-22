//! Daemon-memory PTY ownership and dispatch accounting.
//!
//! A target's mutex is the ordering point for both revocation and actual dispatch.
//! The registry never performs I/O, and an Authority is retained by the operation
//! that originated it: settlement can outlive authority, but cannot revive it.
//! Conditional requests retain native authority until explicit, fully revalidated
//! acceptance. Consent, activity and view facts share this same ordering point;
//! clocks only describe readiness and never schedule another acquisition.

use anyhow::{Result, bail, ensure};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

const PROGRESS_INTERVAL: Duration = Duration::from_secs(3);
const CONTROLLER_LIMIT: usize = 4096;
const CLAIM_HISTORY_LIMIT: usize = 256;
const OFFER_HISTORY_LIMIT: usize = 32;
const DISPATCH_HISTORY_LIMIT: usize = 4096;
const UNSETTLED_LIMIT: usize = 4096;

type TargetRef = Arc<Mutex<Target>>;

pub(crate) struct Registry {
    controllers: BTreeMap<String, Controller>,
    handles: HashMap<String, Binding>,
    targets: BTreeMap<String, TargetRef>,
    retired: Vec<TargetRef>,
    view_revision: u64,
}

#[derive(Clone)]
struct Controller {
    reference: String,
    identity: Identity,
    view: Option<View>,
    claims: BTreeMap<String, TargetRef>,
}

#[derive(Clone, Debug)]
struct Identity {
    id: String,
    role: &'static str,
    label: String,
    native_connection: Option<u64>,
}

#[derive(Clone, Debug)]
struct View {
    controller_id: String,
    connection: u64,
    connected: bool,
    reported: bool,
    serial: u64,
    terminal: Option<String>,
    focused: Option<bool>,
}

impl View {
    fn public(&self) -> Value {
        json!({
            "controller_id": self.controller_id,
            "connected": self.connected,
            "reported": self.reported,
            "serial": self.serial,
            "terminal": self.terminal,
            "focused": self.focused,
        })
    }
}

#[derive(Clone)]
enum Binding {
    Controller(String),
    Control(Arc<Authority>),
    Claim {
        controller: String,
        target: TargetRef,
        id: String,
    },
    Offer {
        controller: String,
        target: TargetRef,
        id: String,
    },
}

impl Binding {
    fn kind(&self) -> &'static str {
        match self {
            Self::Controller(_) => "controller",
            Self::Control(_) => "control",
            Self::Claim { .. } => "claim",
            Self::Offer { .. } => "offer",
        }
    }

    fn controller(&self) -> &str {
        match self {
            Self::Controller(id) => id,
            Self::Control(authority) => &authority.controller_id,
            Self::Claim { controller, .. } | Self::Offer { controller, .. } => controller,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Authority {
    target: TargetRef,
    controller_id: String,
    generation: u64,
    reference: String,
}

#[derive(Debug)]
struct Target {
    terminal: String,
    alive: bool,
    generation: u64,
    owner: Option<Owner>,
    reservation: Option<String>,
    resumable_owner: Option<String>,
    claims: BTreeMap<String, Claim>,
    calls: BTreeMap<(u64, String), Call>,
    // Only current-generation dispatch IDs need tombstones. Once revoked, the
    // authority cannot dispatch again, so settled IDs can be reclaimed together.
    dispatched: HashSet<String>,
    native_batch: Option<String>,
    activity_revision: u64,
    last_activity: Option<Instant>,
    clock_sample: Option<Instant>,
    view_revision: u64,
    views: BTreeMap<String, View>,
    pending_handles: Vec<NewHandle>,
    events: Vec<(String, Value)>,
}

#[derive(Debug)]
struct Owner {
    identity: Identity,
    reference: String,
    generation: u64,
    // Controller recovery returns the issuance receipt, not a new safe-grant
    // acknowledgement for work that has subsequently been forwarded.
    receipt: Value,
}

#[derive(Debug)]
struct Claim {
    id: String,
    reference: String,
    requester: Identity,
    source_owner: Option<String>,
    source_generation: u64,
    yielded_generation: Option<u64>,
    phase: &'static str,
    created: Instant,
    phase_since: Instant,
    progress_at: Instant,
    offer: Option<Offer>,
    stale_offers: std::collections::VecDeque<String>,
    grant: Option<Grant>,
    conditional: Option<Conditional>,
}

#[derive(Clone, Copy, Debug)]
enum Condition {
    HumanIdle { idle_ms: u64, warning_ms: u64 },
    ViewsBackgrounded { all: bool },
}

#[derive(Debug)]
struct Conditional {
    when: Condition,
    view_revision: u64,
    consent: Option<Consent>,
    // Excludes the interpolated remainder: only semantic clock transitions emit.
    published: Option<(&'static str, bool, bool, &'static str, bool)>,
}

#[derive(Debug)]
struct Consent {
    activity_revision: u64,
    view_revision: u64,
    view_serial: u64,
    started: Instant,
}

impl Condition {
    fn from_when(when: &Value) -> Option<Self> {
        match when["kind"].as_str() {
            Some("human-idle") => Some(Self::HumanIdle {
                idle_ms: when["idle_ms"].as_u64().expect("validated idle duration"),
                warning_ms: when["warning_ms"]
                    .as_u64()
                    .expect("validated warning duration"),
            }),
            Some("views-backgrounded") => Some(Self::ViewsBackgrounded {
                all: when["scope"] == "all",
            }),
            _ => None,
        }
    }

    fn public(&self) -> Value {
        match self {
            Self::HumanIdle {
                idle_ms,
                warning_ms,
            } => json!({"kind":"human-idle","idle_ms":idle_ms,"warning_ms":warning_ms}),
            Self::ViewsBackgrounded { all } => {
                json!({"kind":"views-backgrounded","scope":if *all { "all" } else { "owner" }})
            }
        }
    }
}

#[derive(Debug)]
struct Offer {
    reference: String,
    consumed: bool,
}

#[derive(Debug)]
struct Grant {
    reference: String,
    generation: u64,
    mode: &'static str,
}

#[derive(Debug)]
struct Call {
    kind: &'static str,
    controller_id: String,
    generation: u64,
    outcome: &'static str,
    started: Instant,
}

impl Call {
    fn public(&self, id: &str, now: Instant) -> Value {
        json!({
            "id": id,
            "kind": self.kind,
            "controller_id": self.controller_id,
            "generation": self.generation,
            "outcome": self.outcome,
            "elapsed_ms": elapsed_ms(now, self.started),
        })
    }
}

#[derive(Debug)]
enum NewHandle {
    Remove(String),
    Control {
        reference: String,
        controller: String,
        generation: u64,
    },
    Claim {
        reference: String,
        controller: String,
        id: String,
    },
    Offer {
        reference: String,
        controller: String,
        id: String,
    },
}

impl Registry {
    pub(crate) fn new() -> Self {
        Self {
            controllers: BTreeMap::new(),
            handles: HashMap::new(),
            targets: BTreeMap::new(),
            retired: Vec::new(),
            view_revision: 0,
        }
    }

    pub(crate) fn add_target(&mut self, terminal: &str) {
        let mut generation = 0;
        if let Some(previous) = self.targets.remove(terminal) {
            self.with_target(previous.clone(), |target| {
                target.end(Instant::now());
                Ok(())
            })
            .expect("ending a target is infallible");
            generation = previous
                .lock()
                .generation
                .checked_add(1)
                .expect("ownership generation exhausted");
            self.retired.push(previous);
        }
        let views = self
            .controllers
            .iter()
            .filter_map(|(id, controller)| controller.view.clone().map(|view| (id.clone(), view)))
            .collect();
        self.targets.insert(
            terminal.to_owned(),
            Arc::new(Mutex::new(Target {
                terminal: terminal.to_owned(),
                alive: true,
                generation,
                owner: None,
                reservation: None,
                resumable_owner: None,
                claims: BTreeMap::new(),
                calls: BTreeMap::new(),
                dispatched: HashSet::new(),
                native_batch: None,
                activity_revision: 0,
                last_activity: None,
                clock_sample: None,
                view_revision: self.view_revision,
                views,
                pending_handles: Vec::new(),
                events: Vec::new(),
            })),
        );
    }

    pub(crate) fn end_target(&mut self, terminal: &str) {
        if let Some(target) = self.targets.get(terminal).cloned() {
            self.with_target(target, |target| {
                target.end(Instant::now());
                Ok(())
            })
            .expect("ending a target is infallible");
        }
    }

    pub(crate) fn create(&mut self, request: &Value, connection: u64) -> Result<Value> {
        self.create_at(request, connection, Instant::now())
    }

    fn create_at(&mut self, request: &Value, connection: u64, now: Instant) -> Result<Value> {
        request_shape(
            request,
            "controller-create",
            &["role", "label"],
            &["role", "label"],
        )?;
        let role = match string(request, "role")? {
            "external" => "external",
            "attachment" => {
                ensure!(
                    request["persistent"] == true,
                    "invalid-request: attachment registration requires persistent:true"
                );
                "attachment"
            }
            _ => bail!("invalid-request: unknown controller role"),
        };
        let label = request["label"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("invalid-request: label must be a string"))?;
        ensure!(
            self.controllers.len() < CONTROLLER_LIMIT,
            "controller-busy: controller registration limit reached; close unused registrations"
        );
        let id = mint();
        let reference = mint();
        let view = (role == "attachment").then(|| View {
            controller_id: id.clone(),
            connection,
            connected: true,
            reported: false,
            serial: 0,
            terminal: None,
            focused: None,
        });
        self.controllers.insert(
            id.clone(),
            Controller {
                reference: reference.clone(),
                identity: Identity {
                    id: id.clone(),
                    role,
                    label: label.to_owned(),
                    native_connection: (role == "attachment").then_some(connection),
                },
                view: view.clone(),
                claims: BTreeMap::new(),
            },
        );
        self.handles
            .insert(reference.clone(), Binding::Controller(id.clone()));
        if let Some(view) = view {
            self.change_view(&id, Some(view), now);
        }
        Ok(json!({"kind":"controller-created", "controller_id":id,
            "controller":capability("controller", &reference)}))
    }

    pub(crate) fn claim(&mut self, request: &Value) -> Result<Value> {
        self.claim_at(request, Instant::now())
    }

    fn claim_at(&mut self, request: &Value, now: Instant) -> Result<Value> {
        let mode = string(request, "mode")?;
        match mode {
            "cooperative" => {
                request_shape(
                    request,
                    "ownership-claim",
                    &["controller", "terminal", "observed", "mode", "when"],
                    &["controller", "terminal", "observed", "mode", "when"],
                )?;
                validate_when(&request["when"])?;
            }
            "force" => request_shape(
                request,
                "ownership-claim",
                &["controller", "terminal", "observed", "mode"],
                &["controller", "terminal", "observed", "mode"],
            )?,
            _ => bail!("invalid-request: unknown ownership mode"),
        }
        let Binding::Controller(controller_id) = self.lookup(&request["controller"])? else {
            bail!("wrong-handle-kind: ownership-claim requires a controller");
        };
        let controller = self
            .controllers
            .get(&controller_id)
            .ok_or_else(|| anyhow::anyhow!("stale-handle: controller is closed"))?;
        let identity = controller.identity.clone();
        let history_full = controller.claims.len() >= CLAIM_HISTORY_LIMIT;
        let terminal = string(request, "terminal")?;
        let target = self.target(terminal)?;
        self.with_target(target, |target| {
            target.require_live()?;
            target.compare(&request["observed"])?;
            if mode == "force" {
                target.supersede_reservation(now);
                return Ok(target.grant(identity, "forced", None, now));
            }
            let condition = Condition::from_when(&request["when"]);
            if condition.is_some() {
                ensure!(
                    identity.role == "external"
                        && target.owner.as_ref().is_some_and(|owner| {
                            owner.identity.role == "attachment"
                                && owner.identity.native_connection.is_some_and(|connection|
                                    target.is_original_native(&owner.identity.id, connection))
                        }),
                    "condition-unavailable: conditional claims require an external requester and a live native owner"
                );
            }
            ensure!(
                target
                    .owner
                    .as_ref()
                    .is_none_or(|owner| owner.identity.id != identity.id),
                "already-owner: inspect the controller's current control"
            );
            ensure!(
                target.reservation.is_none(),
                "control-reserved: another claim reserves this target"
            );
            if condition.is_none() && target.owner.is_none() {
                if target.calls.is_empty() {
                    return Ok(target.grant(identity, "safe", None, now));
                }
                if target.resumable_owner.as_deref() == Some(identity.id.as_str())
                    && target
                        .calls
                        .values()
                        .all(|call| call.controller_id == identity.id)
                {
                    return Ok(target.grant(identity, "resumed", None, now));
                }
            }
            ensure!(
                !history_full,
                "controller-busy: claim history limit reached; close an idle registration"
            );
            let id = mint();
            let reference = mint();
            let phase = if condition.is_some() {
                "watching"
            } else if target.owner.is_some() {
                "waiting-owner"
            } else {
                "waiting-settlement"
            };
            let source_owner = target
                .owner
                .as_ref()
                .map(|owner| owner.identity.id.clone())
                .or_else(|| target.resumable_owner.clone());
            let yielded_generation = target.owner.is_none().then_some(target.generation);
            target.claims.insert(
                id.clone(),
                Claim {
                    id: id.clone(),
                    reference: reference.clone(),
                    requester: identity.clone(),
                    source_owner,
                    source_generation: target.generation,
                    yielded_generation,
                    phase,
                    created: now,
                    phase_since: now,
                    progress_at: now,
                    offer: None,
                    stale_offers: std::collections::VecDeque::new(),
                    grant: None,
                    conditional: condition.map(|when| Conditional {
                        when,
                        view_revision: target.view_revision,
                        consent: None,
                        published: None,
                    }),
                },
            );
            target.pending_handles.push(NewHandle::Claim {
                reference,
                controller: identity.id,
                id: id.clone(),
            });
            target.reservation = Some(id.clone());
            target.emit("control-requested", json!({"claim_id":id}), now);
            target.refresh(now);
            Ok(target.claim_state(&id, now))
        })
    }

    pub(crate) fn handle(&mut self, request: &Value, connection: u64) -> Result<Value> {
        self.handle_at(request, connection, Instant::now())
    }

    fn handle_at(&mut self, request: &Value, connection: u64, now: Instant) -> Result<Value> {
        let method = string(request, "method")?;
        let binding = self.lookup(&request["handle"])?;
        let fields: &[&str] = match (binding.kind(), method) {
            ("controller", "view") => &["handle", "method", "view"],
            ("control", "yield-to" | "keep") => &["handle", "method", "claim_id"],
            ("control", "consent-to") => &[
                "handle",
                "method",
                "claim_id",
                "view_serial",
                "activity_revision",
            ],
            ("claim", "force") => &["handle", "method", "observed"],
            (_, "read" | "close" | "release" | "activity" | "cancel" | "withdraw" | "accept") => {
                &["handle", "method"]
            }
            _ => bail!("invalid-request: unsupported method for this handle kind"),
        };
        request_shape(request, "control-handle", fields, fields)?;
        match binding {
            Binding::Controller(id) => match method {
                "read" => {
                    for target in self.all_targets() {
                        self.with_target(target, |target| {
                            target.refresh(now);
                            Ok(())
                        })?;
                    }
                    self.controller_state(&id)
                }
                "close" => self.close_controller(&id, now),
                "view" => self.report_view(&id, &request["view"], connection, now),
                _ => bail!("wrong-handle-kind: method requires a different capability kind"),
            },
            Binding::Control(authority) => {
                let target = authority.target.clone();
                self.with_target(target, |target| {
                    target.refresh(now);
                    authority.check_locked(target)?;
                    match method {
                        "read" => Ok(json!({"kind":"control-state",
                            "control":capability("control", &authority.reference),
                            "ownership":target.snapshot(now)})),
                        "release" => {
                            if target.reservation.as_ref().is_some_and(|id|
                                target.claims[id].conditional.is_some())
                            {
                                target.supersede_reservation(now);
                            }
                            target.revoke();
                            target.resumable_owner = None;
                            target.mark_yielded(now);
                            target.emit("control-yielded", json!({}), now);
                            target.refresh(now);
                            Ok(transition("released", target.snapshot(now)))
                        }
                        "yield-to" => {
                            let id = string(request, "claim_id")?;
                            target.require_source_claim(id, &authority)?;
                            target.revoke();
                            target.resumable_owner = None;
                            target.mark_yielded(now);
                            target.emit("control-yielded", json!({"claim_id":id}), now);
                            target.refresh(now);
                            Ok(transition("yielded", target.snapshot(now)))
                        }
                        "activity" => {
                            ensure!(target.is_original_native(&authority.controller_id, connection),
                                "not-native-connection: activity requires the original live attachment connection");
                            target.record_activity(now);
                            Ok(transition("activity-recorded", target.snapshot(now)))
                        }
                        "keep" | "consent-to" => {
                            let id = string(request, "claim_id")?;
                            target.require_pending_claim(id)?;
                            let claim = &target.claims[id];
                            ensure!(claim.conditional.is_some(),
                                "wrong-claim-kind: ordinary claims do not support conditional consent or keep");
                            ensure!(
                                claim.source_generation == authority.generation
                                    && claim.source_owner.as_deref() == Some(authority.controller_id.as_str()),
                                "claim-completed: claim no longer targets this owner"
                            );
                            ensure!(target.is_original_native(&authority.controller_id, connection),
                                "not-native-connection: consent and keep require the original live attachment connection");
                            if method == "keep" {
                                target.finish_conditional(id, "aborted-by-owner", "control-aborted", now);
                                target.record_activity(now);
                                Ok(transition("kept", target.snapshot(now)))
                            } else {
                                target.consent(id, request, now)?;
                                Ok(transition("consent-recorded", target.snapshot(now)))
                            }
                        }
                        _ => bail!("wrong-handle-kind: method requires a different capability kind"),
                    }
                })
            }
            Binding::Claim { target, id, .. } => self.with_target(target, |target| {
                target.refresh(now);
                if method == "read" {
                    return Ok(target.claim_state(&id, now));
                }
                ensure!(
                    matches!(method, "cancel" | "withdraw" | "force"),
                    "wrong-handle-kind: method requires a different capability kind"
                );
                target.require_live()?;
                target.require_pending_claim(&id)?;
                if method == "force" {
                    target.compare(&request["observed"])?;
                    let identity = target.claims[&id].requester.clone();
                    return Ok(target.grant(identity, "forced", Some(&id), now));
                }
                let conditional = target.claims[&id].conditional.is_some();
                ensure!(conditional == (method == "withdraw"),
                    "wrong-claim-kind: conditional claims require withdraw; ordinary claims require cancel");
                if conditional {
                    target.finish_conditional(&id, "withdrawn", "control-withdrawn", now);
                    return Ok(transition("withdrawn", target.snapshot(now)));
                }
                let source = target.claims[&id].source_owner.clone();
                if let Some(owner) = &target.owner {
                    let claim = &target.claims[&id];
                    ensure!(
                        Some(owner.identity.id.as_str()) == claim.source_owner.as_deref()
                            && owner.generation == claim.source_generation,
                        "claim-completed: reservation no longer names this owner"
                    );
                    target.revoke();
                }
                target.resumable_owner = source;
                target.reservation = None;
                let claim = target.claims.get_mut(&id).expect("bound claim exists");
                claim.phase = "cancelled";
                claim.phase_since = now;
                target.emit("control-cancelled", json!({"claim_id":id}), now);
                Ok(transition("cancelled", target.snapshot(now)))
            }),
            Binding::Offer { target, id, .. } => self.with_target(target, |target| {
                target.refresh(now);
                ensure!(
                    method == "accept",
                    "wrong-handle-kind: offers support only accept"
                );
                let reference = string(&request["handle"], "ref")?;
                let claim = target
                    .claims
                    .get(&id)
                    .ok_or_else(|| anyhow::anyhow!("stale-offer: claim is no longer retained"))?;
                let offer = claim
                    .offer
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("stale-offer: claim has no offer"))?;
                ensure!(
                    offer.reference == reference,
                    "stale-offer: offer revision changed"
                );
                ensure!(
                    !offer.consumed,
                    "offer-consumed: inspect the claim to recover the grant"
                );
                target.require_live()?;
                ensure!(
                    target.reservation.as_deref() == Some(id.as_str()) && claim.phase == "offered",
                    "stale-offer: reservation no longer offers this transfer"
                );
                if claim.conditional.is_some() {
                    target.require_conditional_accept(&id, now)?;
                } else {
                    ensure!(
                        target.owner.is_none() && claim.yielded_generation == Some(target.generation),
                        "stale-offer: source generation changed"
                    );
                }
                ensure!(
                    target.calls.is_empty(),
                    "not-ready: forwarded calls have not settled"
                );
                let identity = claim.requester.clone();
                Ok(target.grant(identity, "safe", Some(&id), now))
            }),
        }
    }

    pub(crate) fn snapshot(&self, terminal: &str) -> Value {
        self.snapshot_at(terminal, Instant::now())
    }

    fn snapshot_at(&self, terminal: &str, now: Instant) -> Value {
        self.targets.get(terminal).map_or(Value::Null, |target| {
            let mut target = target.lock();
            target.refresh(now);
            target.snapshot(now)
        })
    }

    pub(crate) fn authorize(
        &mut self,
        terminal: &str,
        handle: &Value,
        _connection: u64,
    ) -> Result<Arc<Authority>> {
        let Binding::Control(authority) = self.lookup(handle)? else {
            bail!("wrong-handle-kind: input requires a current control capability");
        };
        {
            let target = authority.target.lock();
            ensure!(
                target.terminal == terminal,
                "controller-mismatch: control belongs to a different terminal"
            );
            authority.check_locked(&target)?;
        }
        Ok(authority)
    }

    pub(crate) fn depart(&mut self, connection: u64) {
        self.depart_at(connection, Instant::now());
    }

    fn depart_at(&mut self, connection: u64, now: Instant) {
        let changed: Vec<_> = self
            .controllers
            .iter_mut()
            .filter_map(|(id, controller)| {
                let view = controller.view.as_mut()?;
                if view.connection != connection || !view.connected {
                    return None;
                }
                view.connected = false;
                Some((id.clone(), view.clone()))
            })
            .collect();
        for (id, view) in changed {
            self.change_view(&id, Some(view), now);
        }
    }

    pub(crate) fn tick(&mut self, now: Instant) {
        for target in self.all_targets() {
            self.with_target(target, |target| {
                target.refresh(now);
                if let Some(id) = target.reservation.clone() {
                    let claim = target.claims.get_mut(&id).expect("reserved claim exists");
                    if now.saturating_duration_since(claim.progress_at) >= PROGRESS_INTERVAL {
                        claim.progress_at = now;
                        let blockers = target.blockers(&id, now);
                        target.emit(
                            "control-progress",
                            json!({"claim_id":id,"blockers":blockers}),
                            now,
                        );
                    }
                }
                Ok(())
            })
            .expect("tick is infallible");
        }
    }

    pub(crate) fn take_events(&mut self) -> Vec<(String, String, Value)> {
        let mut events = Vec::new();
        for target in self.all_targets() {
            let mut state = target.lock();
            self.sync_handles(&target, &mut state);
            let terminal = state.terminal.clone();
            events.extend(
                state
                    .events
                    .drain(..)
                    .map(|(kind, data)| (kind, terminal.clone(), data)),
            );
        }
        // Replaced lifetimes without retained work/history need no registry root.
        self.retired.retain(|target| {
            let target = target.lock();
            !target.calls.is_empty() || !target.claims.is_empty()
        });
        events
    }

    fn lookup(&self, handle: &Value) -> Result<Binding> {
        object_shape(handle, &["kind", "ref"], &["kind", "ref"])?;
        let kind = string(handle, "kind")?;
        ensure!(
            matches!(kind, "controller" | "control" | "claim" | "offer"),
            "wrong-handle-kind: unknown capability kind"
        );
        let reference = string(handle, "ref")?;
        let binding = self
            .handles
            .get(reference)
            .ok_or_else(|| anyhow::anyhow!("stale-handle: capability is unknown or revoked"))?;
        ensure!(
            binding.kind() == kind,
            "wrong-handle-kind: tag does not match the minted capability"
        );
        Ok(binding.clone())
    }

    fn target(&self, terminal: &str) -> Result<TargetRef> {
        self.targets
            .get(terminal)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("target-ended: terminal has no current PTY lifetime"))
    }

    fn all_targets(&self) -> Vec<TargetRef> {
        self.targets
            .values()
            .chain(self.retired.iter())
            .cloned()
            .collect()
    }

    fn with_target<T>(
        &mut self,
        target: TargetRef,
        apply: impl FnOnce(&mut Target) -> Result<T>,
    ) -> Result<T> {
        let mut state = target.lock();
        let result = apply(&mut state);
        self.sync_handles(&target, &mut state);
        result
    }

    fn sync_handles(&mut self, target: &TargetRef, state: &mut Target) {
        for handle in state.pending_handles.drain(..) {
            match handle {
                NewHandle::Remove(reference) => {
                    self.handles.remove(&reference);
                }
                NewHandle::Control {
                    reference,
                    controller,
                    generation,
                } => {
                    self.handles.insert(
                        reference.clone(),
                        Binding::Control(Arc::new(Authority {
                            target: target.clone(),
                            controller_id: controller,
                            generation,
                            reference,
                        })),
                    );
                }
                NewHandle::Claim {
                    reference,
                    controller,
                    id,
                } => {
                    if let Some(owner) = self.controllers.get_mut(&controller) {
                        owner.claims.insert(id.clone(), target.clone());
                    }
                    self.handles.insert(
                        reference,
                        Binding::Claim {
                            controller,
                            target: target.clone(),
                            id,
                        },
                    );
                }
                NewHandle::Offer {
                    reference,
                    controller,
                    id,
                } => {
                    self.handles.insert(
                        reference,
                        Binding::Offer {
                            controller,
                            target: target.clone(),
                            id,
                        },
                    );
                }
            }
        }
    }

    fn controller_state(&self, id: &str) -> Result<Value> {
        let controller = self
            .controllers
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("stale-handle: controller is closed"))?;
        let controls: Vec<_> = self
            .targets
            .values()
            .filter_map(|target| {
                let target = target.lock();
                target
                    .owner
                    .as_ref()
                    .filter(|owner| owner.identity.id == id)
                    .map(|owner| owner.receipt.clone())
            })
            .collect();
        let claims: Vec<_> = controller
            .claims
            .iter()
            .filter_map(|(id, target)| {
                let target = target.lock();
                let claim = target.claims.get(id)?;
                Some(
                    json!({"claim":capability("claim", &claim.reference), "claim_id":claim.id,
                "terminal":target.terminal, "purpose":claim.purpose(), "phase":claim.phase}),
                )
            })
            .collect();
        Ok(
            json!({"kind":"controller-state", "controller":capability("controller", &controller.reference),
            "controller_id":id, "controls":controls, "claims":claims}),
        )
    }

    fn close_controller(&mut self, id: &str, now: Instant) -> Result<Value> {
        let controller = self
            .controllers
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("stale-handle: controller is closed"))?;
        for target in self.all_targets() {
            let target = target.lock();
            ensure!(
                target
                    .owner
                    .as_ref()
                    .is_none_or(|owner| owner.identity.id != id),
                "controller-busy: release owned terminals before closing"
            );
        }
        for (claim_id, target) in &controller.claims {
            let target = target.lock();
            ensure!(
                target
                    .claims
                    .get(claim_id)
                    .is_none_or(|claim| !claim.pending()),
                "controller-busy: cancel pending claims before closing"
            );
        }
        let controller = self.controllers.remove(id).expect("controller exists");
        self.handles.retain(|_, binding| binding.controller() != id);
        for (claim_id, target) in controller.claims {
            target.lock().claims.remove(&claim_id);
        }
        if controller.view.is_some() {
            self.change_view(id, None, now);
        }
        Ok(transition("controller-closed", Value::Null))
    }

    fn report_view(
        &mut self,
        id: &str,
        value: &Value,
        connection: u64,
        now: Instant,
    ) -> Result<Value> {
        object_shape(
            value,
            &["serial", "terminal", "focused"],
            &["serial", "terminal", "focused"],
        )?;
        let serial = value["serial"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("invalid-request: view serial must be a counter"))?;
        let terminal = match &value["terminal"] {
            Value::Null => None,
            Value::String(terminal) if !terminal.is_empty() => {
                self.target(terminal)?.lock().require_live()?;
                Some(terminal.clone())
            }
            _ => bail!("invalid-request: view terminal must be a terminal ID or null"),
        };
        let focused = match value["focused"] {
            Value::Null => None,
            Value::Bool(focused) => Some(focused),
            _ => bail!("invalid-request: view focused must be a boolean or null"),
        };
        let controller = self
            .controllers
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("stale-handle: controller is closed"))?;
        let view = controller.view.as_mut().ok_or_else(|| {
            anyhow::anyhow!("not-native-connection: external controllers have no native view")
        })?;
        ensure!(
            view.connection == connection && view.connected,
            "not-native-connection: views require the original live attachment connection"
        );
        ensure!(
            !view.reported || serial > view.serial,
            "stale-view: view serial did not increase"
        );
        view.serial = serial;
        view.reported = true;
        view.terminal = terminal.clone();
        view.focused = focused;
        let view = view.clone();
        self.change_view(id, Some(view), now);
        let ownership = terminal.as_ref().map_or(Value::Null, |terminal| {
            self.targets[terminal].lock().snapshot(now)
        });
        Ok(transition("view-recorded", ownership))
    }

    fn change_view(&mut self, id: &str, view: Option<View>, now: Instant) {
        self.view_revision = self
            .view_revision
            .checked_add(1)
            .expect("view revision exhausted");
        for target_ref in self.all_targets() {
            let mut target = target_ref.lock();
            let now = target.observe(now);
            if let Some(view) = &view {
                target.views.insert(id.to_owned(), view.clone());
            } else {
                target.views.remove(id);
            }
            target.view_revision = self.view_revision;
            target.changed_condition_view(id, now);
            target.refresh(now);
            target.emit("control-view-changed", json!({}), now);
            self.sync_handles(&target_ref, &mut target);
        }
    }
}

impl Authority {
    pub(crate) fn check(&self) -> Result<()> {
        self.check_locked(&self.target.lock())
    }

    fn check_locked(&self, target: &Target) -> Result<()> {
        target.require_live()?;
        ensure!(
            target.owner.as_ref().is_some_and(|owner| {
                owner.reference == self.reference
                    && owner.generation == self.generation
                    && owner.identity.id == self.controller_id
                    && target.generation == self.generation
            }),
            "stale-handle: control is no longer current"
        );
        Ok(())
    }

    /// Called only after successful public input admission. Borrowing a native
    /// capability from another socket does not manufacture human activity.
    pub(crate) fn activity(&self, connection: u64) -> Result<()> {
        self.activity_at(connection, Instant::now())
    }

    fn activity_at(&self, connection: u64, now: Instant) -> Result<()> {
        let mut target = self.target.lock();
        self.check_locked(&target)?;
        if target.is_original_native(&self.controller_id, connection) {
            target.record_activity(now);
        }
        Ok(())
    }

    pub(crate) fn forward_native(&self) -> Result<()> {
        self.forward_native_at(Instant::now())
    }

    fn forward_native_at(&self, now: Instant) -> Result<()> {
        let mut target = self.target.lock();
        self.check_locked(&target)?;
        if target.native_batch.is_some() {
            return Ok(());
        }
        ensure!(
            target.calls.len() < UNSETTLED_LIMIT,
            "settlement-unavailable: retained unsettled call limit reached"
        );
        let id = mint();
        target.calls.insert(
            (self.generation, id.clone()),
            Call {
                kind: "native-input",
                controller_id: self.controller_id.clone(),
                generation: self.generation,
                outcome: "unknown",
                started: now,
            },
        );
        target.native_batch = Some(id);
        target.refresh(now);
        Ok(())
    }

    pub(crate) fn forward(&self, call_id: &str, kind: &str) -> Result<()> {
        self.forward_at(call_id, kind, Instant::now())
    }

    fn forward_at(&self, call_id: &str, kind: &str, now: Instant) -> Result<()> {
        ensure!(
            !call_id.is_empty(),
            "invalid-request: dispatch requires a nonempty call ID"
        );
        let kind = match kind {
            "shell-execute" => "shell-execute",
            "agent-mutation" => "agent-mutation",
            _ => bail!("invalid-request: unsupported action kind"),
        };
        let mut target = self.target.lock();
        self.check_locked(&target)?;
        ensure!(
            !target.dispatched.contains(call_id),
            "invalid-request: call was already forwarded"
        );
        ensure!(
            target.dispatched.len() < DISPATCH_HISTORY_LIMIT
                && target.calls.len() < UNSETTLED_LIMIT,
            "settlement-unavailable: dispatch accounting limit reached; no retained call was discarded"
        );
        target.dispatched.insert(call_id.to_owned());
        target.calls.insert(
            (self.generation, call_id.to_owned()),
            Call {
                kind,
                controller_id: self.controller_id.clone(),
                generation: self.generation,
                outcome: "pending",
                started: now,
            },
        );
        target.refresh(now);
        Ok(())
    }

    pub(crate) fn settle(&self, call_id: &str, outcome: &str) {
        self.settle_at(call_id, outcome, Instant::now());
    }

    fn settle_at(&self, call_id: &str, outcome: &str, now: Instant) {
        if !matches!(outcome, "applied" | "rejected" | "unknown") {
            return;
        }
        let mut target = self.target.lock();
        let key = (self.generation, call_id.to_owned());
        let Some(call) = target.calls.get_mut(&key) else {
            return;
        };
        if call.controller_id != self.controller_id {
            return;
        }
        // There is no native application acknowledgement seam yet. Neither a
        // write receipt nor a generic settlement report proves native drain.
        if call.kind == "native-input" && outcome != "unknown" {
            return;
        }
        if outcome == "unknown" {
            call.outcome = "unknown";
        } else {
            target.calls.remove(&key);
        }
        target.emit(
            "control-call-settled",
            json!({"call_id":call_id,"outcome":outcome}),
            now,
        );
        target.refresh(now);
    }
}

impl Claim {
    fn pending(&self) -> bool {
        matches!(
            self.phase,
            "waiting-owner" | "watching" | "countdown" | "waiting-settlement" | "offered"
        )
    }

    fn purpose(&self) -> &'static str {
        if self.conditional.is_some() {
            "conditional"
        } else {
            "manual"
        }
    }
}

impl Target {
    fn observe(&mut self, now: Instant) -> Instant {
        let now = self.clock_sample.map_or(now, |previous| previous.max(now));
        self.clock_sample = Some(now);
        now
    }

    fn require_live(&self) -> Result<()> {
        ensure!(self.alive, "target-ended: PTY lifetime ended");
        Ok(())
    }

    fn compare(&self, observed: &Value) -> Result<()> {
        object_shape(
            observed,
            &["generation", "reservation_id"],
            &["generation", "reservation_id"],
        )?;
        let generation = observed["generation"].as_u64().ok_or_else(|| {
            anyhow::anyhow!("invalid-request: observed generation must be a counter")
        })?;
        let reservation = match &observed["reservation_id"] {
            Value::Null => None,
            Value::String(id) if !id.is_empty() => Some(id.as_str()),
            _ => bail!("invalid-request: reservation_id must be an ID or null"),
        };
        ensure!(
            generation == self.generation && reservation == self.reservation.as_deref(),
            "stale-observation: ownership or reservation changed"
        );
        Ok(())
    }

    fn unsettled(&self, now: Instant) -> Vec<Value> {
        self.calls
            .iter()
            .map(|((_, id), call)| call.public(id, now))
            .collect()
    }

    fn snapshot(&self, now: Instant) -> Value {
        let now = self.clock_sample.map_or(now, |previous| previous.max(now));
        let owner = self.owner.as_ref().map(|owner| json!({
            "controller_id":owner.identity.id, "role":owner.identity.role, "label":owner.identity.label,
        }));
        let reservation = self
            .reservation
            .as_ref()
            .and_then(|id| self.claims.get(id))
            .map(|claim| {
                let mut public = json!({
                    "claim_id":claim.id, "requester_id":claim.requester.id,
                    "purpose":claim.purpose(), "phase":claim.phase,
                });
                if claim.conditional.is_some() {
                    public["conditional"] = self.conditional_public(claim, now);
                }
                public
            });
        let views: Vec<_> = self.views.values().map(View::public).collect();
        json!({"terminal":self.terminal,
            "observed":{"generation":self.generation,"reservation_id":self.reservation},
            "owner":owner, "reservation":reservation, "unsettled":self.unsettled(now),
            "activity_revision":self.activity_revision,"view_revision":self.view_revision,"views":views})
    }

    fn emit(&mut self, kind: &str, mut data: Value, now: Instant) {
        data["ownership"] = self.snapshot(now);
        self.events.push((kind.to_owned(), data));
    }

    fn revoke(&mut self) {
        if let Some(owner) = self.owner.take() {
            self.pending_handles
                .push(NewHandle::Remove(owner.reference));
            self.generation = self
                .generation
                .checked_add(1)
                .expect("ownership generation exhausted");
        }
        self.dispatched.clear();
        self.native_batch = None;
    }

    fn grant(
        &mut self,
        identity: Identity,
        mode: &'static str,
        claim_id: Option<&str>,
        now: Instant,
    ) -> Value {
        let now = self.observe(now);
        self.revoke();
        self.generation = self
            .generation
            .checked_add(1)
            .expect("ownership generation exhausted");
        let reference = mint();
        let unsettled = self.unsettled(now);
        let mut receipt = json!({"kind":"control","control":capability("control", &reference),
            "terminal":self.terminal,"generation":self.generation,"mode":mode,"unsettled":unsettled});
        if mode == "forced" {
            receipt["warning"] =
                json!({"code":"prior-calls-may-apply","calls":receipt["unsettled"]});
        }
        self.owner = Some(Owner {
            identity: identity.clone(),
            reference: reference.clone(),
            generation: self.generation,
            receipt: receipt.clone(),
        });
        self.reservation = None;
        self.resumable_owner = None;
        if identity.role == "attachment" {
            self.record_activity(now);
        }
        self.pending_handles.push(NewHandle::Control {
            reference: reference.clone(),
            controller: identity.id,
            generation: self.generation,
        });
        if let Some(id) = claim_id {
            let claim = self
                .claims
                .get_mut(id)
                .expect("grant names a retained claim");
            claim.phase = "granted";
            claim.phase_since = now;
            if let Some(offer) = &mut claim.offer {
                offer.consumed = true;
            }
            claim.grant = Some(Grant {
                reference,
                generation: self.generation,
                mode,
            });
        }
        let data = if let Some(id) = claim_id {
            json!({"mode":mode,"claim_id":id})
        } else {
            json!({"mode":mode})
        };
        self.emit("control-granted", data, now);
        receipt
    }

    fn require_source_claim(&self, id: &str, authority: &Authority) -> Result<()> {
        self.require_pending_claim(id)?;
        let claim = &self.claims[id];
        ensure!(
            claim.conditional.is_none(),
            "wrong-claim-kind: conditional claims require consent, not yield-to"
        );
        ensure!(
            claim.phase == "waiting-owner"
                && claim.source_generation == authority.generation
                && claim.source_owner.as_deref() == Some(authority.controller_id.as_str()),
            "claim-completed: claim no longer targets this ownership generation"
        );
        Ok(())
    }

    fn require_pending_claim(&self, id: &str) -> Result<()> {
        let claim = self.claims.get(id).ok_or_else(|| {
            anyhow::anyhow!("claim-completed: claim is not retained for this target")
        })?;
        ensure!(
            claim.pending() && self.reservation.as_deref() == Some(id),
            "claim-completed: claim is no longer the active reservation"
        );
        Ok(())
    }

    fn mark_yielded(&mut self, now: Instant) {
        if let Some(id) = &self.reservation {
            let claim = self.claims.get_mut(id).expect("reserved claim exists");
            claim.phase = "waiting-settlement";
            claim.phase_since = now;
            claim.yielded_generation = Some(self.generation);
        }
    }

    fn source_is_current(&self, claim: &Claim) -> bool {
        self.owner.as_ref().is_some_and(|owner| {
            Some(owner.identity.id.as_str()) == claim.source_owner.as_deref()
                && owner.generation == claim.source_generation
                && self.generation == claim.source_generation
        })
    }

    fn owner_can_consent(&self, claim: &Claim) -> bool {
        self.source_is_current(claim)
            && claim
                .source_owner
                .as_ref()
                .and_then(|id| self.views.get(id))
                .is_some_and(|view| view.connected && view.reported && view.focused.is_some())
    }

    fn eligibility(&self, claim: &Claim, now: Instant) -> &'static str {
        let now = self.clock_sample.map_or(now, |previous| previous.max(now));
        let conditional = claim.conditional.as_ref().expect("conditional claim");
        if !self.source_is_current(claim) {
            return "unknown";
        }
        match conditional.when {
            Condition::HumanIdle { idle_ms, .. } => match self.last_activity {
                Some(last) if elapsed_ms(now, last) >= idle_ms => "eligible",
                Some(_) => "ineligible",
                None => "unknown",
            },
            Condition::ViewsBackgrounded { all } => {
                let mut unknown = false;
                for view in self.views.values().filter(|view| {
                    view.connected
                        && (all
                            || Some(view.controller_id.as_str()) == claim.source_owner.as_deref())
                }) {
                    if !view.reported || view.focused.is_none() {
                        unknown = true;
                    } else if view.terminal.as_deref() == Some(self.terminal.as_str())
                        && view.focused == Some(true)
                    {
                        return "ineligible";
                    }
                }
                if unknown || !self.owner_can_consent(claim) {
                    "unknown"
                } else {
                    "eligible"
                }
            }
        }
    }

    fn conditional_clock(&self, claim: &Claim, now: Instant) -> (&'static str, Option<u64>) {
        let now = self.clock_sample.map_or(now, |previous| previous.max(now));
        let conditional = claim.conditional.as_ref().expect("conditional claim");
        match conditional.when {
            Condition::HumanIdle {
                idle_ms,
                warning_ms,
            } => {
                if let Some(consent) = &conditional.consent {
                    (
                        "warning",
                        Some(warning_ms.saturating_sub(elapsed_ms(now, consent.started))),
                    )
                } else {
                    (
                        "idle",
                        Some(self.last_activity.map_or(idle_ms, |last| {
                            idle_ms.saturating_sub(elapsed_ms(now, last))
                        })),
                    )
                }
            }
            Condition::ViewsBackgrounded { .. } => ("none", None),
        }
    }

    fn conditional_facts(
        &self,
        claim: &Claim,
        now: Instant,
    ) -> (&'static str, bool, bool, &'static str, bool) {
        let conditional = claim.conditional.as_ref().expect("conditional claim");
        let eligibility = self.eligibility(claim, now);
        let recorded = conditional.consent.is_some();
        let ready = claim.pending()
            && self.reservation.as_deref() == Some(claim.id.as_str())
            && eligibility == "eligible"
            && !recorded
            && self.owner_can_consent(claim);
        let (kind, remaining) = self.conditional_clock(claim, now);
        (
            eligibility,
            ready,
            recorded,
            kind,
            kind == "warning" && remaining == Some(0),
        )
    }

    fn conditional_public(&self, claim: &Claim, now: Instant) -> Value {
        let conditional = claim.conditional.as_ref().expect("conditional claim");
        let (eligibility, ready, recorded, _, _) = self.conditional_facts(claim, now);
        let (kind, remaining) = self.conditional_clock(claim, now);
        json!({"when":conditional.when.public(),"eligibility":eligibility,
            "ready_for_consent":ready,"consent":if recorded { "recorded" } else { "awaiting" },
            "clock":{"kind":kind,"remaining_ms":remaining}})
    }

    fn consent(&mut self, id: &str, request: &Value, now: Instant) -> Result<()> {
        let now = self.observe(now);
        let claim = &self.claims[id];
        let view = claim
            .source_owner
            .as_ref()
            .and_then(|owner| self.views.get(owner))
            .ok_or_else(|| anyhow::anyhow!("condition-unavailable: native owner has no view"))?;
        let serial = request["view_serial"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("invalid-request: view_serial must be a counter"))?;
        let activity = request["activity_revision"].as_u64().ok_or_else(|| {
            anyhow::anyhow!("invalid-request: activity_revision must be a counter")
        })?;
        ensure!(
            view.reported,
            "condition-unavailable: native owner has not reported a view"
        );
        ensure!(
            serial == view.serial,
            "stale-view: displayed owner view changed"
        );
        ensure!(
            activity == self.activity_revision,
            "stale-observation: human activity changed since display"
        );
        ensure!(
            self.owner_can_consent(claim) && self.eligibility(claim, now) != "unknown",
            "condition-unavailable: current native facts cannot establish consent"
        );
        ensure!(
            self.conditional_facts(claim, now).1,
            "not-ready: condition is not ready for fresh consent"
        );
        let conditional = self
            .claims
            .get_mut(id)
            .expect("claim exists")
            .conditional
            .as_mut()
            .expect("conditional claim");
        conditional.consent = Some(Consent {
            activity_revision: activity,
            view_revision: conditional.view_revision,
            view_serial: serial,
            started: now,
        });
        self.refresh(now);
        Ok(())
    }

    fn require_conditional_accept(&self, id: &str, now: Instant) -> Result<()> {
        let claim = &self.claims[id];
        let conditional = claim.conditional.as_ref().expect("conditional claim");
        let consent = conditional
            .consent
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("stale-offer: conditional consent was withdrawn"))?;
        ensure!(
            self.source_is_current(claim) && self.owner_can_consent(claim),
            "stale-offer: native owner or connection changed"
        );
        ensure!(
            consent.activity_revision == self.activity_revision
                && consent.view_revision == conditional.view_revision
                && claim
                    .source_owner
                    .as_ref()
                    .and_then(|id| self.views.get(id))
                    .is_some_and(|view| view.serial == consent.view_serial),
            "stale-offer: consent observations changed"
        );
        ensure!(
            self.eligibility(claim, now) == "eligible",
            "not-ready: conditional predicate is no longer eligible"
        );
        ensure!(
            self.conditional_clock(claim, now)
                .1
                .is_none_or(|ms| ms == 0),
            "not-ready: warning is not complete"
        );
        ensure!(
            self.calls.is_empty(),
            "not-ready: forwarded calls have not settled"
        );
        Ok(())
    }

    fn invalidate_offer(&mut self, id: &str) {
        let claim = self.claims.get_mut(id).expect("claim exists");
        if let Some(offer) = claim.offer.take() {
            // Retain bounded recent stale-offer diagnostics, never reusable
            // proposals or an unbounded handle leak under view churn.
            claim.stale_offers.push_back(offer.reference);
            if claim.stale_offers.len() > OFFER_HISTORY_LIMIT {
                self.pending_handles.push(NewHandle::Remove(
                    claim
                        .stale_offers
                        .pop_front()
                        .expect("history exceeds limit"),
                ));
            }
        }
    }

    fn finish_conditional(&mut self, id: &str, phase: &'static str, event: &str, now: Instant) {
        self.invalidate_offer(id);
        let claim = self.claims.get_mut(id).expect("claim exists");
        claim.phase = phase;
        claim.phase_since = now;
        claim
            .conditional
            .as_mut()
            .expect("conditional claim")
            .consent = None;
        self.reservation = None;
        self.emit(event, json!({"claim_id":id}), now);
    }

    fn changed_condition_view(&mut self, controller: &str, now: Instant) {
        let Some(id) = self.reservation.clone() else {
            return;
        };
        let claim = self.claims.get_mut(&id).expect("claim exists");
        let Some(conditional) = &mut claim.conditional else {
            return;
        };
        let relevant = matches!(conditional.when, Condition::ViewsBackgrounded { all: true })
            || claim.source_owner.as_deref() == Some(controller);
        if relevant {
            conditional.view_revision = self.view_revision;
            conditional.consent = None;
            claim.phase = "watching";
            claim.phase_since = now;
            self.invalidate_offer(&id);
        }
    }

    fn refresh_conditional(&mut self, id: &str, now: Instant) {
        if !self.source_is_current(&self.claims[id]) {
            self.supersede_reservation(now);
            return;
        }
        let claim = &self.claims[id];
        let conditional = claim.conditional.as_ref().expect("conditional claim");
        if conditional
            .consent
            .as_ref()
            .is_some_and(|consent| consent.activity_revision != self.activity_revision)
        {
            self.finish_conditional(id, "aborted-by-owner", "control-aborted", now);
            return;
        }
        if conditional.consent.is_some()
            && (self.eligibility(claim, now) != "eligible" || !self.owner_can_consent(claim))
        {
            let claim = self.claims.get_mut(id).expect("claim exists");
            claim
                .conditional
                .as_mut()
                .expect("conditional claim")
                .consent = None;
            self.invalidate_offer(id);
        }
        let facts = self.conditional_facts(&self.claims[id], now);
        let complete = facts.2 && (facts.3 == "none" || facts.4);
        let phase = if !facts.2 {
            "watching"
        } else if !complete {
            "countdown"
        } else if !self.calls.is_empty() {
            "waiting-settlement"
        } else {
            "offered"
        };
        let claim = self.claims.get_mut(id).expect("claim exists");
        let changed_phase = claim.phase != phase;
        if changed_phase {
            claim.phase = phase;
            claim.phase_since = now;
        }
        let conditional = claim.conditional.as_mut().expect("conditional claim");
        let changed_facts = conditional.published != Some(facts);
        conditional.published = Some(facts);
        if phase != "offered" {
            self.invalidate_offer(id);
        } else if claim.offer.is_none() {
            self.make_offer(id, now);
        }
        if changed_facts || changed_phase {
            let blockers = self.blockers(id, now);
            self.emit(
                "control-progress",
                json!({"claim_id":id,"blockers":blockers}),
                now,
            );
        }
    }

    fn refresh(&mut self, now: Instant) {
        let now = self.observe(now);
        if !self.alive {
            return;
        }
        let Some(id) = self.reservation.clone() else {
            return;
        };
        if self.claims[&id].conditional.is_some() {
            self.refresh_conditional(&id, now);
            return;
        }
        if self.owner.is_some() || !self.calls.is_empty() {
            return;
        }
        let claim = self.claims.get_mut(&id).expect("reserved claim exists");
        if claim.phase != "waiting-settlement" || claim.yielded_generation != Some(self.generation)
        {
            return;
        }
        self.make_offer(&id, now);
    }

    fn make_offer(&mut self, id: &str, now: Instant) {
        let claim = self.claims.get_mut(id).expect("claim exists");
        let reference = mint();
        claim.phase = "offered";
        claim.phase_since = now;
        claim.offer = Some(Offer {
            reference: reference.clone(),
            consumed: false,
        });
        self.pending_handles.push(NewHandle::Offer {
            reference,
            controller: claim.requester.id.clone(),
            id: id.to_owned(),
        });
        self.emit("control-offered", json!({"claim_id":id}), now);
    }

    fn blockers(&self, id: &str, now: Instant) -> Vec<Value> {
        let claim = &self.claims[id];
        if !claim.pending() {
            return Vec::new();
        }
        let mut blockers = Vec::new();
        if claim.phase == "waiting-owner" {
            blockers.push(json!({"kind":"owner","elapsed_ms":elapsed_ms(now, claim.created)}));
        } else if claim.phase == "offered" {
            blockers.push(json!({"kind":"requester-acceptance","elapsed_ms":elapsed_ms(now, claim.phase_since)}));
        } else if claim.conditional.is_some() {
            let facts = self.conditional_facts(claim, now);
            let kind = if facts.1 {
                "owner-consent"
            } else {
                "condition"
            };
            blockers.push(json!({"kind":kind,"elapsed_ms":elapsed_ms(now, claim.phase_since)}));
        }
        blockers.extend(
            self.calls
                .iter()
                .map(|((_, id), call)| json!({"kind":"settlement","call":call.public(id, now)})),
        );
        blockers
    }

    fn claim_state(&self, id: &str, now: Instant) -> Value {
        let claim = &self.claims[id];
        let mut state = json!({"kind":"claim-state", "claim":capability("claim", &claim.reference),
            "claim_id":claim.id,"purpose":claim.purpose(),"phase":claim.phase,"terminal":self.terminal,
            "ownership":self.snapshot(now),"blockers":self.blockers(id, now)});
        if claim.phase == "offered" {
            let offer = claim.offer.as_ref().expect("offered claim has an offer");
            state["offer"] = capability("offer", &offer.reference);
        }
        if let Some(grant) = &claim.grant {
            let current = self.alive
                && self.owner.as_ref().is_some_and(|owner| {
                    owner.reference == grant.reference
                        && owner.generation == grant.generation
                        && owner.identity.id == claim.requester.id
                });
            state["current"] = json!(current);
            state["grant_generation"] = json!(grant.generation);
            state["grant_mode"] = json!(grant.mode);
            if current {
                state["control"] = capability("control", &grant.reference);
            }
        }
        state
    }

    fn supersede_reservation(&mut self, now: Instant) {
        if let Some(id) = self.reservation.take() {
            let claim = self.claims.get_mut(&id).expect("reserved claim exists");
            claim.phase = "superseded";
            claim.phase_since = now;
        }
    }

    fn end(&mut self, now: Instant) {
        if !self.alive {
            return;
        }
        self.alive = false;
        self.revoke();
        self.resumable_owner = None;
        if let Some(id) = self.reservation.take() {
            let claim = self.claims.get_mut(&id).expect("reserved claim exists");
            claim.phase = "target-ended";
            claim.phase_since = now;
            self.emit("control-cancelled", json!({"claim_id":id}), now);
        }
    }

    fn is_original_native(&self, controller: &str, connection: u64) -> bool {
        self.owner.as_ref().is_some_and(|owner| {
            owner.identity.id == controller
                && owner.identity.native_connection == Some(connection)
                && self
                    .views
                    .get(controller)
                    .is_some_and(|view| view.connected && view.connection == connection)
        })
    }

    fn record_activity(&mut self, now: Instant) {
        let now = self.observe(now);
        self.activity_revision = self
            .activity_revision
            .checked_add(1)
            .expect("activity revision exhausted");
        self.last_activity = Some(self.last_activity.map_or(now, |previous| previous.max(now)));
        let Some(id) = self.reservation.as_deref() else {
            return;
        };
        let Some(conditional) = self.claims[id].conditional.as_ref() else {
            return;
        };
        if conditional.consent.is_some() {
            let id = id.to_owned();
            self.finish_conditional(&id, "aborted-by-owner", "control-aborted", now);
        } else if conditional.published != Some(self.conditional_facts(&self.claims[id], now)) {
            self.refresh(now);
        }
    }
}

fn mint() -> String {
    Uuid::new_v4().to_string()
}

fn capability(kind: &str, reference: &str) -> Value {
    json!({"kind":kind,"ref":reference})
}

fn transition(outcome: &str, ownership: Value) -> Value {
    json!({"kind":"transition","outcome":outcome,"ownership":ownership})
}

fn elapsed_ms(now: Instant, start: Instant) -> u64 {
    now.saturating_duration_since(start)
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("invalid-request: {field} must be a nonempty string"))
}

fn object_shape(value: &Value, allowed: &[&str], required: &[&str]) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("invalid-request: expected an object"))?;
    ensure!(
        object.keys().all(|key| allowed.contains(&key.as_str())),
        "invalid-request: unknown field"
    );
    ensure!(
        required.iter().all(|field| object.contains_key(*field)),
        "invalid-request: missing required field"
    );
    Ok(())
}

fn request_shape(request: &Value, op: &str, fields: &[&str], required: &[&str]) -> Result<()> {
    let object = request
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("invalid-request: expected a request object"))?;
    ensure!(
        request["version"] == 1,
        "unsupported-version: API version 1 required"
    );
    ensure!(request["op"] == op, "invalid-request: unexpected operation");
    ensure!(
        object.keys().all(
            |key| matches!(key.as_str(), "version" | "op" | "persistent")
                || fields.contains(&key.as_str())
        ),
        "invalid-request: unknown request field"
    );
    ensure!(
        required.iter().all(|field| object.contains_key(*field)),
        "invalid-request: missing required request field"
    );
    ensure!(
        object.get("persistent").is_none_or(Value::is_boolean),
        "invalid-request: persistent must be a boolean"
    );
    Ok(())
}

fn validate_when(when: &Value) -> Result<()> {
    match string(when, "kind")? {
        "now" => object_shape(when, &["kind"], &["kind"]),
        "human-idle" => {
            object_shape(
                when,
                &["kind", "idle_ms", "warning_ms"],
                &["kind", "idle_ms", "warning_ms"],
            )?;
            ensure!(
                when["idle_ms"].as_u64().is_some()
                    && when["warning_ms"].as_u64().is_some_and(|ms| ms > 0),
                "invalid-request: idle and warning durations must be valid counters"
            );
            Ok(())
        }
        "views-backgrounded" => {
            object_shape(when, &["kind", "scope"], &["kind", "scope"])?;
            ensure!(
                matches!(when["scope"].as_str(), Some("owner" | "all")),
                "invalid-request: background scope must be owner or all"
            );
            Ok(())
        }
        _ => bail!("invalid-request: unknown ownership condition"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn register(registry: &mut Registry, role: &str, connection: u64, now: Instant) -> Value {
        registry
            .create_at(
                &json!({"version":1,"op":"controller-create","role":role,
            "label":role,"persistent":role == "attachment"}),
                connection,
                now,
            )
            .unwrap()
    }

    fn observation(registry: &Registry, terminal: &str) -> Value {
        registry.targets[terminal].lock().snapshot(Instant::now())["observed"].clone()
    }

    fn acquire(registry: &mut Registry, controller: &Value, terminal: &str, now: Instant) -> Value {
        registry
            .claim_at(
                &json!({"version":1,"op":"ownership-claim","controller":controller,
            "terminal":terminal,"observed":observation(registry,terminal),"mode":"cooperative",
            "when":{"kind":"now"}}),
                now,
            )
            .unwrap()
    }

    fn method(registry: &mut Registry, handle: &Value, method: &str, now: Instant) -> Value {
        registry
            .handle_at(
                &json!({"version":1,"op":"control-handle","handle":handle,"method":method}),
                1,
                now,
            )
            .unwrap()
    }

    fn error(result: Result<impl Sized>, code: &str) {
        let Err(error) = result else {
            panic!("expected {code}");
        };
        assert!(
            error.to_string().starts_with(&format!("{code}:")),
            "{error:#}"
        );
    }

    fn yielded(registry: &mut Registry, control: &Value, claim: &Value, now: Instant) {
        registry
            .handle_at(
                &json!({"version":1,"op":"control-handle","handle":control,
            "method":"yield-to","claim_id":claim["claim_id"]}),
                1,
                now,
            )
            .unwrap();
    }

    fn native_fixture(now: Instant) -> (Registry, Value, Value, Value) {
        let mut registry = Registry::new();
        registry.add_target("pty");
        let native = register(&mut registry, "attachment", 1, now);
        view_at(&mut registry, &native, 1, 1, json!("pty"), json!(true), now);
        let human = acquire(&mut registry, &native["controller"], "pty", now);
        let external = register(&mut registry, "external", 2, now);
        (registry, native, human, external)
    }

    fn view_at(
        registry: &mut Registry,
        controller: &Value,
        connection: u64,
        serial: u64,
        terminal: Value,
        focused: Value,
        now: Instant,
    ) {
        registry.handle_at(&json!({
            "version":1,"op":"control-handle","handle":controller["controller"],"method":"view",
            "view":{"serial":serial,"terminal":terminal,"focused":focused}
        }), connection, now).unwrap();
    }

    fn conditional_at(
        registry: &mut Registry,
        external: &Value,
        when: Value,
        now: Instant,
    ) -> Value {
        let observed = registry.snapshot_at("pty", now)["observed"].clone();
        registry
            .claim_at(
                &json!({
                    "version":1,"op":"ownership-claim","controller":external["controller"],
                    "terminal":"pty","mode":"cooperative","observed":observed,"when":when
                }),
                now,
            )
            .unwrap()
    }

    fn consent_request(control: &Value, claim: &Value, ownership: &Value, serial: u64) -> Value {
        json!({"version":1,"op":"control-handle","handle":control,"method":"consent-to",
            "claim_id":claim["claim_id"],"view_serial":serial,
            "activity_revision":ownership["activity_revision"]})
    }

    fn consent_at(
        registry: &mut Registry,
        human: &Value,
        claim: &Value,
        serial: u64,
        now: Instant,
    ) {
        let observation = registry.snapshot_at("pty", now);
        registry
            .handle_at(
                &consent_request(&human["control"], claim, &observation, serial),
                1,
                now,
            )
            .unwrap();
    }

    fn claim_action(handle: &Value, method: &str, claim: &Value) -> Value {
        json!({"version":1,"op":"control-handle","handle":handle,
            "method":method,"claim_id":claim["claim_id"]})
    }

    #[test]
    fn idle_readiness_warning_and_settlement_share_the_public_clock() {
        let now = Instant::now();
        let (mut registry, _, human, external) = native_fixture(now);
        let authority = registry.authorize("pty", &human["control"], 1).unwrap();
        let claim = conditional_at(
            &mut registry,
            &external,
            json!({"kind":"human-idle","idle_ms":250,"warning_ms":1700}),
            now,
        );
        let initial = &claim["ownership"]["reservation"]["conditional"];
        assert_eq!(claim["purpose"], "conditional");
        assert_eq!(initial["clock"], json!({"kind":"idle","remaining_ms":250}));
        assert_eq!(initial["ready_for_consent"], false);
        let ready = registry.snapshot_at("pty", now + Duration::from_millis(250));
        assert_eq!(
            ready["reservation"]["conditional"]["ready_for_consent"],
            true
        );
        assert_eq!(
            ready["reservation"]["conditional"]["clock"],
            json!({"kind":"idle","remaining_ms":0})
        );
        assert_eq!(ready["reservation"]["conditional"]["consent"], "awaiting");
        // Reaching idle zero is not warning expiry and has no authority effect.
        authority.check().unwrap();
        authority
            .forward_at("answer", "agent-mutation", now)
            .unwrap();
        consent_at(
            &mut registry,
            &human,
            &claim,
            1,
            now + Duration::from_millis(300),
        );
        let warning = registry.snapshot_at("pty", now + Duration::from_millis(1000));
        assert_eq!(
            warning["reservation"]["conditional"]["clock"],
            json!({"kind":"warning","remaining_ms":1000})
        );
        registry.tick(now + Duration::from_millis(2000));
        let blocked = method(
            &mut registry,
            &claim["claim"],
            "read",
            now + Duration::from_millis(2000),
        );
        assert_eq!(blocked["phase"], "waiting-settlement");
        assert_eq!(
            blocked["ownership"]["reservation"]["conditional"]["clock"],
            json!({"kind":"warning","remaining_ms":0})
        );
        assert!(blocked.get("offer").is_none());
        authority.check().unwrap();
        authority.settle_at("answer", "unknown", now + Duration::from_secs(3));
        registry.tick(now + Duration::from_secs(100));
        assert_eq!(
            method(
                &mut registry,
                &claim["claim"],
                "read",
                now + Duration::from_secs(100)
            )["phase"],
            "waiting-settlement"
        );
        authority.settle_at("answer", "applied", now + Duration::from_secs(101));
        let offered = method(
            &mut registry,
            &claim["claim"],
            "read",
            now + Duration::from_secs(101),
        );
        assert_eq!(offered["phase"], "offered");
        assert_eq!(offered["ownership"]["owner"], claim["ownership"]["owner"]);
        authority.check().unwrap();
        let grant = method(
            &mut registry,
            &offered["offer"],
            "accept",
            now + Duration::from_secs(101),
        );
        assert_eq!(grant["mode"], "safe");
        assert_ne!(grant["control"], human["control"]);
        error(authority.check(), "stale-handle");
    }

    #[test]
    fn conditional_events_publish_semantics_not_interpolated_ticks_or_private_handles() {
        let now = Instant::now();
        let (mut registry, _, human, external) = native_fixture(now);
        let claim = conditional_at(
            &mut registry,
            &external,
            json!({"kind":"human-idle","idle_ms":100,"warning_ms":777}),
            now,
        );
        registry.take_events();
        let elapsed = registry.snapshot_at("pty", now + Duration::from_millis(50));
        assert_eq!(
            elapsed["reservation"]["conditional"]["clock"]["remaining_ms"],
            50
        );
        assert!(registry.take_events().is_empty());
        registry.snapshot_at("pty", now + Duration::from_millis(100));
        let eligible = registry.take_events();
        assert!(
            eligible
                .iter()
                .any(|(kind, _, data)| kind == "control-progress"
                    && data["ownership"]["reservation"]["conditional"]["ready_for_consent"]
                        == true)
        );
        consent_at(
            &mut registry,
            &human,
            &claim,
            1,
            now + Duration::from_millis(100),
        );
        let consent = registry.take_events();
        assert!(consent.iter().any(
            |(_, _, data)| data["ownership"]["reservation"]["conditional"]["clock"]["kind"]
                == "warning"
        ));
        registry.snapshot_at("pty", now + Duration::from_millis(500));
        assert!(registry.take_events().is_empty());
        // A post-gap snapshot refreshes expiry/phase, even without a timer tick.
        let restored = registry.snapshot_at("pty", now + Duration::from_secs(20));
        assert_eq!(restored["reservation"]["phase"], "offered");
        assert_eq!(
            restored["reservation"]["conditional"]["when"]["warning_ms"],
            777
        );
        assert_eq!(
            restored["reservation"]["conditional"]["clock"]["remaining_ms"],
            0
        );
        let expired = registry.take_events();
        assert!(
            expired
                .iter()
                .any(|(kind, _, data)| kind == "control-progress"
                    && data["ownership"]["reservation"]["conditional"]["clock"]["remaining_ms"]
                        == 0)
        );
        for (_, _, data) in eligible.iter().chain(consent.iter()).chain(expired.iter()) {
            let encoded = data.to_string();
            assert!(!encoded.contains("\"ref\""));
            assert!(!encoded.contains(human["control"]["ref"].as_str().unwrap()));
            assert!(!encoded.contains(claim["claim"]["ref"].as_str().unwrap()));
        }
        let private = method(
            &mut registry,
            &claim["claim"],
            "read",
            now + Duration::from_secs(20),
        );
        assert_eq!(private["ownership"], restored);
        assert!(private.get("remaining_ms").is_none());
    }

    #[test]
    fn preconsent_activity_resets_idle_without_rearming_and_stale_display_cannot_consent() {
        let now = Instant::now();
        let (mut registry, native, human, external) = native_fixture(now);
        let authority = registry.authorize("pty", &human["control"], 1).unwrap();
        let claim = conditional_at(
            &mut registry,
            &external,
            json!({"kind":"human-idle","idle_ms":100,"warning_ms":500}),
            now,
        );
        let before = registry.snapshot_at("pty", now + Duration::from_millis(100));
        let stale = consent_request(&human["control"], &claim, &before, 1);
        authority
            .activity_at(1, now + Duration::from_millis(100))
            .unwrap();
        error(
            registry.handle_at(&stale, 1, now + Duration::from_millis(100)),
            "stale-observation",
        );
        let reset = registry.snapshot_at("pty", now + Duration::from_millis(100));
        assert_eq!(reset["reservation"]["claim_id"], claim["claim_id"]);
        assert_eq!(
            reset["reservation"]["conditional"]["clock"]["remaining_ms"],
            100
        );
        let early = consent_request(&human["control"], &claim, &reset, 1);
        error(
            registry.handle_at(&early, 1, now + Duration::from_millis(100)),
            "not-ready",
        );
        view_at(
            &mut registry,
            &native,
            1,
            2,
            json!("pty"),
            json!(true),
            now + Duration::from_millis(200),
        );
        let ready = registry.snapshot_at("pty", now + Duration::from_millis(200));
        let stale_view = consent_request(&human["control"], &claim, &ready, 1);
        error(
            registry.handle_at(&stale_view, 1, now + Duration::from_millis(200)),
            "stale-view",
        );
        let valid = consent_request(&human["control"], &claim, &ready, 2);
        error(
            registry.handle_at(&valid, 9, now + Duration::from_millis(200)),
            "not-native-connection",
        );
        registry
            .handle_at(&valid, 1, now + Duration::from_millis(200))
            .unwrap();
        authority.check().unwrap();
    }

    #[test]
    fn zero_idle_still_rejects_activity_between_display_and_consent() {
        let now = Instant::now();
        let (mut registry, _, human, external) = native_fixture(now);
        let claim = conditional_at(
            &mut registry,
            &external,
            json!({"kind":"human-idle","idle_ms":0,"warning_ms":1}),
            now,
        );
        let stale = consent_request(&human["control"], &claim, &claim["ownership"], 1);
        let authority = registry.authorize("pty", &human["control"], 1).unwrap();
        authority.activity_at(1, now).unwrap();
        error(registry.handle_at(&stale, 1, now), "stale-observation");
        assert_eq!(
            registry.snapshot_at("pty", now)["reservation"]["conditional"]["ready_for_consent"],
            true
        );
        consent_at(&mut registry, &human, &claim, 1, now);
        assert_eq!(
            method(&mut registry, &claim["claim"], "read", now)["phase"],
            "countdown"
        );
    }

    #[test]
    fn pending_ack_keep_aborts_once_and_late_consent_never_resurrects_it() {
        for consent_first in [false, true] {
            let now = Instant::now();
            let (mut registry, _, human, external) = native_fixture(now);
            let claim = conditional_at(
                &mut registry,
                &external,
                json!({"kind":"human-idle","idle_ms":0,"warning_ms":900}),
                now,
            );
            let consent = consent_request(&human["control"], &claim, &claim["ownership"], 1);
            if consent_first {
                registry.handle_at(&consent, 1, now).unwrap();
            }
            let keep = claim_action(&human["control"], "keep", &claim);
            error(registry.handle_at(&keep, 8, now), "not-native-connection");
            let kept = registry.handle_at(&keep, 1, now).unwrap();
            assert_eq!(kept["outcome"], "kept");
            error(registry.handle_at(&consent, 1, now), "claim-completed");
            registry.tick(now + Duration::from_secs(86400));
            let aborted = method(
                &mut registry,
                &claim["claim"],
                "read",
                now + Duration::from_secs(86400),
            );
            assert_eq!(aborted["phase"], "aborted-by-owner");
            assert!(aborted["ownership"]["reservation"].is_null());
            let current = method(
                &mut registry,
                &human["control"],
                "read",
                now + Duration::from_secs(86400),
            );
            assert_eq!(current["control"], human["control"]);
            assert_eq!(
                current["ownership"]["observed"]["generation"],
                human["generation"]
            );
            // Only a fresh, explicit external request can arm another attempt.
            let fresh = conditional_at(
                &mut registry,
                &external,
                json!({"kind":"human-idle","idle_ms":0,"warning_ms":123}),
                now + Duration::from_secs(86400),
            );
            assert_ne!(fresh["claim_id"], claim["claim_id"]);
            assert_eq!(fresh["phase"], "watching");
        }
    }

    #[test]
    fn conditional_activity_accept_race_has_exactly_one_winner_and_never_revives_human_control() {
        for activity_first in [false, true] {
            let now = Instant::now();
            let (mut registry, _, human, external) = native_fixture(now);
            let authority = registry.authorize("pty", &human["control"], 1).unwrap();
            let claim = conditional_at(
                &mut registry,
                &external,
                json!({"kind":"human-idle","idle_ms":0,"warning_ms":1}),
                now,
            );
            consent_at(&mut registry, &human, &claim, 1, now);
            let later = now + Duration::from_millis(1);
            let offered = method(&mut registry, &claim["claim"], "read", later);
            let accept = json!({"version":1,"op":"control-handle","handle":offered["offer"],"method":"accept"});
            if activity_first {
                authority.activity_at(1, later).unwrap();
                error(registry.handle_at(&accept, 2, later), "stale-offer");
                assert_eq!(
                    method(&mut registry, &claim["claim"], "read", later)["phase"],
                    "aborted-by-owner"
                );
                assert_eq!(
                    method(&mut registry, &human["control"], "read", later)["control"],
                    human["control"]
                );
            } else {
                let grant = registry.handle_at(&accept, 2, later).unwrap();
                assert_ne!(grant["control"], human["control"]);
                error(authority.activity_at(1, later), "stale-handle");
                error(
                    authority.forward_at("late-input", "agent-mutation", later),
                    "stale-handle",
                );
                assert_eq!(
                    method(&mut registry, &claim["claim"], "read", later)["control"],
                    grant["control"]
                );
            }
        }
    }

    #[test]
    fn dispatch_after_conditional_offer_invalidates_it_until_exact_calls_settle() {
        let now = Instant::now();
        let (mut registry, _, human, external) = native_fixture(now);
        let authority = registry.authorize("pty", &human["control"], 1).unwrap();
        let claim = conditional_at(
            &mut registry,
            &external,
            json!({"kind":"human-idle","idle_ms":0,"warning_ms":1}),
            now,
        );
        consent_at(&mut registry, &human, &claim, 1, now);
        let later = now + Duration::from_millis(1);
        let old = method(&mut registry, &claim["claim"], "read", later);
        authority
            .forward_at("queued-answer", "agent-mutation", later)
            .unwrap();
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle",
            "handle":old["offer"],"method":"accept"}),
                2,
                later,
            ),
            "stale-offer",
        );
        let waiting = method(&mut registry, &claim["claim"], "read", later);
        assert_eq!(waiting["phase"], "waiting-settlement");
        assert_eq!(
            waiting["ownership"]["reservation"]["conditional"]["consent"],
            "recorded"
        );
        authority.settle_at("queued-answer", "unknown", later);
        assert_eq!(
            method(&mut registry, &claim["claim"], "read", later)["phase"],
            "waiting-settlement"
        );
        authority.settle_at("queued-answer", "rejected", later);
        let fresh = method(&mut registry, &claim["claim"], "read", later);
        assert_ne!(fresh["offer"], old["offer"]);
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle",
            "handle":old["offer"],"method":"accept"}),
                2,
                later,
            ),
            "stale-offer",
        );
        method(&mut registry, &fresh["offer"], "accept", later);
    }

    #[test]
    fn borrowed_input_never_keeps_human_control_and_unknown_native_input_never_settles() {
        let now = Instant::now();
        let (mut registry, _, human, external) = native_fixture(now);
        let authority = registry.authorize("pty", &human["control"], 9).unwrap();
        let claim = conditional_at(
            &mut registry,
            &external,
            json!({"kind":"human-idle","idle_ms":0,"warning_ms":1}),
            now,
        );
        consent_at(&mut registry, &human, &claim, 1, now);
        let revision = registry.snapshot_at("pty", now)["activity_revision"].clone();
        authority.activity_at(9, now).unwrap();
        assert_eq!(
            registry.snapshot_at("pty", now)["activity_revision"],
            revision
        );
        authority.forward_native_at(now).unwrap();
        let later = now + Duration::from_secs(60);
        let waiting = method(&mut registry, &claim["claim"], "read", later);
        assert_eq!(waiting["phase"], "waiting-settlement");
        let call = &waiting["ownership"]["unsettled"][0];
        assert_eq!(call["kind"], "native-input");
        authority.settle_at(call["id"].as_str().unwrap(), "applied", later);
        authority.settle_at(call["id"].as_str().unwrap(), "rejected", later);
        assert_eq!(
            method(&mut registry, &claim["claim"], "read", later)["phase"],
            "waiting-settlement"
        );
        authority.activity_at(1, later).unwrap();
        assert_eq!(
            method(&mut registry, &claim["claim"], "read", later)["phase"],
            "aborted-by-owner"
        );
        authority.check().unwrap();
        assert_eq!(
            registry.snapshot_at("pty", later)["unsettled"][0]["outcome"],
            "unknown"
        );
    }

    #[test]
    fn conditional_consent_requires_reported_known_owner_facts_without_revoking_control() {
        for when in [
            json!({"kind":"views-backgrounded","scope":"owner"}),
            json!({"kind":"views-backgrounded","scope":"all"}),
            json!({"kind":"human-idle","idle_ms":0,"warning_ms":1}),
        ] {
            let now = Instant::now();
            let mut registry = Registry::new();
            registry.add_target("pty");
            let native = register(&mut registry, "attachment", 1, now);
            let human = acquire(&mut registry, &native["controller"], "pty", now);
            let authority = registry.authorize("pty", &human["control"], 1).unwrap();
            let external = register(&mut registry, "external", 2, now);
            let eligibility = if when["kind"] == "human-idle" {
                "eligible"
            } else {
                "unknown"
            };
            let claim = conditional_at(&mut registry, &external, when, now);
            for reported in [false, true] {
                if reported {
                    // Zero is a valid first report, not proof that a view exists.
                    view_at(&mut registry, &native, 1, 0, json!("pty"), Value::Null, now);
                }
                let displayed = registry.snapshot_at("pty", now);
                assert_eq!(displayed["views"][0]["reported"], reported);
                assert_eq!(displayed["views"][0]["serial"], 0);
                let conditional = &displayed["reservation"]["conditional"];
                assert_eq!(conditional["eligibility"], eligibility);
                assert_eq!(conditional["ready_for_consent"], false);
                assert_eq!(conditional["consent"], "awaiting");
                let consent = consent_request(&human["control"], &claim, &displayed, 0);
                error(
                    registry.handle_at(&consent, 1, now),
                    "condition-unavailable",
                );
                let pending = method(&mut registry, &claim["claim"], "read", now);
                assert_eq!(pending["phase"], "watching");
                assert_eq!(pending["ownership"]["observed"], displayed["observed"]);
                assert_eq!(
                    pending["ownership"]["reservation"]["conditional"]["consent"],
                    "awaiting"
                );
                assert!(pending.get("offer").is_none());
                assert!(pending.get("control").is_none());
                let current = method(&mut registry, &human["control"], "read", now);
                assert_eq!(current["control"], human["control"]);
                assert_eq!(
                    current["ownership"]["observed"]["generation"],
                    human["generation"]
                );
                authority.check().unwrap();
            }
        }
    }

    #[test]
    fn conditional_consent_distinguishes_changed_reports_from_current_readiness() {
        for (focused, eligibility, rejection) in [
            (Value::Null, "unknown", Some("condition-unavailable")),
            (json!(true), "ineligible", Some("not-ready")),
            (json!(false), "eligible", None),
        ] {
            let now = Instant::now();
            let (mut registry, native, human, external) = native_fixture(now);
            let authority = registry.authorize("pty", &human["control"], 1).unwrap();
            view_at(
                &mut registry,
                &native,
                1,
                2,
                json!("pty"),
                json!(false),
                now,
            );
            let claim = conditional_at(
                &mut registry,
                &external,
                json!({"kind":"views-backgrounded","scope":"owner"}),
                now,
            );
            let displayed = registry.snapshot_at("pty", now);
            assert_eq!(
                displayed["reservation"]["conditional"]["ready_for_consent"],
                true
            );
            let stale = consent_request(&human["control"], &claim, &displayed, 2);
            view_at(&mut registry, &native, 1, 3, json!("pty"), focused, now);
            let current = registry.snapshot_at("pty", now);
            let conditional = &current["reservation"]["conditional"];
            assert_eq!(conditional["eligibility"], eligibility);
            assert_eq!(conditional["ready_for_consent"], rejection.is_none());
            error(registry.handle_at(&stale, 1, now), "stale-view");
            let fresh = consent_request(&human["control"], &claim, &current, 3);
            error(registry.handle_at(&fresh, 9, now), "not-native-connection");
            if let Some(code) = rejection {
                error(registry.handle_at(&fresh, 1, now), code);
            } else {
                registry.handle_at(&fresh, 1, now).unwrap();
            }
            let pending = method(&mut registry, &claim["claim"], "read", now);
            assert_eq!(
                pending["phase"],
                if rejection.is_none() {
                    "offered"
                } else {
                    "watching"
                }
            );
            assert_eq!(pending.get("offer").is_some(), rejection.is_none());
            assert!(pending.get("control").is_none());
            assert_eq!(
                pending["ownership"]["reservation"]["conditional"]["consent"],
                if rejection.is_none() {
                    "recorded"
                } else {
                    "awaiting"
                }
            );
            assert_eq!(
                pending["ownership"]["reservation"]["conditional"]["ready_for_consent"],
                false
            );
            assert_eq!(
                method(&mut registry, &human["control"], "read", now)["control"],
                human["control"]
            );
            assert_eq!(
                pending["ownership"]["observed"]["generation"],
                human["generation"]
            );
            authority.check().unwrap();
            if rejection.is_none() {
                let grant = method(&mut registry, &pending["offer"], "accept", now);
                assert_eq!(grant["mode"], "safe");
                assert_ne!(grant["control"], human["control"]);
                error(authority.check(), "stale-handle");
            }
        }
    }

    #[test]
    fn background_scope_is_explicit_and_unknown_membership_retracts_all_scope_offers() {
        let now = Instant::now();
        let (mut registry, native, human, external) = native_fixture(now);
        let observer = register(&mut registry, "attachment", 3, now);
        view_at(
            &mut registry,
            &native,
            1,
            2,
            json!("pty"),
            json!(false),
            now,
        );
        let claim = conditional_at(
            &mut registry,
            &external,
            json!({"kind":"views-backgrounded","scope":"all"}),
            now,
        );
        assert_eq!(
            claim["ownership"]["reservation"]["conditional"]["eligibility"],
            "unknown"
        );
        assert_eq!(
            claim["ownership"]["reservation"]["conditional"]["clock"],
            json!({"kind":"none","remaining_ms":null})
        );
        let unavailable = consent_request(&human["control"], &claim, &claim["ownership"], 2);
        error(
            registry.handle_at(&unavailable, 1, now),
            "condition-unavailable",
        );
        view_at(
            &mut registry,
            &observer,
            3,
            1,
            json!("pty"),
            json!(false),
            now,
        );
        consent_at(&mut registry, &human, &claim, 2, now);
        let first = method(&mut registry, &claim["claim"], "read", now);
        assert_eq!(first["phase"], "offered");
        let joined = register(&mut registry, "attachment", 4, now);
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle",
            "handle":first["offer"],"method":"accept"}),
                2,
                now,
            ),
            "stale-offer",
        );
        let unknown = method(&mut registry, &claim["claim"], "read", now);
        assert_eq!(unknown["phase"], "watching");
        assert_eq!(
            unknown["ownership"]["reservation"]["conditional"]["consent"],
            "awaiting"
        );
        assert_eq!(
            unknown["ownership"]["reservation"]["conditional"]["eligibility"],
            "unknown"
        );
        view_at(&mut registry, &joined, 4, 1, json!("pty"), json!(true), now);
        assert_eq!(
            registry.snapshot_at("pty", now)["reservation"]["conditional"]["eligibility"],
            "ineligible"
        );
        // Departure is not owner consent. It restores readiness, never an offer.
        registry.depart_at(4, now);
        let watching = method(&mut registry, &claim["claim"], "read", now);
        assert_eq!(watching["phase"], "watching");
        assert_eq!(
            watching["ownership"]["reservation"]["conditional"]["ready_for_consent"],
            true
        );
        consent_at(&mut registry, &human, &claim, 2, now);
        let second = method(&mut registry, &claim["claim"], "read", now);
        assert_ne!(second["offer"], first["offer"]);
        view_at(
            &mut registry,
            &observer,
            3,
            2,
            json!("pty"),
            Value::Null,
            now,
        );
        assert_eq!(
            method(&mut registry, &claim["claim"], "read", now)["phase"],
            "watching"
        );
        assert_eq!(
            method(&mut registry, &human["control"], "read", now)["control"],
            human["control"]
        );
    }

    #[test]
    fn owner_scope_ignores_unrelated_views_but_picker_and_unknown_owner_never_imply_background() {
        let now = Instant::now();
        let (mut registry, native, human, external) = native_fixture(now);
        let claim = conditional_at(
            &mut registry,
            &external,
            json!({"kind":"views-backgrounded","scope":"owner"}),
            now,
        );
        assert_eq!(
            claim["ownership"]["reservation"]["conditional"]["eligibility"],
            "ineligible"
        );
        // A picker does not change selected terminal or outer focus.
        view_at(&mut registry, &native, 1, 2, json!("pty"), json!(true), now);
        assert_eq!(
            registry.snapshot_at("pty", now)["reservation"]["conditional"]["eligibility"],
            "ineligible"
        );
        view_at(&mut registry, &native, 1, 3, json!("pty"), Value::Null, now);
        assert_eq!(
            registry.snapshot_at("pty", now)["reservation"]["conditional"]["eligibility"],
            "unknown"
        );
        view_at(
            &mut registry,
            &native,
            1,
            4,
            json!("pty"),
            json!(false),
            now,
        );
        consent_at(&mut registry, &human, &claim, 4, now);
        let offered = method(&mut registry, &claim["claim"], "read", now);
        register(&mut registry, "attachment", 3, now);
        let still = method(&mut registry, &claim["claim"], "read", now);
        assert_eq!(still["offer"], offered["offer"]);
        method(&mut registry, &offered["offer"], "accept", now);
    }

    #[test]
    fn disconnected_owner_cannot_consent_and_disconnect_does_not_release_or_grant() {
        let now = Instant::now();
        let (mut registry, native, human, external) = native_fixture(now);
        view_at(
            &mut registry,
            &native,
            1,
            2,
            json!("pty"),
            json!(false),
            now,
        );
        let claim = conditional_at(
            &mut registry,
            &external,
            json!({"kind":"views-backgrounded","scope":"owner"}),
            now,
        );
        consent_at(&mut registry, &human, &claim, 2, now);
        let offered = method(&mut registry, &claim["claim"], "read", now);
        registry.depart_at(1, now);
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle",
            "handle":offered["offer"],"method":"accept"}),
                2,
                now,
            ),
            "stale-offer",
        );
        let state = registry.snapshot_at("pty", now);
        assert_eq!(state["owner"], claim["ownership"]["owner"]);
        assert_eq!(state["reservation"]["phase"], "watching");
        assert_eq!(
            state["reservation"]["conditional"]["ready_for_consent"],
            false
        );
        error(
            registry.handle_at(
                &consent_request(&human["control"], &claim, &state, 2),
                1,
                now,
            ),
            "not-native-connection",
        );
        assert_eq!(
            method(&mut registry, &human["control"], "read", now)["control"],
            human["control"]
        );
    }

    #[test]
    fn withdraw_preserves_human_token_release_supersedes_and_handback_needs_no_idle_delay() {
        let now = Instant::now();
        let (mut registry, _, human, external) = native_fixture(now);
        let claim = conditional_at(
            &mut registry,
            &external,
            json!({"kind":"human-idle","idle_ms":0,"warning_ms":1}),
            now,
        );
        error(
            registry.handle_at(&claim_action(&human["control"], "yield-to", &claim), 1, now),
            "wrong-claim-kind",
        );
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle",
            "handle":claim["claim"],"method":"cancel"}),
                2,
                now,
            ),
            "wrong-claim-kind",
        );
        consent_at(&mut registry, &human, &claim, 1, now);
        let later = now + Duration::from_millis(1);
        let offered = method(&mut registry, &claim["claim"], "read", later);
        method(&mut registry, &claim["claim"], "withdraw", later);
        assert_eq!(
            method(&mut registry, &claim["claim"], "read", later)["phase"],
            "withdrawn"
        );
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle",
            "handle":offered["offer"],"method":"accept"}),
                2,
                later,
            ),
            "stale-offer",
        );
        assert_eq!(
            method(&mut registry, &human["control"], "read", later)["control"],
            human["control"]
        );
        let fresh = conditional_at(
            &mut registry,
            &external,
            json!({"kind":"human-idle","idle_ms":60000,"warning_ms":5000}),
            later,
        );
        let released = method(&mut registry, &human["control"], "release", later);
        assert!(released["ownership"]["owner"].is_null());
        assert!(released["ownership"]["reservation"].is_null());
        assert_eq!(
            method(&mut registry, &fresh["claim"], "read", later)["phase"],
            "superseded"
        );
        let handed_back = acquire(&mut registry, &external["controller"], "pty", later);
        assert_eq!(handed_back["mode"], "safe");
        assert_ne!(handed_back["control"], human["control"]);
    }

    #[test]
    fn no_external_request_means_indefinite_native_control_despite_time_and_background() {
        let now = Instant::now();
        let (mut registry, native, human, _) = native_fixture(now);
        view_at(
            &mut registry,
            &native,
            1,
            2,
            json!("pty"),
            json!(false),
            now,
        );
        registry.tick(now + Duration::from_secs(86400 * 365));
        let state = method(
            &mut registry,
            &human["control"],
            "read",
            now + Duration::from_secs(86400 * 365),
        );
        assert_eq!(state["control"], human["control"]);
        assert!(state["ownership"]["reservation"].is_null());
        assert_eq!(
            state["ownership"]["observed"]["generation"],
            human["generation"]
        );
    }

    #[test]
    fn daemon_clock_observations_do_not_regress_when_older_captures_arrive_late() {
        let now = Instant::now();
        let (mut registry, _, human, external) = native_fixture(now);
        let claim = conditional_at(
            &mut registry,
            &external,
            json!({"kind":"human-idle","idle_ms":100,"warning_ms":100}),
            now,
        );
        registry.snapshot_at("pty", now + Duration::from_millis(100));
        let delayed = registry.snapshot_at("pty", now + Duration::from_millis(50));
        assert_eq!(
            delayed["reservation"]["conditional"]["clock"]["remaining_ms"],
            0
        );
        // Consent starts at the current daemon observation, not a stale caller timestamp.
        consent_at(
            &mut registry,
            &human,
            &claim,
            1,
            now + Duration::from_millis(50),
        );
        let warning = registry.snapshot_at("pty", now + Duration::from_millis(150));
        assert_eq!(
            warning["reservation"]["conditional"]["clock"]["remaining_ms"],
            50
        );
        let delayed_warning = registry.snapshot_at("pty", now + Duration::from_millis(125));
        assert_eq!(
            delayed_warning["reservation"]["conditional"]["clock"]["remaining_ms"],
            50
        );
    }

    #[test]
    fn scoped_view_accept_race_never_retargets_or_rolls_back_a_committed_grant() {
        for view_first in [false, true] {
            let now = Instant::now();
            let (mut registry, native, human, external) = native_fixture(now);
            view_at(
                &mut registry,
                &native,
                1,
                2,
                json!("pty"),
                json!(false),
                now,
            );
            let claim = conditional_at(
                &mut registry,
                &external,
                json!({"kind":"views-backgrounded","scope":"all"}),
                now,
            );
            consent_at(&mut registry, &human, &claim, 2, now);
            let offered = method(&mut registry, &claim["claim"], "read", now);
            let accept = json!({"version":1,"op":"control-handle","handle":offered["offer"],"method":"accept"});
            if view_first {
                view_at(&mut registry, &native, 1, 3, json!("pty"), json!(true), now);
                error(registry.handle_at(&accept, 2, now), "stale-offer");
                let pending = method(&mut registry, &claim["claim"], "read", now);
                assert_eq!(pending["phase"], "watching");
                assert_eq!(
                    pending["ownership"]["reservation"]["conditional"]["consent"],
                    "awaiting"
                );
                assert_eq!(
                    method(&mut registry, &human["control"], "read", now)["control"],
                    human["control"]
                );
                error(
                    registry.handle_at(
                        &json!({"version":1,"op":"control-handle",
                    "handle":native["controller"],"method":"view",
                    "view":{"serial":2,"terminal":"pty","focused":false}}),
                        1,
                        now,
                    ),
                    "stale-view",
                );
                assert_eq!(
                    registry.snapshot_at("pty", now)["reservation"]["conditional"]["eligibility"],
                    "ineligible"
                );
            } else {
                let grant = registry.handle_at(&accept, 2, now).unwrap();
                view_at(&mut registry, &native, 1, 3, json!("pty"), json!(true), now);
                assert_eq!(
                    method(&mut registry, &claim["claim"], "read", now)["control"],
                    grant["control"]
                );
                error(
                    registry.handle_at(&claim_action(&human["control"], "keep", &claim), 1, now),
                    "stale-handle",
                );
            }
        }
    }

    #[test]
    fn force_supersedes_conditional_intent_without_old_offer_or_consent_reviving_it() {
        let now = Instant::now();
        let (mut registry, _, human, external) = native_fixture(now);
        let claim = conditional_at(
            &mut registry,
            &external,
            json!({"kind":"human-idle","idle_ms":0,"warning_ms":1}),
            now,
        );
        consent_at(&mut registry, &human, &claim, 1, now);
        let later = now + Duration::from_millis(1);
        let offered = method(&mut registry, &claim["claim"], "read", later);
        let recovery = register(&mut registry, "external", 3, later);
        let observed = registry.snapshot_at("pty", later)["observed"].clone();
        let forced = registry.claim_at(&json!({"version":1,"op":"ownership-claim",
            "controller":recovery["controller"],"terminal":"pty","mode":"force","observed":observed}), later).unwrap();
        assert_eq!(forced["mode"], "forced");
        assert_eq!(
            method(&mut registry, &claim["claim"], "read", later)["phase"],
            "superseded"
        );
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle",
            "handle":offered["offer"],"method":"accept"}),
                2,
                later,
            ),
            "stale-offer",
        );
        error(
            registry.handle_at(
                &consent_request(&human["control"], &claim, &offered["ownership"], 1),
                1,
                later,
            ),
            "stale-handle",
        );
        assert_eq!(
            method(&mut registry, &forced["control"], "read", later)["control"],
            forced["control"]
        );
    }

    #[test]
    fn conditional_offer_revisions_have_bounded_tombstones_under_view_churn() {
        let now = Instant::now();
        let (mut registry, native, human, external) = native_fixture(now);
        view_at(
            &mut registry,
            &native,
            1,
            2,
            json!("pty"),
            json!(false),
            now,
        );
        let claim = conditional_at(
            &mut registry,
            &external,
            json!({"kind":"views-backgrounded","scope":"owner"}),
            now,
        );
        let mut first_offer = Value::Null;
        for revision in 2..(OFFER_HISTORY_LIMIT as u64 + 12) {
            consent_at(&mut registry, &human, &claim, revision, now);
            let offered = method(&mut registry, &claim["claim"], "read", now);
            if revision == 2 {
                first_offer = offered["offer"].clone();
            }
            view_at(
                &mut registry,
                &native,
                1,
                revision + 1,
                json!("pty"),
                json!(false),
                now,
            );
        }
        let target = registry.targets["pty"].lock();
        assert_eq!(
            target.claims[claim["claim_id"].as_str().unwrap()]
                .stale_offers
                .len(),
            OFFER_HISTORY_LIMIT
        );
        drop(target);
        assert_eq!(
            registry
                .handles
                .values()
                .filter(|binding| matches!(binding, Binding::Offer { .. }))
                .count(),
            OFFER_HISTORY_LIMIT
        );
        // Once a tombstone is deliberately reclaimed it remains unusable, not
        // mapped to any later proposal or revived under the same human token.
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle",
            "handle":first_offer,"method":"accept"}),
                2,
                now,
            ),
            "stale-handle",
        );
        assert_eq!(
            method(&mut registry, &human["control"], "read", now)["control"],
            human["control"]
        );
    }

    #[test]
    fn conditional_requests_require_external_requester_and_a_live_native_source_owner() {
        let now = Instant::now();
        let (mut registry, _, human, external) = native_fixture(now);
        let observer = register(&mut registry, "attachment", 3, now);
        let mut request = json!({"version":1,"op":"ownership-claim",
            "controller":observer["controller"],"terminal":"pty","mode":"cooperative",
            "observed":registry.snapshot_at("pty", now)["observed"],
            "when":{"kind":"human-idle","idle_ms":0,"warning_ms":1}});
        error(registry.claim_at(&request, now), "condition-unavailable");
        registry.depart_at(1, now);
        request["controller"] = external["controller"].clone();
        error(registry.claim_at(&request, now), "condition-unavailable");
        assert_eq!(
            method(&mut registry, &human["control"], "read", now)["control"],
            human["control"]
        );
    }

    #[test]
    fn cooperative_request_preserves_authority_and_only_explicit_accept_grants() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add_target("pty");
        let a = register(&mut registry, "external", 1, now);
        let b = register(&mut registry, "external", 2, now);
        let c = register(&mut registry, "external", 3, now);
        let first = acquire(&mut registry, &a["controller"], "pty", now);
        let authority = registry.authorize("pty", &first["control"], 1).unwrap();
        let request = acquire(&mut registry, &b["controller"], "pty", now);
        assert_eq!(request["phase"], "waiting-owner");
        authority.check().unwrap();
        error(
            registry.authorize("pty", &request["claim"], 2),
            "wrong-handle-kind",
        );
        yielded(&mut registry, &first["control"], &request, now);
        error(
            authority.forward_at("never-dispatched", "shell-execute", now),
            "stale-handle",
        );
        let offered = method(&mut registry, &request["claim"], "read", now);
        assert_eq!(offered["phase"], "offered");
        assert!(offered["ownership"]["owner"].is_null());
        error(
            registry.claim_at(
                &json!({"version":1,"op":"ownership-claim","controller":c["controller"],
            "terminal":"pty","observed":observation(&registry,"pty"),"mode":"cooperative",
            "when":{"kind":"now"}}),
                now,
            ),
            "control-reserved",
        );
        let grant = method(&mut registry, &offered["offer"], "accept", now);
        assert_eq!(grant["mode"], "safe");
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle","handle":offered["offer"],
            "method":"accept"}),
                2,
                now,
            ),
            "offer-consumed",
        );
        let recovered = method(&mut registry, &request["claim"], "read", now);
        assert_eq!(recovered["control"], grant["control"]);
        assert_eq!(recovered["current"], true);
    }

    #[test]
    fn dispatch_winning_yield_blocks_offer_until_authoritative_settlement() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add_target("pty");
        let a = register(&mut registry, "external", 1, now);
        let b = register(&mut registry, "external", 2, now);
        let grant = acquire(&mut registry, &a["controller"], "pty", now);
        let authority = registry.authorize("pty", &grant["control"], 1).unwrap();
        authority
            .forward_at("source", "shell-execute", now)
            .unwrap();
        error(
            authority.forward_at("source", "shell-execute", now),
            "invalid-request",
        );
        let claim = acquire(&mut registry, &b["controller"], "pty", now);
        yielded(&mut registry, &grant["control"], &claim, now);
        let waiting = method(&mut registry, &claim["claim"], "read", now);
        assert_eq!(waiting["phase"], "waiting-settlement");
        assert_eq!(waiting["blockers"][0]["call"]["id"], "source");
        authority.settle_at("source", "unknown", now + Duration::from_secs(1));
        registry.tick(now + Duration::from_secs(30));
        assert_eq!(
            method(&mut registry, &claim["claim"], "read", now)["phase"],
            "waiting-settlement"
        );
        authority.settle_at("source", "applied", now + Duration::from_secs(31));
        let offered = method(
            &mut registry,
            &claim["claim"],
            "read",
            now + Duration::from_secs(31),
        );
        assert_eq!(offered["phase"], "offered");
        assert!(
            offered["ownership"]["unsettled"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let events = registry.take_events();
        assert!(
            events
                .iter()
                .any(|(kind, _, data)| kind == "control-call-settled"
                    && data["outcome"] == "applied")
        );
    }

    #[test]
    fn native_stream_is_one_unknown_batch_and_never_a_timer_fence() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add_target("pty");
        let a = register(&mut registry, "attachment", 1, now);
        let b = register(&mut registry, "external", 2, now);
        let grant = acquire(&mut registry, &a["controller"], "pty", now);
        let authority = registry.authorize("pty", &grant["control"], 1).unwrap();
        for index in 0..1000 {
            authority
                .forward_native_at(now + Duration::from_millis(index))
                .unwrap();
        }
        let claim = acquire(&mut registry, &b["controller"], "pty", now);
        yielded(&mut registry, &grant["control"], &claim, now);
        registry.tick(now + Duration::from_secs(300));
        let state = method(
            &mut registry,
            &claim["claim"],
            "read",
            now + Duration::from_secs(300),
        );
        assert_eq!(state["phase"], "waiting-settlement");
        let calls = state["ownership"]["unsettled"].as_array().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["kind"], "native-input");
        assert_eq!(calls[0]["outcome"], "unknown");
    }

    #[test]
    fn cancel_first_revokes_owner_grants_nobody_and_prior_owner_resumes_fresh() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add_target("pty");
        let a = register(&mut registry, "external", 1, now);
        let b = register(&mut registry, "external", 2, now);
        let grant = acquire(&mut registry, &a["controller"], "pty", now);
        let authority = registry.authorize("pty", &grant["control"], 1).unwrap();
        authority.forward_at("old", "agent-mutation", now).unwrap();
        let claim = acquire(&mut registry, &b["controller"], "pty", now);
        method(&mut registry, &claim["claim"], "cancel", now);
        error(authority.check(), "stale-handle");
        assert!(registry.snapshot("pty")["owner"].is_null());
        assert!(registry.snapshot("pty")["reservation"].is_null());
        let resumed = acquire(&mut registry, &a["controller"], "pty", now);
        assert_eq!(resumed["mode"], "resumed");
        assert_ne!(resumed["control"], grant["control"]);
        assert_eq!(resumed["unsettled"][0]["id"], "old");
        let new_authority = registry.authorize("pty", &resumed["control"], 1).unwrap();
        authority.settle_at("old", "rejected", now);
        new_authority.check().unwrap();
        assert!(
            registry.snapshot("pty")["unsettled"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn accept_first_makes_cancel_terminal_without_releasing_the_winner() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add_target("pty");
        let a = register(&mut registry, "external", 1, now);
        let b = register(&mut registry, "external", 2, now);
        let grant = acquire(&mut registry, &a["controller"], "pty", now);
        let claim = acquire(&mut registry, &b["controller"], "pty", now);
        yielded(&mut registry, &grant["control"], &claim, now);
        let offered = method(&mut registry, &claim["claim"], "read", now);
        let winner = method(&mut registry, &offered["offer"], "accept", now);
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle","handle":claim["claim"],
            "method":"cancel"}),
                2,
                now,
            ),
            "claim-completed",
        );
        registry.authorize("pty", &winner["control"], 2).unwrap();
    }

    #[test]
    fn exact_force_receipts_supersede_stranded_offer_and_late_calls_only_settle_old_work() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add_target("pty");
        let a = register(&mut registry, "external", 1, now);
        let b = register(&mut registry, "external", 2, now);
        let c = register(&mut registry, "external", 3, now);
        let first = acquire(&mut registry, &a["controller"], "pty", now);
        let old = registry.authorize("pty", &first["control"], 1).unwrap();
        old.forward_at("old", "shell-execute", now).unwrap();
        let stale = observation(&registry, "pty");
        let claim = acquire(&mut registry, &b["controller"], "pty", now);
        error(
            registry.claim_at(
                &json!({"version":1,"op":"ownership-claim","controller":c["controller"],
            "terminal":"pty","observed":stale,"mode":"force"}),
                now,
            ),
            "stale-observation",
        );
        let forced = registry
            .claim_at(
                &json!({"version":1,"op":"ownership-claim","controller":c["controller"],
            "terminal":"pty","observed":observation(&registry,"pty"),"mode":"force"}),
                now,
            )
            .unwrap();
        assert_eq!(forced["mode"], "forced");
        assert_eq!(forced["warning"]["calls"][0]["id"], "old");
        assert_eq!(
            method(&mut registry, &claim["claim"], "read", now)["phase"],
            "superseded"
        );
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle","handle":claim["claim"],
            "method":"force","observed":observation(&registry,"pty")}),
                2,
                now,
            ),
            "claim-completed",
        );
        let current = registry.authorize("pty", &forced["control"], 3).unwrap();
        old.settle_at("old", "applied", now);
        current.check().unwrap();
        assert_eq!(
            registry.snapshot("pty")["owner"]["controller_id"],
            c["controller_id"]
        );
        assert!(
            registry.snapshot("pty")["unsettled"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn force_winning_offer_race_invalidates_offer_without_consuming_it() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add_target("pty");
        let a = register(&mut registry, "external", 1, now);
        let b = register(&mut registry, "external", 2, now);
        let c = register(&mut registry, "external", 3, now);
        let first = acquire(&mut registry, &a["controller"], "pty", now);
        let claim = acquire(&mut registry, &b["controller"], "pty", now);
        yielded(&mut registry, &first["control"], &claim, now);
        let offered = method(&mut registry, &claim["claim"], "read", now);
        let forced = registry
            .claim_at(
                &json!({"version":1,"op":"ownership-claim","controller":c["controller"],
            "terminal":"pty","observed":observation(&registry,"pty"),"mode":"force"}),
                now,
            )
            .unwrap();
        assert_eq!(forced["warning"]["calls"], json!([]));
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle","handle":offered["offer"],
            "method":"accept"}),
                2,
                now,
            ),
            "stale-offer",
        );
        registry.authorize("pty", &forced["control"], 3).unwrap();
    }

    #[test]
    fn original_connection_views_departure_and_activity_are_not_borrowable() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add_target("pty");
        let native = register(&mut registry, "attachment", 7, now);
        let view = json!({"version":1,"op":"control-handle","handle":native["controller"],"method":"view",
            "view":{"serial":2,"terminal":"pty","focused":null}});
        error(registry.handle_at(&view, 8, now), "not-native-connection");
        registry.handle_at(&view, 7, now).unwrap();
        error(registry.handle_at(&view, 7, now), "stale-view");
        let grant = acquire(&mut registry, &native["controller"], "pty", now);
        let authority = registry.authorize("pty", &grant["control"], 8).unwrap();
        let before = registry.snapshot("pty")["activity_revision"].clone();
        authority.activity_at(8, now).unwrap();
        assert_eq!(registry.snapshot("pty")["activity_revision"], before);
        authority.activity_at(7, now).unwrap();
        assert_eq!(
            registry.snapshot("pty")["activity_revision"].as_u64(),
            Some(before.as_u64().unwrap() + 1)
        );
        registry.depart_at(7, now);
        authority.check().unwrap();
        assert_eq!(registry.snapshot("pty")["views"][0]["connected"], false);
        error(registry.handle_at(&view, 7, now), "not-native-connection");
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle","handle":grant["control"],
            "method":"activity"}),
                8,
                now,
            ),
            "not-native-connection",
        );
        let revision = registry.snapshot("pty")["activity_revision"].clone();
        authority.activity_at(7, now).unwrap();
        assert_eq!(registry.snapshot("pty")["activity_revision"], revision);
    }

    #[test]
    fn recovery_is_private_current_and_historical_and_close_never_releases() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add_target("pty");
        let a = register(&mut registry, "external", 1, now);
        let b = register(&mut registry, "external", 2, now);
        let grant = acquire(&mut registry, &a["controller"], "pty", now);
        registry.depart_at(1, now);
        let recovered = method(&mut registry, &a["controller"], "read", now);
        assert_eq!(recovered["controls"][0]["control"], grant["control"]);
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle","handle":a["controller"],
            "method":"close"}),
                1,
                now,
            ),
            "controller-busy",
        );
        let claim = acquire(&mut registry, &b["controller"], "pty", now);
        yielded(&mut registry, &grant["control"], &claim, now);
        let offered = method(&mut registry, &claim["claim"], "read", now);
        let accepted = method(&mut registry, &offered["offer"], "accept", now);
        method(&mut registry, &accepted["control"], "release", now);
        let historical = method(&mut registry, &claim["claim"], "read", now);
        assert_eq!(historical["phase"], "granted");
        assert_eq!(historical["current"], false);
        assert!(historical.get("control").is_none());
        method(&mut registry, &b["controller"], "close", now);
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle","handle":claim["claim"],
            "method":"read"}),
                2,
                now,
            ),
            "stale-handle",
        );
    }

    #[test]
    fn progress_has_exact_three_second_boundary_actual_blockers_and_no_private_refs() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add_target("pty");
        let a = register(&mut registry, "external", 1, now);
        let b = register(&mut registry, "external", 2, now);
        let grant = acquire(&mut registry, &a["controller"], "pty", now);
        let authority = registry.authorize("pty", &grant["control"], 1).unwrap();
        authority
            .forward_at("unsettled", "shell-execute", now)
            .unwrap();
        let claim = acquire(&mut registry, &b["controller"], "pty", now);
        registry.take_events();
        registry.tick(now + Duration::from_millis(2999));
        assert!(registry.take_events().is_empty());
        registry.tick(now + Duration::from_millis(3000));
        let events = registry.take_events();
        assert_eq!(events.len(), 1);
        let (kind, terminal, data) = &events[0];
        assert_eq!(kind, "control-progress");
        assert_eq!(terminal, "pty");
        assert_eq!(data["claim_id"], claim["claim_id"]);
        assert_eq!(
            data["blockers"][0],
            json!({"kind":"owner","elapsed_ms":3000})
        );
        assert_eq!(data["blockers"][1]["call"]["elapsed_ms"], 3000);
        fn contains_ref(value: &Value) -> bool {
            match value {
                Value::Object(object) => {
                    object.contains_key("ref") || object.values().any(contains_ref)
                }
                Value::Array(array) => array.iter().any(contains_ref),
                _ => false,
            }
        }
        assert!(!contains_ref(data));
        assert!(!contains_ref(&registry.snapshot("pty")));
        authority.check().unwrap();
    }

    #[test]
    fn minted_kind_and_lifetime_cannot_be_changed_by_json_and_conditions_fail_explicitly() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add_target("pty");
        let a = register(&mut registry, "external", 1, now);
        let forged = json!({"kind":"control","ref":a["controller"]["ref"]});
        error(registry.authorize("pty", &forged, 1), "wrong-handle-kind");
        error(
            registry.claim_at(
                &json!({"version":1,"op":"ownership-claim","controller":a["controller"],
            "terminal":"pty","observed":observation(&registry,"pty"),"mode":"cooperative",
            "when":{"kind":"human-idle","idle_ms":0,"warning_ms":1}}),
                now,
            ),
            "condition-unavailable",
        );
        assert!(registry.snapshot("pty")["owner"].is_null());
        let grant = acquire(&mut registry, &a["controller"], "pty", now);
        let authority = registry.authorize("pty", &grant["control"], 1).unwrap();
        registry.end_target("pty");
        error(authority.check(), "target-ended");
        registry.add_target("pty");
        error(
            registry.authorize("pty", &grant["control"], 1),
            "stale-handle",
        );
        let replacement = acquire(&mut registry, &a["controller"], "pty", now);
        assert_ne!(replacement["control"], grant["control"]);
        error(
            authority.forward_at("old-lifetime", "shell-execute", now),
            "target-ended",
        );
    }

    #[test]
    fn claim_force_keeps_generation_specific_calls_and_recovery_is_not_reacquisition() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add_target("pty");
        let a = register(&mut registry, "external", 1, now);
        let b = register(&mut registry, "external", 2, now);
        let first = acquire(&mut registry, &a["controller"], "pty", now);
        let old = registry.authorize("pty", &first["control"], 1).unwrap();
        old.forward_at("operation", "agent-mutation", now).unwrap();
        let claim = acquire(&mut registry, &b["controller"], "pty", now);
        let forced = registry
            .handle_at(
                &json!({"version":1,"op":"control-handle","handle":claim["claim"],
            "method":"force","observed":observation(&registry,"pty")}),
                2,
                now,
            )
            .unwrap();
        assert_eq!(forced["warning"]["calls"].as_array().unwrap().len(), 1);
        let current = registry.authorize("pty", &forced["control"], 2).unwrap();
        current
            .forward_at("operation", "agent-mutation", now + Duration::from_secs(1))
            .unwrap();
        old.settle_at("operation", "applied", now + Duration::from_secs(2));
        old.settle_at("operation", "unknown", now + Duration::from_secs(3));
        let state = registry.targets["pty"]
            .lock()
            .snapshot(now + Duration::from_secs(3));
        assert_eq!(state["unsettled"].as_array().unwrap().len(), 1);
        assert_eq!(state["unsettled"][0]["controller_id"], b["controller_id"]);
        assert_eq!(state["unsettled"][0]["generation"], forced["generation"]);
        assert_eq!(state["unsettled"][0]["outcome"], "pending");
        let recovered = method(
            &mut registry,
            &claim["claim"],
            "read",
            now + Duration::from_secs(3),
        );
        assert_eq!(recovered["control"], forced["control"]);
        assert_eq!(recovered["grant_mode"], "forced");
        current.check().unwrap();
        current.settle_at("operation", "rejected", now + Duration::from_secs(4));
        error(
            current.forward_at("operation", "agent-mutation", now + Duration::from_secs(4)),
            "invalid-request",
        );
        assert!(
            registry.snapshot("pty")["unsettled"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn offered_cancel_wins_and_cannot_later_cancel_a_fresh_acquisition() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add_target("pty");
        let a = register(&mut registry, "external", 1, now);
        let b = register(&mut registry, "external", 2, now);
        let first = acquire(&mut registry, &a["controller"], "pty", now);
        let claim = acquire(&mut registry, &b["controller"], "pty", now);
        yielded(&mut registry, &first["control"], &claim, now);
        let offered = method(&mut registry, &claim["claim"], "read", now);
        method(&mut registry, &claim["claim"], "cancel", now);
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle","handle":offered["offer"],
            "method":"accept"}),
                2,
                now,
            ),
            "stale-offer",
        );
        assert!(registry.snapshot("pty")["owner"].is_null());
        let fresh = acquire(&mut registry, &a["controller"], "pty", now);
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle","handle":claim["claim"],
            "method":"cancel"}),
                2,
                now,
            ),
            "claim-completed",
        );
        registry.authorize("pty", &fresh["control"], 1).unwrap();
    }

    #[test]
    fn target_end_retains_unknown_calls_but_never_offers_after_late_settlement() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add_target("pty");
        let a = register(&mut registry, "external", 1, now);
        let b = register(&mut registry, "external", 2, now);
        let first = acquire(&mut registry, &a["controller"], "pty", now);
        let authority = registry.authorize("pty", &first["control"], 1).unwrap();
        authority
            .forward_at("source", "shell-execute", now)
            .unwrap();
        authority.settle_at("source", "unknown", now);
        let claim = acquire(&mut registry, &b["controller"], "pty", now);
        registry.end_target("pty");
        let ended = method(&mut registry, &claim["claim"], "read", now);
        assert_eq!(ended["phase"], "target-ended");
        assert_eq!(ended["ownership"]["unsettled"][0]["outcome"], "unknown");
        assert!(ended.get("offer").is_none());
        authority.settle_at("source", "applied", now + Duration::from_secs(1));
        registry.tick(now + Duration::from_secs(30));
        let ended = method(
            &mut registry,
            &claim["claim"],
            "read",
            now + Duration::from_secs(30),
        );
        assert_eq!(ended["phase"], "target-ended");
        assert!(ended["ownership"]["owner"].is_null());
        assert!(
            ended["ownership"]["unsettled"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(ended.get("offer").is_none());
    }

    #[test]
    fn admission_and_view_changes_are_not_human_activity_and_unknown_fields_fail_closed() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add_target("pty");
        error(
            registry.create_at(
                &json!({"version":1,"op":"controller-create","role":"attachment",
            "label":"human"}),
                7,
                now,
            ),
            "invalid-request",
        );
        let native = register(&mut registry, "attachment", 7, now);
        let grant = acquire(&mut registry, &native["controller"], "pty", now);
        let before = registry.snapshot("pty")["activity_revision"].clone();
        let authority = registry.authorize("pty", &grant["control"], 7).unwrap();
        authority.check().unwrap();
        registry
            .handle_at(
                &json!({"version":1,"op":"control-handle","handle":native["controller"],
            "method":"view","view":{"serial":0,"terminal":null,"focused":null}}),
                7,
                now,
            )
            .unwrap();
        assert_eq!(registry.snapshot("pty")["activity_revision"], before);
        registry
            .handle_at(
                &json!({"version":1,"op":"control-handle","handle":grant["control"],
            "method":"activity"}),
                7,
                now,
            )
            .unwrap();
        assert_eq!(
            registry.snapshot("pty")["activity_revision"].as_u64(),
            Some(before.as_u64().unwrap() + 1)
        );
        error(
            registry.handle_at(
                &json!({"version":1,"op":"control-handle","handle":grant["control"],
            "method":"release","args":{}}),
                7,
                now,
            ),
            "invalid-request",
        );
        authority.check().unwrap();
        let recovered = method(&mut registry, &native["controller"], "read", now);
        assert_eq!(recovered["controls"][0]["mode"], "safe");
        assert_eq!(recovered["controls"][0]["unsettled"], json!([]));
    }
}
