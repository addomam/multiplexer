//! Durable conversation intent, never durable runtime authority or input.
use super::*;
use crate::recovery::{Agent, Journal, Peer, Project, Record, RecoveryBlock};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt;

/// Bind an opaque agent session to the exact trusted launch namespace, without
/// copying potentially secret argv into the durable recovery metadata.
pub(super) fn launch_hash(command: &[String]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"amux ACP launch identity v1\0");
    for argument in command {
        digest.update((argument.len() as u64).to_le_bytes());
        digest.update(argument.as_bytes());
    }
    let digest = digest.finalize();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push(HEX[usize::from(byte >> 4)] as char);
        text.push(HEX[usize::from(byte & 15)] as char);
    }
    text
}

#[derive(Clone, PartialEq)]
struct Candidate {
    record: Record,
    persisted: bool,
    reopening: bool,
    root_lifetime: Option<String>,
}

fn refresh_observed_peer(
    record: &mut Record,
    pid: i32,
    previous_root: Option<&str>,
    root: Option<&str>,
) {
    if record.launch_pending
        || record.blocked.is_some()
        || previous_root != root
        || record.peer.as_ref().is_none_or(|peer| peer.pid != pid)
    {
        record.peer = Peer::capture(pid);
        record.launch_pending = false;
    }
}

fn same_reopen_intent(current: &Record, expected: &Record) -> bool {
    // Project labels/context can advance while the once-per-start snapshot is
    // waiting. Every launch/settlement field remains part of the exclusion.
    // Exhaustive patterns make new durable fields require an explicit decision.
    let Record {
        id,
        project:
            Project {
                id: project_id,
                name: _,
                context: _,
            },
        name,
        cwd,
        agent,
        session_id,
        session_file,
        launch_hash,
        settings,
        peer,
        launch_pending,
        blocked,
    } = expected;
    current.id == *id
        && current.project.id == *project_id
        && current.name == *name
        && current.cwd == *cwd
        && current.agent == *agent
        && current.session_id == *session_id
        && current.session_file == *session_file
        && current.launch_hash == *launch_hash
        && current.settings == *settings
        && current.peer == *peer
        && current.launch_pending == *launch_pending
        && current.blocked == *blocked
}

struct TargetRecovery {
    revision: u64,
    project_id: String,
    checkpoint: Arc<Mutex<()>>,
}

enum ForgetGuard<'a> {
    Any,
    Snapshot(Option<&'a Candidate>, u64),
}

pub(super) struct RecoveryState {
    journal: Option<Journal>,
    targets: BTreeMap<String, Candidate>,
    reported: BTreeSet<String>,
    runtime: BTreeMap<String, TargetRecovery>,
    projects: BTreeMap<String, (u64, Project)>,
}

impl RecoveryState {
    pub(super) fn open(config: &Path, socket: &Path) -> (Self, Option<anyhow::Error>) {
        let (journal, error) = match Journal::open(config, socket) {
            Ok(journal) => (Some(journal), None),
            Err(error) => (None, Some(error.context("conversation recovery disabled"))),
        };
        (
            Self {
                journal,
                targets: BTreeMap::new(),
                reported: BTreeSet::new(),
                runtime: BTreeMap::new(),
                projects: BTreeMap::new(),
            },
            error,
        )
    }

    fn remember_project(&mut self, sequence: u64, project: Project) {
        if let Some((previous, saved)) = self.projects.get_mut(&project.id) {
            if sequence >= *previous {
                *previous = sequence;
                *saved = project;
            }
        } else {
            self.projects
                .insert(project.id.clone(), (sequence, project));
        }
    }

    fn matches(&self, id: &str, candidate: Option<&Candidate>, revision: u64) -> bool {
        self.runtime
            .get(id)
            .is_some_and(|target| target.revision == revision)
            && self.targets.get(id) == candidate
    }

    fn remove_runtime(&mut self, id: &str) {
        if let Some(target) = self.runtime.remove(id) {
            self.prune_project(&target.project_id);
        }
    }

    fn prune_project(&mut self, id: &str) {
        // Retained failed-to-reopen records still need sequence-qualified
        // project metadata, even after their last active target disappears.
        if !self.runtime.values().any(|target| target.project_id == id)
            && !self
                .journal
                .as_ref()
                .is_some_and(|journal| journal.has_project(id))
        {
            self.projects.remove(id);
        }
    }

    pub(super) fn agent_store(&self) -> Option<PathBuf> {
        self.journal.as_ref().map(Journal::agent_store)
    }
}

fn reported_settings(configuration: &Value) -> Value {
    let reported = &configuration["reported"];
    let reported_options = reported["options"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut categories = [0_usize; 3];
    for option in reported_options {
        match option["category"].as_str() {
            Some("model") => categories[0] += 1,
            Some("mode") => categories[1] += 1,
            Some("thought_level") => categories[2] += 1,
            _ => {}
        }
    }
    let mut settings = serde_json::Map::new();
    for key in ["model", "mode", "effort"] {
        // Model resolution rejects an ambiguous modern category before legacy
        // fallback. Mode resolution can still use genuine legacy offerings.
        if key == "model" && categories[0] > 1 {
            continue;
        }
        if let Some(value) = reported[key]["current"].as_str() {
            settings.insert(key.to_owned(), json!(value));
        }
    }
    let mut options = serde_json::Map::new();
    for option in reported_options {
        if option["type"] != "select" {
            continue;
        }
        let semantic = match option["category"].as_str() {
            Some("model") if categories[0] == 1 => reported["model"]["current"].as_str(),
            Some("mode") if categories[1] == 1 => reported["mode"]["current"].as_str(),
            Some("thought_level") if categories[2] == 1 => reported["effort"]["current"].as_str(),
            _ => None,
        };
        if semantic.is_some_and(|value| option["current"].as_str() == Some(value)) {
            continue;
        }
        let (Some(id), Some(current)) = (option["id"].as_str(), option["current"].as_str()) else {
            continue;
        };
        if option["offered"]
            .as_array()
            .is_some_and(|offered| offered.iter().any(|choice| choice["id"] == current))
        {
            options.insert(id.to_owned(), json!(current));
        }
    }
    if !options.is_empty() {
        settings.insert("options".into(), options.into());
    }
    settings.into()
}

/// Verify only the bounded agent-native header, never copy/log its transcript.
fn omp_backing_file(path: &Path, session: &str, cwd: &Path) -> Result<()> {
    ensure!(
        path.is_absolute(),
        "recovery-unavailable: OMP session file must be absolute"
    );
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .with_context(|| {
            format!(
                "recovery-unavailable: opening OMP session {}",
                path.display()
            )
        })?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.nlink() == 1 && metadata.uid() == unsafe { libc::geteuid() },
        "recovery-unavailable: unsafe OMP session file"
    );
    let mut line = String::new();
    BufReader::new(file)
        .take(64 * 1024 + 1)
        .read_line(&mut line)?;
    ensure!(
        line.ends_with('\n') && line.len() <= 64 * 1024,
        "recovery-unavailable: invalid OMP session header"
    );
    let header: Value =
        serde_json::from_str(&line).context("recovery-unavailable: invalid OMP session header")?;
    ensure!(
        header["type"] == "session" && header["id"] == session,
        "recovery-unavailable: OMP conversation identity changed"
    );
    ensure!(
        header["cwd"].as_str().map(Path::new) == Some(cwd),
        "recovery-unavailable: OMP conversation directory changed"
    );
    Ok(())
}

fn omp_resume_source(path: &Path) -> Result<String> {
    let path = path
        .to_str()
        .context("recovery-unavailable: non-UTF8 OMP session path")?;
    let mut source = String::with_capacity(path.len() + 64);
    source.push_str("command omp -e \"$AMUX_OMP_EXTENSION\" --resume '");
    for character in path.chars() {
        if character == '\'' {
            source.push_str("'\\''");
        } else {
            source.push(character);
        }
    }
    source.push('\'');
    Ok(source)
}

impl Daemon {
    pub(super) fn recovery_diagnostic(&self, target: Option<&str>, error: &anyhow::Error) {
        let message = format!("{error:#}");
        let mut state = self.state.lock();
        Self::diagnostic(
            &mut state,
            "recovery",
            target,
            json!({"kind":"recovery","message":diagnostic_text(&message),"target":target}),
        );
        self.changed.notify_all();
    }

    fn recovery_error_once(&self, target: &str, error: anyhow::Error) {
        if self.recovery.lock().reported.insert(target.to_owned()) {
            self.recovery_diagnostic(Some(target), &error);
        }
    }

    fn recovery_project(&self, project: &str) -> Option<(u64, Project)> {
        let state = self.state.lock();
        let project = state.store.projects.get(project)?;
        Some((
            state.seq,
            Project {
                id: project.id.clone(),
                name: project.name.clone(),
                context: project.context.clone(),
            },
        ))
    }

    pub(super) fn mark_reopen(&self, record: &Record, pending: bool) -> Result<()> {
        let mut recovery = self.recovery.lock();
        let journal = recovery
            .journal
            .as_mut()
            .context("recovery-unavailable: journal disabled")?;
        let mut records = journal.records();
        let Some(current) = records.iter_mut().find(|saved| saved.id == record.id) else {
            if !pending {
                return Ok(());
            }
            anyhow::bail!("recovery-unavailable: saved intent changed before launch");
        };
        if pending {
            ensure!(
                same_reopen_intent(current, record),
                "recovery-unavailable: saved intent changed before launch"
            );
        } else if current.peer != record.peer || !current.launch_pending {
            return Ok(());
        }
        current.launch_pending = pending;
        journal.replace(records)?;
        Ok(())
    }

    fn register_recovery_target(
        &self,
        id: &str,
        sequence: u64,
        project: Project,
        record: Option<Record>,
        reopening: bool,
    ) {
        let mut recovery = self.recovery.lock();
        if recovery.journal.is_none() {
            return;
        }
        recovery.runtime.insert(
            id.into(),
            TargetRecovery {
                revision: 0,
                project_id: project.id.clone(),
                checkpoint: Arc::new(Mutex::new(())),
            },
        );
        recovery.remember_project(sequence, project);
        if let Some(mut record) = record {
            if let Some((_, current)) = recovery.projects.get(&record.project.id)
                && record.project != *current
            {
                record.project = current.clone();
            }
            record.launch_pending = reopening;
            recovery.targets.insert(
                id.into(),
                Candidate {
                    record,
                    persisted: false,
                    reopening,
                    root_lifetime: None,
                },
            );
        }
    }

    fn recovery_launch_project(&self, id: &str, cwd: &Path) -> (u64, Project) {
        self.recovery_project(id).unwrap_or_else(|| {
            (
                self.state.lock().seq,
                Project {
                    id: id.into(),
                    name: "unsaved".into(),
                    context: cwd.into(),
                },
            )
        })
    }

    pub(super) fn stage_acp_recovery(
        &self,
        id: &str,
        project_id: &str,
        name: &str,
        cwd: &Path,
        launch: Option<(&str, String)>,
        restored: Option<&Record>,
    ) {
        let (sequence, project) = self.recovery_launch_project(project_id, cwd);
        let record = restored.cloned().or_else(|| {
            launch.map(|(profile, launch_hash)| Record {
                id: Uuid::new_v4().to_string(),
                project: project.clone(),
                name: name.into(),
                cwd: cwd.into(),
                agent: Agent::Acp {
                    profile: profile.into(),
                },
                session_id: String::new(),
                session_file: None,
                launch_hash: Some(launch_hash),
                settings: json!({}),
                peer: None,
                launch_pending: false,
                blocked: None,
            })
        });
        self.register_recovery_target(id, sequence, project, record, restored.is_some());
    }

    pub(super) fn stage_native_recovery(
        &self,
        id: &str,
        project_id: &str,
        name: &str,
        cwd: &Path,
        model: Option<&str>,
        restored: Option<&Record>,
    ) {
        let (sequence, project) = self.recovery_launch_project(project_id, cwd);
        let record = restored.cloned().or_else(|| {
            model.map(|model| Record {
                id: Uuid::new_v4().to_string(),
                project: project.clone(),
                name: name.into(),
                cwd: cwd.into(),
                agent: Agent::Codex {
                    model: model.into(),
                },
                session_id: String::new(),
                session_file: None,
                launch_hash: None,
                settings: json!({}),
                peer: None,
                launch_pending: false,
                blocked: None,
            })
        });
        self.register_recovery_target(id, sequence, project, record, restored.is_some());
    }

    pub(super) fn abandon_recovery_target(&self, id: &str) {
        let mut recovery = self.recovery.lock();
        recovery.targets.remove(id);
        recovery.reported.remove(id);
        recovery.remove_runtime(id);
    }

    pub(super) fn retire_recovery_target(&self, id: &str) -> Result<()> {
        self.erase_recovery(id, true, ForgetGuard::Any).map(|_| ())
    }

    pub(super) fn native_recovery_exited(&self, id: &str) -> Result<()> {
        if self.stopping.load(Ordering::Acquire) || SIGNAL_STOP.load(Ordering::Relaxed) {
            return Ok(());
        }
        if self
            .recovery
            .lock()
            .targets
            .get(id)
            .is_some_and(|candidate| candidate.reopening)
        {
            // An unpublished/not-yet-established recovery failure is not a user
            // closing the old conversation. Preserve its settlement evidence.
            self.abandon_recovery_target(id);
            return Ok(());
        }
        self.retire_recovery_target(id)
    }

    pub(super) fn forget_root_recovery(&self, id: &str, root: &str) -> Result<()> {
        if self.stopping.load(Ordering::Acquire) || SIGNAL_STOP.load(Ordering::Relaxed) {
            return Ok(());
        }
        let Some(checkpoint) = self.recovery_checkpoint_scope(id) else {
            return Ok(());
        };
        let _checkpoint = checkpoint.lock();
        // A selected root can quit before its first candidate is durable.
        // Displaced shutdowns must not erase a newer selected root instead.
        if !self.integration.root_closed(id, root) {
            return Ok(());
        }
        self.erase_recovery(id, false, ForgetGuard::Any).map(|_| ())
    }

    fn erase_recovery(&self, id: &str, retire: bool, guard: ForgetGuard<'_>) -> Result<bool> {
        let mut recovery = self.recovery.lock();
        match guard {
            ForgetGuard::Any => {}
            ForgetGuard::Snapshot(candidate, revision)
                if recovery.matches(id, candidate, revision) => {}
            _ => return Ok(false),
        }
        if retire {
            recovery.remove_runtime(id);
        } else if let Some(target) = recovery.runtime.get_mut(id) {
            target.revision = target.revision.wrapping_add(1);
        }
        let Some(candidate) = recovery.targets.get(id) else {
            return Ok(true);
        };
        let record_id = candidate.record.id.clone();
        if let Some(journal) = &mut recovery.journal {
            let mut records = journal.records();
            records.retain(|record| record.id != record_id);
            journal
                .replace(records)
                .context("conversation closed but its recovery record could not be forgotten")?;
        }
        if let Some(candidate) = recovery.targets.remove(id) {
            recovery.prune_project(&candidate.record.project.id);
        }
        recovery.reported.remove(id);
        Ok(true)
    }

    pub(super) fn sync_recovery_project(&self, id: &str) -> Result<()> {
        let Some((sequence, project)) = self.recovery_project(id) else {
            return Ok(());
        };
        let mut recovery = self.recovery.lock();
        let Some(journal) = recovery.journal.as_ref() else {
            return Ok(());
        };
        let mut records = journal.records();
        if !recovery
            .runtime
            .values()
            .any(|target| target.project_id == id)
            && !records.iter().any(|record| record.project.id == id)
        {
            recovery.projects.remove(id);
            return Ok(());
        }
        recovery.remember_project(sequence, project);
        let RecoveryState {
            journal, projects, ..
        } = &mut *recovery;
        let project = &projects[id].1;
        let journal = journal.as_mut().expect("checked journal");
        for record in &mut records {
            if record.project.id == id && record.project != *project {
                record.project = project.clone();
            }
        }
        journal.replace(records)?;
        Ok(())
    }

    fn block_native_observation(
        &self,
        id: &str,
        candidate: Option<&Candidate>,
        revision: u64,
        root: Option<&str>,
        reason: RecoveryBlock,
    ) {
        if let Some(candidate) = candidate {
            if candidate.record.blocked == Some(reason) {
                let mut recovery = self.recovery.lock();
                if recovery.matches(id, Some(candidate), revision)
                    && let Some(current) = recovery.targets.get_mut(id)
                    && current.root_lifetime.as_deref() != root
                {
                    current.root_lifetime = root.map(str::to_owned);
                }
            } else {
                let mut record = candidate.record.clone();
                record.blocked = Some(reason);
                self.persist_recovery(
                    id,
                    Some(candidate),
                    record,
                    candidate.reopening,
                    revision,
                    root,
                );
            }
        } else if let Err(error) =
            self.erase_recovery(id, false, ForgetGuard::Snapshot(None, revision))
        {
            self.recovery_error_once(id, error);
        }
    }

    fn recovery_checkpoint_scope(&self, id: &str) -> Option<Arc<Mutex<()>>> {
        self.recovery
            .lock()
            .runtime
            .get(id)
            .map(|target| Arc::clone(&target.checkpoint))
    }

    /// Called for changed broker targets/native reports, not from paint/heartbeat timers.
    pub(super) fn checkpoint_recovery(&self, id: &str) {
        self.checkpoint_recovery_inner(id, false);
    }

    pub(super) fn checkpoint_recovery_before_shutdown(&self) {
        let mut targets: BTreeSet<_> = self.recovery.lock().targets.keys().cloned().collect();
        targets.extend(self.state.lock().terminals.keys().cloned());
        for id in targets {
            self.checkpoint_recovery_inner(&id, true);
        }
    }

    fn checkpoint_recovery_inner(&self, id: &str, final_checkpoint: bool) {
        let Some(checkpoint) = self.recovery_checkpoint_scope(id) else {
            return;
        };
        // Observe only after acquiring this target's scope. A competing newer
        // checkpoint must not lose its already-drained dirty notification.
        let _checkpoint = checkpoint.lock();
        if self.stopping.load(Ordering::Acquire)
            || (!final_checkpoint && SIGNAL_STOP.load(Ordering::Relaxed))
        {
            return;
        }
        let (candidate, revision) = {
            let recovery = self.recovery.lock();
            if recovery.journal.is_none() {
                return;
            }
            let Some(target) = recovery.runtime.get(id) else {
                return;
            };
            (recovery.targets.get(id).cloned(), target.revision)
        };
        let (entry, acp_published) = {
            let state = self.state.lock();
            (
                state.terminals.get(id).cloned(),
                state.agent_views.contains_key(id),
            )
        };
        let is_acp = candidate
            .as_ref()
            .is_some_and(|candidate| matches!(candidate.record.agent, Agent::Acp { .. }));
        if (is_acp && !acp_published) || (!is_acp && entry.is_none()) {
            return;
        }
        let observed = match candidate.as_ref().map(|candidate| &candidate.record.agent) {
            Some(Agent::Acp { .. }) => match self.acp.recovery(id) {
                Ok(value) => Some(value),
                Err(error) => {
                    self.recovery_error_once(id, error);
                    return;
                }
            },
            Some(Agent::Codex { .. }) => match self.codex.recovery(id) {
                Ok(value) => value,
                Err(error) => {
                    self.recovery_error_once(id, error);
                    return;
                }
            },
            _ => self.integration.recovery(id),
        };
        let Some(observed) = observed else {
            if candidate.as_ref().is_some_and(|candidate| {
                !candidate.reopening && matches!(candidate.record.agent, Agent::Omp)
            }) && self.integration.state(id)["shell"]["ready"] == true
                && candidate
                    .as_ref()
                    .and_then(|candidate| candidate.record.peer.as_ref())
                    .is_some_and(|peer| matches!(peer.status(), Ok(false)))
                && let Err(error) = self.erase_recovery(
                    id,
                    false,
                    ForgetGuard::Snapshot(candidate.as_ref(), revision),
                )
            {
                self.recovery_error_once(id, error);
            }
            return;
        };
        let root = observed["root_lifetime"].as_str();
        if !is_acp && observed["recoverable"] != true {
            self.block_native_observation(
                id,
                candidate.as_ref(),
                revision,
                root,
                RecoveryBlock::NativeContext,
            );
            self.recovery_error_once(id, anyhow::anyhow!(
                "recovery-unavailable: current native context is not recoverable ({}); previous intent is blocked, not reopened",
                observed["reason"].as_str().unwrap_or("unverified-context")
            ));
            return;
        }
        let ready = !is_acp || observed["lifecycle"] == "ready";
        if is_acp && !ready {
            let reopening = candidate
                .as_ref()
                .is_some_and(|candidate| candidate.reopening);
            if reopening && matches!(observed["lifecycle"].as_str(), Some("failed" | "ended")) {
                self.acp.cleanup(id);
                if self
                    .acp
                    .recovery(id)
                    .is_ok_and(|state| state["cleanup_complete"] == true)
                    && let Some(candidate) = candidate
                        .as_ref()
                        .filter(|candidate| candidate.record.launch_pending)
                {
                    // No process from this failed launch remains. The prior
                    // definitely-gone pin is safe to retain for a future attempt.
                    let mut record = candidate.record.clone();
                    record.launch_pending = false;
                    self.persist_recovery(id, Some(candidate), record, true, revision, None);
                }
                let reason = self
                    .acp
                    .metadata(id)
                    .ok()
                    .and_then(|metadata| {
                        metadata["resource"]["failure"].as_str().map(str::to_owned)
                    })
                    .unwrap_or_else(|| "saved conversation could not be loaded".into());
                self.recovery_error_once(id, anyhow::anyhow!("recovery-unavailable: {reason}; saved conversation retained, no replacement started"));
                return;
            }
            if !reopening {
                return;
            }
        }
        let session = observed["session_id"]
            .as_str()
            .filter(|session| !session.is_empty())
            .or_else(|| {
                candidate
                    .as_ref()
                    .filter(|candidate| candidate.reopening)
                    .map(|candidate| candidate.record.session_id.as_str())
            });
        let Some(session) = session else {
            return;
        };
        if candidate
            .as_ref()
            .is_some_and(|candidate| candidate.reopening && candidate.record.session_id != session)
        {
            self.recovery_error_once(
                id,
                anyhow::anyhow!("recovery-unavailable: reopened conversation identity changed"),
            );
            return;
        }
        let (mut record, was_persisted) = if let Some(candidate) = &candidate {
            (candidate.record.clone(), candidate.persisted)
        } else {
            let Some(entry) = &entry else {
                return;
            };
            let Some((_, project)) = self.recovery_project(&entry.project_id) else {
                return;
            };
            (
                Record {
                    id: Uuid::new_v4().to_string(),
                    project,
                    name: entry.name.clone(),
                    cwd: entry.cwd.clone(),
                    agent: Agent::Omp,
                    session_id: String::new(),
                    session_file: None,
                    launch_hash: None,
                    settings: json!({}),
                    peer: None,
                    launch_pending: false,
                    blocked: None,
                },
                false,
            )
        };
        record.session_id = session.into();
        if matches!(record.agent, Agent::Acp { .. }) {
            record.name = observed["name"].as_str().unwrap_or(&record.name).to_owned();
            if ready {
                record.settings = reported_settings(&observed["configuration"]);
            }
        } else {
            if let Some(entry) = &entry {
                record.name = entry.name.clone();
            }
            record.session_file = observed["session_file"].as_str().map(PathBuf::from);
            if matches!(record.agent, Agent::Omp) {
                let Some(cwd) = observed["cwd"].as_str() else {
                    return;
                };
                record.cwd = cwd.into();
            }
        }
        let Some(pid) = observed["peer_pid"]
            .as_i64()
            .and_then(|pid| i32::try_from(pid).ok())
        else {
            return;
        };
        refresh_observed_peer(
            &mut record,
            pid,
            candidate
                .as_ref()
                .and_then(|candidate| candidate.root_lifetime.as_deref()),
            root,
        );
        if record.peer.is_none() {
            if !is_acp {
                self.block_native_observation(
                    id,
                    candidate.as_ref(),
                    revision,
                    root,
                    RecoveryBlock::ProcessIdentity,
                );
            }
            self.recovery_error_once(
                id,
                anyhow::anyhow!("recovery-unavailable: agent process identity cannot be pinned"),
            );
            return;
        }
        record.blocked = None;
        if was_persisted
            && candidate
                .as_ref()
                .is_some_and(|candidate| candidate.record == record)
        {
            let mut recovery = self.recovery.lock();
            if recovery.matches(id, candidate.as_ref(), revision) {
                if let Some((_, project)) = recovery.projects.get(&record.project.id)
                    && record.project != *project
                {
                    record.project = project.clone();
                }
                if let Some(current) = recovery.targets.get_mut(id) {
                    current.record = record;
                    current.reopening &= !ready;
                    if current.root_lifetime.as_deref() != root {
                        current.root_lifetime = root.map(str::to_owned);
                    }
                }
            }
            return;
        }
        // Heartbeats with unchanged identity perform no transcript/header I/O.
        // Revalidate the backing file before committing changed native intent.
        if matches!(record.agent, Agent::Omp) {
            let Some(file) = &record.session_file else {
                return;
            };
            if let Err(error) = omp_backing_file(file, session, &record.cwd) {
                self.block_native_observation(
                    id,
                    candidate.as_ref(),
                    revision,
                    root,
                    RecoveryBlock::NativeContext,
                );
                self.recovery_error_once(id, error);
                return;
            }
        }
        let reopening = candidate
            .as_ref()
            .is_some_and(|candidate| candidate.reopening)
            && !ready;
        self.persist_recovery(id, candidate.as_ref(), record, reopening, revision, root);
    }

    fn persist_recovery(
        &self,
        id: &str,
        candidate: Option<&Candidate>,
        mut record: Record,
        reopening: bool,
        revision: u64,
        root_lifetime: Option<&str>,
    ) {
        let mut recovery = self.recovery.lock();
        if !recovery.matches(id, candidate, revision) {
            return;
        }
        if let Some((_, project)) = recovery.projects.get(&record.project.id)
            && record.project != *project
        {
            record.project = project.clone();
        }
        let Some(journal) = &mut recovery.journal else {
            return;
        };
        let mut records = journal.records();
        for saved in &mut records {
            if saved.project.id == record.project.id && saved.project != record.project {
                saved.project = record.project.clone();
            }
        }
        if let Some(current) = records.iter_mut().find(|current| current.id == record.id) {
            *current = record.clone();
        } else {
            records.push(record.clone());
        }
        if let Err(error) = journal.replace(records) {
            drop(recovery);
            self.recovery_error_once(id, error.context("saving conversation recovery metadata"));
            return;
        }
        recovery.targets.insert(
            id.into(),
            Candidate {
                record,
                persisted: true,
                reopening,
                root_lifetime: root_lifetime.map(str::to_owned),
            },
        );
        recovery.reported.remove(id);
    }

    pub(super) fn recovery_records(&self) -> Vec<Record> {
        self.recovery
            .lock()
            .journal
            .as_ref()
            .map(Journal::records)
            .unwrap_or_default()
    }

    pub(super) fn reopen_conversations(self: &Arc<Self>, records: Vec<Record>) {
        for record in records {
            if self.stopping.load(Ordering::Acquire) || SIGNAL_STOP.load(Ordering::Relaxed) {
                break;
            }
            if let Err(error) = self.reopen_conversation(&record) {
                self.recovery_diagnostic(Some(&record.id), &error);
            }
        }
    }

    fn reopen_conversation(self: &Arc<Self>, record: &Record) -> Result<()> {
        ensure!(
            record.blocked.is_none(),
            "recovery-unavailable: current native context or process identity was not verified; previous intent is blocked, not reopened"
        );
        ensure!(
            !record.launch_pending,
            "recovery-unavailable: previous reopening process settlement is unknown; refusing a duplicate conversation process"
        );
        let peer = record
            .peer
            .as_ref()
            .context("recovery-unavailable: previous agent process identity was not verified")?;
        ensure!(
            !peer.status().context(
                "recovery-unavailable: cannot determine whether previous agent still exists"
            )?,
            "recovery-unavailable: previous agent still exists; refusing a duplicate conversation process"
        );
        ensure!(
            record.cwd.is_dir(),
            "recovery-unavailable: saved directory {} is missing",
            record.cwd.display()
        );
        {
            let mut state = self.state.lock();
            state
                .store
                .projects
                .entry(record.project.id.clone())
                .or_insert_with(|| projects::Project {
                    id: record.project.id.clone(),
                    name: record.project.name.clone(),
                    context: record.project.context.clone(),
                    saved: false,
                    main_terminal: None,
                    hooks: Vec::new(),
                    recipes: BTreeMap::new(),
                });
        }
        let mut request =
            json!({"version":1,"project":record.project.id,"name":record.name,"cwd":record.cwd});
        match &record.agent {
            Agent::Acp { profile } => {
                request["op"] = json!("acp-open");
                request["agent"] = json!(profile);
                self.acp_open(&request, Some(record))?;
            }
            Agent::Omp => {
                let file = record
                    .session_file
                    .as_ref()
                    .context("recovery-unavailable: OMP session file missing from record")?;
                omp_backing_file(file, &record.session_id, &record.cwd)?;
                request["command"] = json!(omp_resume_source(file)?);
                let context = self.open_context(&request)?;
                self.mark_reopen(record, true)?;
                if let Err(error) = self.open_inner(&request, context, None, Some(record)) {
                    // Every fallible native-open step precedes publication and
                    // application command admission; no requested OMP started.
                    if let Err(rollback) = self.mark_reopen(record, false) {
                        self.recovery_diagnostic(Some(&record.id), &rollback);
                    }
                    return Err(error);
                }
            }
            Agent::Codex { model } => {
                // The prepare API requires a real controller identity. Automatic
                // startup gets its own separately retired authority in open_inner.
                let controller = self.controls.lock().create(&json!({
                    "version":1,"op":"controller-create","role":"external","label":"conversation reopening"
                }), 0)?["controller"].clone();
                request["op"] = json!("agent-open");
                request["controller"] = controller.clone();
                request["model"] = json!(model);
                request["resume_thread"] = json!(record.session_id);
                let result = self.agent_open(&request, 0, Some(record));
                let _ = self.controls.lock().handle(
                    &json!({
                        "version":1,"op":"control-handle","handle":controller,"method":"close"
                    }),
                    0,
                );
                result?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CheckpointSetupRoot(Option<PathBuf>);
    impl Drop for CheckpointSetupRoot {
        fn drop(&mut self) {
            if let Some(root) = &self.0 {
                let _ = fs::remove_dir_all(root);
            }
        }
    }

    struct CheckpointFixture {
        root: PathBuf,
        daemon: Arc<Daemon>,
        terminal: String,
    }

    impl CheckpointFixture {
        fn new() -> Self {
            let root = fs::canonicalize("/tmp")
                .unwrap()
                .join(format!("amux-rscope-{}", Uuid::new_v4().simple()));
            fs::create_dir(&root).unwrap();
            let mut setup = CheckpointSetupRoot(Some(root.clone()));
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            let startup = root.join("startup");
            fs::create_dir(&startup).unwrap();
            // A private, idle owned PTY; no shell hooks invoke the unit-test binary.
            fs::write(startup.join(".zshenv"), "exec /bin/sleep 600\n").unwrap();
            let runtime = root.join("runtime");
            fs::create_dir(&runtime).unwrap();
            let socket = runtime.join("control.sock");
            let config = root.join("config.json");
            let mut store = Store::load(&config, Some(root.join("projects"))).unwrap();
            let project_id = store.unsaved_id();
            store.projects.get_mut(&project_id).unwrap().context = root.clone();
            let (recovery, error) = RecoveryState::open(&config, &socket);
            assert!(error.is_none(), "{error:?}");
            let native_store = recovery.agent_store();
            let (hook_jobs, _receiver) = std::sync::mpsc::sync_channel(64);
            let daemon = Arc::new(Daemon {
                daemon_id: Uuid::new_v4().to_string(),
                state: Mutex::new(State {
                    store,
                    terminals: BTreeMap::new(),
                    agent_views: BTreeMap::new(),
                    opening_terminals: 0,
                    seq: 0,
                    events: VecDeque::new(),
                    event_bytes: 0,
                    diagnostics: Vec::new(),
                    observed: BTreeMap::new(),
                    opening_projects: BTreeSet::new(),
                    detach_generation: 0,
                }),
                controls: Mutex::new(crate::control::Registry::new()),
                pending_launches: Mutex::new(BTreeMap::new()),
                codex: crate::codex::Broker::new(&runtime, native_store.as_deref()).unwrap(),
                acp: crate::acp::Broker::new(),
                changed: Condvar::new(),
                integration: Integration::new(&runtime, &socket, &std::env::current_exe().unwrap())
                    .unwrap(),
                git: GitCache::new(),
                stopping: AtomicBool::new(false),
                startup_pending: AtomicBool::new(false),
                hook_jobs,
                recovery: Mutex::new(recovery),
            });
            let mut fixture = Self {
                root,
                daemon,
                terminal: String::new(),
            };
            setup.0 = None;
            let context = OpenContext {
                admission: fixture.daemon.reserve_terminal().unwrap(),
                project_id,
                cwd: fixture.root.clone(),
                env: BTreeMap::from([
                    ("HOME".into(), fixture.root.to_string_lossy().into_owned()),
                    ("ZDOTDIR".into(), startup.to_string_lossy().into_owned()),
                    ("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into()),
                    ("TERM".into(), "xterm-256color".into()),
                ]),
                freshness: Value::Null,
                command: None,
            };
            let opened = fixture
                .daemon
                .open_inner(&json!({"name":"Native scope"}), context, None, None)
                .unwrap();
            fixture.terminal = opened.terminal["id"].as_str().unwrap().into();
            fixture
        }

        fn header(&self, session: &str) -> PathBuf {
            let path = self.root.join(format!("{session}.jsonl"));
            fs::write(
                &path,
                format!(
                    "{}\n",
                    json!({"type":"session","version":3,"id":session,"cwd":self.root})
                ),
            )
            .unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            path
        }

        fn report(&self, root: &str, session: &str, file: &Path) {
            self.daemon.integration.report(&self.terminal, json!({
                "kind":"agent","instance_id":root,"agent_id":"Main","agent_kind":"main","parent_id":null,
                "agent_family":"omp","activity":"idle","pid":std::process::id(),
                "lifecycle_event":"session_start","conversation_id":session,
                "session_file":file,"session_persisted":true,"cwd":self.root
            })).unwrap();
        }

        fn shutdown_root(&self, root: &str) {
            self.daemon.integration.report(&self.terminal, json!({
                "kind":"agent","instance_id":root,"agent_id":"Main","agent_kind":"main","parent_id":null,
                "agent_family":"omp","activity":"idle","pid":std::process::id(),
                "lifecycle_event":"session_shutdown","shutdown":true
            })).unwrap();
            self.daemon
                .forget_root_recovery(&self.terminal, root)
                .unwrap();
        }

        fn rows(&self) -> Vec<Record> {
            self.daemon
                .recovery
                .lock()
                .journal
                .as_ref()
                .unwrap()
                .records()
        }

        fn record(&self, session: &str, file: PathBuf) -> Record {
            let entry = self.daemon.state.lock().terminals[&self.terminal].clone();
            Record {
                id: Uuid::new_v4().to_string(),
                project: self.daemon.recovery_project(&entry.project_id).unwrap().1,
                name: entry.name,
                cwd: self.root.clone(),
                agent: Agent::Omp,
                session_id: session.into(),
                session_file: Some(file),
                launch_hash: None,
                settings: json!({}),
                peer: Peer::capture(std::process::id() as i32),
                launch_pending: false,
                blocked: None,
            }
        }
    }

    impl Drop for CheckpointFixture {
        fn drop(&mut self) {
            self.daemon.stopping.store(true, Ordering::Release);
            let entries: Vec<_> = self
                .daemon
                .state
                .lock()
                .terminals
                .values()
                .cloned()
                .collect();
            for entry in entries {
                entry.terminal.close();
            }
            self.daemon.acp.shutdown();
            self.daemon.codex.shutdown();
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn project_metadata_can_advance_without_reauthorizing_changed_or_retired_intent() {
        let fixture = CheckpointFixture::new();
        let session = Uuid::new_v4().to_string();
        let file = fixture.header(&session);
        let record = fixture.record(&session, file);
        fixture
            .daemon
            .recovery
            .lock()
            .journal
            .as_mut()
            .unwrap()
            .replace(vec![record.clone()])
            .unwrap();
        let project = {
            let mut state = fixture.daemon.state.lock();
            state.seq += 1;
            let project = state.store.projects.get_mut(&record.project.id).unwrap();
            project.name = "Updated while startup waits".into();
            project.context = fixture.root.join("new-context");
            project.id.clone()
        };
        fixture.daemon.sync_recovery_project(&project).unwrap();
        let current_project = fixture.daemon.recovery_project(&project).unwrap().1;
        fixture.daemon.mark_reopen(&record, true).unwrap();
        assert_eq!(fixture.rows()[0].project, current_project);
        assert!(fixture.rows()[0].launch_pending);
        assert!(
            fixture.daemon.mark_reopen(&record, true).is_err(),
            "the same snapshot must not admit a second pending launch"
        );
        fixture.daemon.stage_native_recovery(
            &fixture.terminal,
            &project,
            &record.name,
            &record.cwd,
            None,
            Some(&record),
        );
        fixture.daemon.mark_reopen(&record, false).unwrap();
        assert_eq!(fixture.rows()[0].project, current_project);
        assert!(!fixture.rows()[0].launch_pending);
        let mut replaced_peer = fixture.rows().pop().unwrap();
        replaced_peer.peer.as_mut().unwrap().start[0] += 1;
        replaced_peer.launch_pending = true;
        fixture
            .daemon
            .recovery
            .lock()
            .journal
            .as_mut()
            .unwrap()
            .replace(vec![replaced_peer.clone()])
            .unwrap();
        assert!(fixture.daemon.mark_reopen(&record, true).is_err());
        fixture.daemon.mark_reopen(&record, false).unwrap();
        assert_eq!(
            fixture.rows(),
            [replaced_peer],
            "a stale rollback must not settle another peer's pending launch"
        );
        fixture
            .daemon
            .retire_recovery_target(&fixture.terminal)
            .unwrap();
        assert!(fixture.daemon.mark_reopen(&record, true).is_err());
        fixture.daemon.mark_reopen(&record, false).unwrap();
        assert!(
            fixture.rows().is_empty(),
            "neither a stale launch nor its rollback may revive retired intent"
        );
    }

    #[test]
    fn reopen_snapshot_rejects_every_non_metadata_intent_and_settlement_change() {
        let fixture = CheckpointFixture::new();
        let session = Uuid::new_v4().to_string();
        let file = fixture.header(&session);
        let record = fixture.record(&session, file);
        fixture
            .daemon
            .recovery
            .lock()
            .journal
            .as_mut()
            .unwrap()
            .replace(vec![record.clone()])
            .unwrap();
        type IntentChange = (&'static str, fn(&mut Record));
        let changes: &[IntentChange] = &[
            ("record ID", |saved| saved.id.push_str("-changed")),
            ("project ID", |saved| saved.project.id.push_str("-changed")),
            ("view name", |saved| saved.name.push_str(" changed")),
            ("cwd", |saved| saved.cwd.push("changed")),
            ("agent", |saved| {
                saved.agent = Agent::Codex {
                    model: "changed".into(),
                };
            }),
            ("session ID", |saved| saved.session_id.push_str("-changed")),
            ("session file", |saved| saved.session_file = None),
            ("launch fingerprint", |saved| {
                saved.launch_hash = Some("a".repeat(64));
            }),
            ("settings", |saved| {
                saved.settings = json!({"effort":"high"})
            }),
            ("missing peer", |saved| saved.peer = None),
            ("peer PID", |saved| saved.peer.as_mut().unwrap().pid += 1),
            ("peer birth", |saved| {
                saved.peer.as_mut().unwrap().start[0] += 1
            }),
            ("peer boot", |saved| {
                saved.peer.as_mut().unwrap().boot.push_str("-changed");
            }),
            ("pending launch", |saved| saved.launch_pending = true),
            ("native block", |saved| {
                saved.blocked = Some(RecoveryBlock::NativeContext);
            }),
            ("identity block", |saved| {
                saved.blocked = Some(RecoveryBlock::ProcessIdentity);
            }),
        ];
        for (field, change) in changes {
            let mut stale = record.clone();
            change(&mut stale);
            assert!(
                fixture.daemon.mark_reopen(&stale, true).is_err(),
                "changed {field} was treated as a metadata-only update"
            );
            assert_eq!(
                fixture.rows(),
                std::slice::from_ref(&record),
                "changed {field}"
            );
        }
    }

    #[test]
    fn queued_native_checkpoints_observe_the_latest_selected_root_after_acquiring_the_scope() {
        let fixture = CheckpointFixture::new();
        let a = Uuid::new_v4().to_string();
        let b = Uuid::new_v4().to_string();
        let a_file = fixture.header(&a);
        let b_file = fixture.header(&b);
        fixture.report("root-a", &a, &a_file);
        let checkpoint = fixture
            .daemon
            .recovery_checkpoint_scope(&fixture.terminal)
            .unwrap();
        let held = checkpoint.lock();
        std::thread::scope(|threads| {
            let (started, entered) = std::sync::mpsc::sync_channel(0);
            let daemon = &fixture.daemon;
            let id = &fixture.terminal;
            let first = threads.spawn(move || {
                started.send(()).unwrap();
                daemon.checkpoint_recovery(id);
            });
            entered.recv_timeout(Duration::from_secs(2)).unwrap();
            fixture.report("root-b", &b, &b_file);
            let second = threads.spawn(move || daemon.checkpoint_recovery(id));
            drop(held);
            first.join().unwrap();
            second.join().unwrap();
        });
        let rows = fixture.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].session_id, b);
        assert!(rows[0].peer.as_ref().unwrap().status().unwrap());
        let before = fixture.daemon.state.lock().terminals.len();
        assert!(fixture.daemon.reopen_conversation(&rows[0]).is_err());
        assert_eq!(fixture.daemon.state.lock().terminals.len(), before);
    }

    #[test]
    fn unrecorded_native_quit_invalidates_an_already_captured_checkpoint() {
        let fixture = CheckpointFixture::new();
        let session = Uuid::new_v4().to_string();
        let file = fixture.header(&session);
        fixture.report("first-root", &session, &file);
        let revision = fixture.daemon.recovery.lock().runtime[&fixture.terminal].revision;
        let record = fixture.record(&session, file);
        fixture.shutdown_root("first-root");
        fixture.daemon.persist_recovery(
            &fixture.terminal,
            None,
            record,
            false,
            revision,
            Some("first-root"),
        );
        assert!(
            fixture.rows().is_empty(),
            "a quit first root became eligible after late validation"
        );
    }

    #[test]
    fn replacing_root_quit_forgets_old_intent_before_its_own_candidate_exists() {
        let fixture = CheckpointFixture::new();
        let a = Uuid::new_v4().to_string();
        let b = Uuid::new_v4().to_string();
        let a_file = fixture.header(&a);
        let b_file = fixture.header(&b);
        fixture.report("old-root", &a, &a_file);
        fixture.daemon.checkpoint_recovery(&fixture.terminal);
        let candidate = fixture.daemon.recovery.lock().targets[&fixture.terminal].clone();
        let revision = fixture.daemon.recovery.lock().runtime[&fixture.terminal].revision;
        fixture.report("replacing-root", &b, &b_file);
        let record = fixture.record(&b, b_file);
        fixture.shutdown_root("replacing-root");
        fixture.daemon.persist_recovery(
            &fixture.terminal,
            Some(&candidate),
            record,
            false,
            revision,
            Some("replacing-root"),
        );
        assert!(
            fixture.rows().is_empty(),
            "quit replacement retained or revived a native context"
        );
        let c = Uuid::new_v4().to_string();
        let c_file = fixture.header(&c);
        fixture.report("fresh-root", &c, &c_file);
        fixture.daemon.checkpoint_recovery(&fixture.terminal);
        fixture.shutdown_root("replacing-root");
        assert_eq!(
            fixture.rows()[0].session_id,
            c,
            "displaced shutdown erased the current native context"
        );
    }

    #[test]
    fn replacing_a_root_refreshes_a_stale_birth_pin_even_when_the_numeric_pid_matches() {
        let pid = std::process::id() as i32;
        let mut stale = Peer::capture(pid).expect("capture the live test process");
        stale.start[0] -= 1;
        assert!(
            !stale.status().unwrap(),
            "the old birth pin must not identify this live process"
        );
        let cwd = std::env::temp_dir();
        let mut record = Record {
            id: "native".into(),
            project: Project {
                id: "project".into(),
                name: "Project".into(),
                context: cwd.clone(),
            },
            name: "Native".into(),
            cwd,
            agent: Agent::Omp,
            session_id: Uuid::new_v4().to_string(),
            session_file: None,
            launch_hash: None,
            settings: json!({}),
            peer: Some(stale),
            launch_pending: false,
            blocked: None,
        };
        refresh_observed_peer(&mut record, pid, Some("old-root"), Some("new-root"));
        assert!(
            record.peer.as_ref().unwrap().status().unwrap(),
            "the new saved pin must exclude automatic duplication of this live process"
        );
    }
}
