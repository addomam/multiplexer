//! Bounded conversation references, never live authority or conversation content.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, CString};
use std::fs::{self, File, Metadata};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_RECORDS: usize = 512;
const MAX_PATH: usize = 4096;
const MAX_ID: usize = 256;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Project {
    pub id: String,
    pub name: String,
    pub context: PathBuf,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum Agent {
    Acp { profile: String },
    Omp,
    Codex { model: String },
}

impl<'de> Deserialize<'de> for Agent {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> std::result::Result<Self, D::Error> {
        // A struct-shaped empty variant rejects fields that Serde's internally
        // tagged unit-variant visitor otherwise ignores.
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
        enum SavedAgent {
            Acp { profile: String },
            Omp {},
            Codex { model: String },
        }
        Ok(match SavedAgent::deserialize(input)? {
            SavedAgent::Acp { profile } => Self::Acp { profile },
            SavedAgent::Omp {} => Self::Omp,
            SavedAgent::Codex { model } => Self::Codex { model },
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Peer {
    pub pid: i32,
    pub start: [u64; 2],
    pub boot: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RecoveryBlock {
    NativeContext,
    ProcessIdentity,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub id: String,
    pub project: Project,
    pub name: String,
    pub cwd: PathBuf,
    pub agent: Agent,
    pub session_id: String,
    pub session_file: Option<PathBuf>,
    pub launch_hash: Option<String>,
    #[serde(deserialize_with = "deserialize_settings")]
    pub settings: Value,
    pub peer: Option<Peer>,
    pub launch_pending: bool,
    pub blocked: Option<RecoveryBlock>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Disk {
    version: u32,
    #[serde(deserialize_with = "deserialize_records")]
    records: Vec<Record>,
}

// Reject duplicate keys even inside the otherwise untyped settings field. Only
// the already-supported ACP choice identities are eligible durable metadata.
fn deserialize_settings<'de, D: serde::Deserializer<'de>>(
    input: D,
) -> std::result::Result<Value, D::Error> {
    struct Choices;
    impl<'de> serde::de::Visitor<'de> for Choices {
        type Value = Value;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a bounded map of unique setting identities")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut input: M,
        ) -> std::result::Result<Value, M::Error> {
            let mut values = serde_json::Map::new();
            while let Some(key) = input.next_key::<String>()? {
                if values.len() >= 64 || !valid_text(&key, MAX_ID) || values.contains_key(&key) {
                    return Err(serde::de::Error::custom(
                        "invalid or duplicate choice identity",
                    ));
                }
                let value = input.next_value::<String>()?;
                if !valid_text(&value, MAX_ID) {
                    return Err(serde::de::Error::custom("invalid choice value"));
                }
                values.insert(key, Value::String(value));
            }
            Ok(Value::Object(values))
        }
    }
    struct Settings;
    impl<'de> serde::de::Visitor<'de> for Settings {
        type Value = Value;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a closed ACP settings object")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut input: M,
        ) -> std::result::Result<Value, M::Error> {
            let mut values = serde_json::Map::new();
            while let Some(key) = input.next_key::<String>()? {
                if values.contains_key(&key) {
                    return Err(serde::de::Error::custom("duplicate setting identity"));
                }
                let value = match key.as_str() {
                    "model" | "mode" | "effort" => {
                        let value = input.next_value::<String>()?;
                        if !valid_text(&value, MAX_ID) {
                            return Err(serde::de::Error::custom("invalid setting identity"));
                        }
                        Value::String(value)
                    }
                    "options" => {
                        struct ChoiceValue(Value);
                        impl<'de> Deserialize<'de> for ChoiceValue {
                            fn deserialize<D: serde::Deserializer<'de>>(
                                input: D,
                            ) -> std::result::Result<Self, D::Error> {
                                input.deserialize_map(Choices).map(Self)
                            }
                        }
                        input.next_value::<ChoiceValue>()?.0
                    }
                    _ => return Err(serde::de::Error::custom("unknown durable setting")),
                };
                values.insert(key, value);
            }
            Ok(Value::Object(values))
        }
    }
    input.deserialize_map(Settings)
}

fn deserialize_records<'de, D: serde::Deserializer<'de>>(
    input: D,
) -> std::result::Result<Vec<Record>, D::Error> {
    struct Records;
    impl<'de> serde::de::Visitor<'de> for Records {
        type Value = Vec<Record>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("at most 512 conversation records")
        }
        fn visit_seq<S: serde::de::SeqAccess<'de>>(
            self,
            mut input: S,
        ) -> std::result::Result<Vec<Record>, S::Error> {
            let mut records = Vec::with_capacity(input.size_hint().unwrap_or(0).min(MAX_RECORDS));
            while let Some(record) = input.next_element::<Record>()? {
                if records.len() == MAX_RECORDS {
                    return Err(serde::de::Error::custom("too many conversation records"));
                }
                records.push(record);
            }
            Ok(records)
        }
    }
    input.deserialize_seq(Records)
}

fn valid_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

fn validate_path(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute() && path.to_str().is_some_and(|text| valid_text(text, MAX_PATH)),
        "recovery-invalid: paths must be bounded absolute UTF-8 paths"
    );
    Ok(())
}

fn valid_boot(boot: &str) -> bool {
    let boot = boot.strip_prefix("darwin:").unwrap_or(boot);
    boot.len() == 36
        && boot.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

fn validate_peer(peer: &Peer) -> Result<()> {
    ensure!(
        peer.pid > 0
            && peer.start[0] > 0
            && peer.start[1] < 1_000_000
            && peer.boot.len() <= 80
            && valid_boot(&peer.boot)
            && (peer.boot.starts_with("darwin:") || peer.start[1] == 0),
        "recovery-invalid: malformed process identity"
    );
    Ok(())
}

fn validate_record(record: &Record) -> Result<()> {
    ensure!(
        valid_text(&record.id, MAX_ID)
            && valid_text(&record.project.id, MAX_ID)
            && valid_text(&record.project.name, MAX_ID)
            && valid_text(&record.name, MAX_ID)
            && valid_text(&record.session_id, 1024),
        "recovery-invalid: missing, oversized or unsafe conversation identity"
    );
    validate_path(&record.project.context)?;
    validate_path(&record.cwd)?;
    if let Some(path) = &record.session_file {
        validate_path(path)?;
    }
    ensure!(
        matches!(&record.agent, Agent::Acp { .. }) || record.launch_hash.is_none(),
        "recovery-invalid: native agent cannot contain an ACP launch identity"
    );
    ensure!(
        record.blocked.is_none() || matches!(&record.agent, Agent::Omp | Agent::Codex { .. }),
        "recovery-invalid: only native intent can have a native observation block"
    );
    match &record.agent {
        Agent::Acp { profile } => {
            ensure!(
                !profile.is_empty()
                    && profile.len() <= 64
                    && profile.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
                    }),
                "recovery-invalid: invalid configured ACP profile identity"
            );
            ensure!(
                record
                    .launch_hash
                    .as_deref()
                    .is_some_and(|hash| hash.len() == 64
                        && hash
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))),
                "recovery-invalid: missing or invalid ACP launch identity"
            );
            // Bound the shape before the existing validator serializes it.
            let settings = record
                .settings
                .as_object()
                .context("recovery-invalid: settings object")?;
            ensure!(settings.len() <= 4, "recovery-invalid: settings budget");
            for (key, value) in settings {
                match key.as_str() {
                    "model" | "mode" | "effort" => ensure!(
                        value.as_str().is_some_and(|text| valid_text(text, MAX_ID)),
                        "recovery-invalid: invalid ACP setting identity"
                    ),
                    "options" => {
                        let options = value
                            .as_object()
                            .context("recovery-invalid: options object")?;
                        ensure!(options.len() <= 64, "recovery-invalid: option budget");
                        ensure!(
                            options.iter().all(|(key, value)| {
                                valid_text(key, MAX_ID)
                                    && value.as_str().is_some_and(|text| valid_text(text, MAX_ID))
                            }),
                            "recovery-invalid: invalid ACP choice identity"
                        );
                    }
                    _ => bail!("recovery-invalid: unknown durable setting"),
                }
            }
            crate::acp::validate_requested_settings(&record.settings)?;
        }
        Agent::Codex { model } => {
            ensure!(
                valid_text(model, MAX_ID),
                "recovery-invalid: invalid Codex model identity"
            );
            ensure!(
                record
                    .settings
                    .as_object()
                    .is_some_and(|value| value.is_empty()),
                "recovery-invalid: native settings must be empty"
            );
        }
        Agent::Omp => ensure!(
            record
                .settings
                .as_object()
                .is_some_and(|value| value.is_empty()),
            "recovery-invalid: native settings must be empty"
        ),
    }
    if let Some(peer) = &record.peer {
        validate_peer(peer)?;
    }
    Ok(())
}

fn conversation(record: &Record) -> (&str, &str, &str) {
    match &record.agent {
        Agent::Acp { .. } => (
            "acp",
            record.launch_hash.as_deref().unwrap_or_default(),
            &record.session_id,
        ),
        Agent::Omp => ("omp", "", &record.session_id),
        Agent::Codex { .. } => ("codex", "", &record.session_id),
    }
}

fn normalize(mut records: Vec<Record>) -> Result<Vec<Record>> {
    ensure!(
        records.len() <= MAX_RECORDS,
        "recovery-invalid: too many conversation records"
    );
    for record in &records {
        validate_record(record)?;
    }
    records.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    records.dedup();
    let mut projects = BTreeMap::new();
    let mut conversations = BTreeSet::new();
    for (index, record) in records.iter().enumerate() {
        ensure!(
            index == 0 || records[index - 1].id != record.id,
            "recovery-invalid: conflicting duplicate record identity"
        );
        ensure!(
            conversations.insert(conversation(record)),
            "recovery-invalid: conflicting duplicate conversation identity"
        );
        if let Some(previous) = projects.insert(&record.project.id, &record.project) {
            ensure!(
                previous == &record.project,
                "recovery-invalid: inconsistent project identity"
            );
        }
    }
    Ok(records)
}

#[cfg(target_os = "linux")]
fn boot_identity() -> Result<String> {
    let file = File::open("/proc/sys/kernel/random/boot_id")
        .context("recovery-peer-unknown: reading Linux boot identity")?;
    let mut boot = String::new();
    file.take(81).read_to_string(&mut boot)?;
    let boot = boot.trim();
    ensure!(
        valid_boot(boot),
        "recovery-peer-unknown: invalid Linux boot identity"
    );
    Ok(boot.to_owned())
}

#[cfg(target_os = "macos")]
fn boot_identity() -> Result<String> {
    // kern.boottime moves when the calendar clock is stepped. It cannot
    // establish that an old process belongs to a different boot.
    let mut boot = [0_u8; 37];
    let mut size = boot.len();
    let result = unsafe {
        libc::sysctlbyname(
            c"kern.bootsessionuuid".as_ptr(),
            boot.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    ensure!(
        result == 0 && size == boot.len() && boot[36] == 0,
        "recovery-peer-unknown: reading Darwin boot session identity"
    );
    let boot = std::str::from_utf8(&boot[..36])
        .context("recovery-peer-unknown: invalid Darwin boot session identity")?;
    let boot = Uuid::parse_str(boot)
        .context("recovery-peer-unknown: invalid Darwin boot session identity")?;
    let identity = format!("darwin:{boot}");
    Ok(identity)
}

// Do not use kill(pid, 0): peer inspection has no signaling side effects and
// inaccessible native metadata must never become permission to spawn a duplicate.
#[cfg(target_os = "linux")]
fn definitely_absent(pid: i32) -> bool {
    fs::symlink_metadata(format!("/proc/{pid}"))
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
}

#[cfg(target_os = "macos")]
fn definitely_absent(pid: i32) -> bool {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    let result = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            1,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    result == 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

fn process_status(
    expected: [u64; 2],
    observed: Option<[u64; 2]>,
    absent: impl FnOnce() -> bool,
) -> Result<bool> {
    if let Some(start) = observed {
        return Ok(start == expected);
    }
    if absent() {
        return Ok(false);
    }
    bail!("recovery-peer-unknown: process identity is unavailable; do not reopen automatically")
}

impl Peer {
    pub(crate) fn capture(pid: i32) -> Option<Self> {
        if pid <= 0 {
            return None;
        }
        let peer = Self {
            pid,
            start: crate::runtime::process_stamp(pid)?,
            boot: boot_identity().ok()?,
        };
        validate_peer(&peer).ok()?;
        Some(peer)
    }

    /// Exact live identity, definitely gone/reused, or an explicitly unknown probe.
    pub(crate) fn status(&self) -> Result<bool> {
        validate_peer(self)?;
        if self.boot != boot_identity()? {
            return Ok(false);
        }
        process_status(self.start, crate::runtime::process_stamp(self.pid), || {
            definitely_absent(self.pid)
        })
    }
}

fn canonical_location(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    match fs::canonicalize(&absolute) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure!(
                !fs::symlink_metadata(&absolute)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink()),
                "recovery-invalid: dangling path alias"
            );
            let parent = absolute
                .parent()
                .context("recovery-invalid: missing parent directory")?;
            Ok(fs::canonicalize(parent)?.join(
                absolute
                    .file_name()
                    .context("recovery-invalid: missing file name")?,
            ))
        }
        Err(error) => Err(error).context("recovery-invalid: resolving durable path"),
    }
}

fn private_file(metadata: &Metadata) -> Result<()> {
    ensure!(
        metadata.is_file()
            && metadata.nlink() == 1
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o7777 == 0o600,
        "recovery-invalid: expected a 0600 regular file owned by this user without hard-link aliases"
    );
    Ok(())
}

fn open_at(directory: &File, name: &CStr, flags: i32) -> Result<File> {
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error())
            .context("recovery-file: opening private metadata");
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn named_file(directory: &File, name: &CStr, metadata: &Metadata) -> Result<()> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error())
            .context("recovery-conflict: metadata name changed");
    }
    let stat = unsafe { stat.assume_init() };
    #[cfg(target_os = "macos")]
    let device = stat.st_dev as u64;
    #[cfg(target_os = "linux")]
    let device = stat.st_dev;
    ensure!(
        device == metadata.dev()
            && stat.st_ino == metadata.ino()
            && stat.st_nlink == 1
            && stat.st_uid == unsafe { libc::geteuid() }
            && stat.st_mode & libc::S_IFMT == libc::S_IFREG
            && stat.st_mode & 0o7777 == 0o600,
        "recovery-conflict: metadata identity or permissions changed"
    );
    Ok(())
}

struct WriterLock(File);
impl Drop for WriterLock {
    fn drop(&mut self) {
        // A PTY fork may inherit this open description until exec; explicitly
        // unlock rather than relying solely on our descriptor being closed.
        loop {
            if unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) } == 0 {
                break;
            }
            if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                break;
            }
        }
    }
}

/// One socket-scoped writer for its entire daemon lifetime. All operations use
/// the held canonical directory descriptor, never follow a metadata-file alias.
pub(crate) struct Journal {
    directory: File,
    lock: WriterLock,
    path: PathBuf,
    name: CString,
    lock_name: CString,
    expected: Option<Vec<u8>>,
    records: Vec<Record>,
    uncertain: bool,
}

impl Journal {
    pub(crate) fn open(config_path: &Path, socket: &Path) -> Result<Self> {
        let config = canonical_location(config_path)?;
        let parent = config
            .parent()
            .context("recovery-invalid: configuration has no parent")?;
        let socket = canonical_location(socket)?;
        let mut name = String::from("agent-sessions-");
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in socket.as_os_str().as_bytes() {
            name.push(HEX[(byte >> 4) as usize] as char);
            name.push(HEX[(byte & 15) as usize] as char);
        }
        name.push_str(".json");
        ensure!(
            name.len() + ".lock".len() <= 255,
            "recovery-invalid: canonical socket path is too long for a scoped journal filename"
        );
        let directory_path = CString::new(parent.as_os_str().as_bytes())?;
        let fd = unsafe {
            libc::open(
                directory_path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error())
                .context("recovery-file: opening configuration directory");
        }
        let directory = unsafe { File::from_raw_fd(fd) };
        let metadata = directory.metadata()?;
        ensure!(
            metadata.is_dir()
                && metadata.uid() == unsafe { libc::geteuid() }
                && metadata.mode() & 0o022 == 0,
            "recovery-invalid: configuration parent must be owned by this user and not writable by others"
        );
        let lock_name = CString::new(format!("{name}.lock"))?;
        let lock = match open_at(
            &directory,
            &lock_name,
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
        ) {
            Ok(file) => {
                file.set_permissions(fs::Permissions::from_mode(0o600))?;
                file
            }
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::AlreadyExists) =>
            {
                open_at(&directory, &lock_name, libc::O_RDWR)?
            }
            Err(error) => return Err(error),
        };
        private_file(&lock.metadata()?)?;
        loop {
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::WouldBlock {
                bail!("recovery-busy: another daemon holds this socket's conversation journal");
            }
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error).context("recovery-lock: acquiring exclusive lifetime lock");
            }
        }
        let mut journal = Self {
            directory,
            lock: WriterLock(lock),
            path: parent.join(&name),
            name: CString::new(name)?,
            lock_name,
            expected: None,
            records: Vec::new(),
            uncertain: false,
        };
        journal.check_names()?;
        journal.expected = journal.read_bytes()?;
        if let Some(bytes) = &journal.expected {
            let disk: Disk = serde_json::from_slice(bytes).context(
                "recovery-invalid: corrupt conversation journal; preserve it for repair",
            )?;
            ensure!(
                disk.version == 1,
                "recovery-invalid: unsupported conversation journal version"
            );
            journal.records = normalize(disk.records)?;
        }
        Ok(journal)
    }

    pub(crate) fn records(&self) -> Vec<Record> {
        self.records.clone()
    }

    pub(crate) fn has_project(&self, id: &str) -> bool {
        self.records.iter().any(|record| record.project.id == id)
    }

    /// Agent-native stores are separate from the metadata schema and created
    /// lazily by their owner, not by opening an empty journal.
    pub(crate) fn agent_store(&self) -> PathBuf {
        self.path.with_extension("agents")
    }

    fn check_names(&self) -> Result<()> {
        let parent = self
            .path
            .parent()
            .context("recovery-invalid: missing journal parent")?;
        let named = fs::symlink_metadata(parent)?;
        let held = self.directory.metadata()?;
        ensure!(
            named.is_dir()
                && named.dev() == held.dev()
                && named.ino() == held.ino()
                && named.uid() == unsafe { libc::geteuid() }
                && named.mode() & 0o022 == 0,
            "recovery-conflict: configuration directory changed"
        );
        private_file(&self.lock.0.metadata()?)?;
        named_file(&self.directory, &self.lock_name, &self.lock.0.metadata()?)
    }

    fn read_bytes(&self) -> Result<Option<Vec<u8>>> {
        let file = match open_at(&self.directory, &self.name, libc::O_RDONLY) {
            Ok(file) => file,
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        private_file(&metadata)?;
        ensure!(
            metadata.len() <= MAX_BYTES as u64,
            "recovery-invalid: conversation journal exceeds size budget"
        );
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        (&file).take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= MAX_BYTES,
            "recovery-invalid: conversation journal exceeds size budget"
        );
        private_file(&file.metadata()?)?;
        named_file(&self.directory, &self.name, &metadata)?;
        Ok(Some(bytes))
    }

    pub(crate) fn replace(&mut self, records: Vec<Record>) -> Result<bool> {
        ensure!(
            !self.uncertain,
            "recovery-save: previous directory sync failed; reopen before retrying"
        );
        let records = normalize(records)?;
        self.check_names()?;
        ensure!(
            self.read_bytes()? == self.expected,
            "recovery-conflict: conversation journal changed externally; preserve it and reopen"
        );
        if records == self.records {
            return Ok(false);
        }
        struct BoundedBytes(Vec<u8>);
        impl Write for BoundedBytes {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if bytes.len() > MAX_BYTES.saturating_sub(self.0.len()) {
                    return Err(std::io::Error::other(
                        "conversation journal exceeds size budget",
                    ));
                }
                self.0.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        #[derive(Serialize)]
        struct Checkpoint<'a> {
            version: u32,
            records: &'a [Record],
        }
        let mut bytes = BoundedBytes(Vec::new());
        serde_json::to_writer(
            &mut bytes,
            &Checkpoint {
                version: 1,
                records: &records,
            },
        )
        .context("recovery-save: encoding bounded conversation journal")?;
        let temporary = CString::new(format!(".amux-recovery-{}.tmp", Uuid::new_v4()))?;
        let mut renamed = false;
        let mut created = false;
        let result = (|| -> Result<()> {
            let mut file = open_at(
                &self.directory,
                &temporary,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            )?;
            created = true;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
            private_file(&file.metadata()?)?;
            file.write_all(&bytes.0)?;
            file.sync_all()?;
            self.check_names()?;
            ensure!(
                self.read_bytes()? == self.expected,
                "recovery-conflict: journal changed during checkpoint"
            );
            private_file(&file.metadata()?)?;
            named_file(&self.directory, &temporary, &file.metadata()?)?;
            if unsafe {
                libc::renameat(
                    self.directory.as_raw_fd(),
                    temporary.as_ptr(),
                    self.directory.as_raw_fd(),
                    self.name.as_ptr(),
                )
            } != 0
            {
                return Err(std::io::Error::last_os_error())
                    .context("renaming conversation journal");
            }
            renamed = true;
            self.directory
                .sync_all()
                .context("journal replaced but directory sync failed; reopen before retrying")?;
            Ok(())
        })();
        if result.is_err() {
            if renamed {
                self.uncertain = true;
            } else if created {
                let _ =
                    unsafe { libc::unlinkat(self.directory.as_raw_fd(), temporary.as_ptr(), 0) };
            }
        }
        result.context("recovery-save: conversation metadata was not confirmed durable")?;
        self.expected = Some(bytes.0);
        self.records = records;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs::OpenOptions;
    use std::os::unix::fs::{OpenOptionsExt, symlink};
    use std::process::{Command, Stdio};

    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("ar-{}", Uuid::new_v4().simple()));
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            Self(fs::canonicalize(path).unwrap())
        }
        fn open(&self) -> Journal {
            Journal::open(&self.0.join("config.json"), &self.0.join("daemon.sock")).unwrap()
        }
        fn record(&self, id: &str) -> Record {
            Record {
                id: id.into(),
                project: Project {
                    id: "project".into(),
                    name: "Project".into(),
                    context: self.0.clone(),
                },
                name: format!("Agent {id}"),
                cwd: self.0.clone(),
                agent: Agent::Acp {
                    profile: "fixture".into(),
                },
                session_id: format!("session-{id}"),
                session_file: None,
                launch_hash: Some("0".repeat(64)),
                settings: json!({}),
                peer: None,
                launch_pending: false,
                blocked: None,
            }
        }
        fn write(&self, path: &Path, bytes: &[u8]) {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .unwrap();
            file.write_all(bytes).unwrap();
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .unwrap();
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn absent_journal_stays_absent_until_a_durable_change() {
        let root = Root::new();
        let mut journal = root.open();
        assert!(journal.records().is_empty());
        assert!(!journal.path.exists());
        assert!(!journal.agent_store().exists());
        assert!(!journal.replace(vec![]).unwrap());
        assert!(!journal.path.exists());
        let record = root.record("one");
        assert!(journal.replace(vec![record.clone()]).unwrap());
        let path = journal.path.clone();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o7777, 0o600);
        assert_eq!(fs::metadata(&path).unwrap().nlink(), 1);
        drop(journal);
        let mut reopened = root.open();
        assert_eq!(reopened.records(), vec![record]);
        assert!(reopened.replace(vec![]).unwrap());
        drop(reopened);
        assert!(root.open().records().is_empty());
    }

    #[test]
    fn unchanged_and_reordered_records_do_not_replace_the_inode() {
        let root = Root::new();
        let mut journal = root.open();
        let a = root.record("a");
        let b = root.record("b");
        assert!(journal.replace(vec![b.clone(), a.clone()]).unwrap());
        let old = fs::metadata(&journal.path).unwrap();
        let bytes = fs::read(&journal.path).unwrap();
        assert!(
            !journal
                .replace(vec![a.clone(), b.clone(), a.clone()])
                .unwrap()
        );
        let unchanged = fs::metadata(&journal.path).unwrap();
        assert_eq!(
            (old.ino(), old.mtime(), old.mtime_nsec()),
            (unchanged.ino(), unchanged.mtime(), unchanged.mtime_nsec())
        );
        assert_eq!(fs::read(&journal.path).unwrap(), bytes);
        let mut old_reader = File::open(&journal.path).unwrap();
        let mut changed = b;
        changed.name = "Renamed".into();
        assert!(journal.replace(vec![a, changed.clone()]).unwrap());
        assert_ne!(fs::metadata(&journal.path).unwrap().ino(), old.ino());
        let mut old_bytes = Vec::new();
        old_reader.read_to_end(&mut old_bytes).unwrap();
        assert_eq!(old_bytes, bytes);
        drop(journal);
        assert!(root.open().records().contains(&changed));
    }

    #[test]
    fn lifetime_lock_is_exclusive_and_survives_journal_renames() {
        let root = Root::new();
        let mut journal = root.open();
        let config = root.0.join("config.json");
        let socket = root.0.join("daemon.sock");
        assert!(Journal::open(&config, &socket).is_err());
        assert!(journal.replace(vec![root.record("a")]).unwrap());
        assert!(Journal::open(&config, &socket).is_err());
        drop(journal);
        assert!(Journal::open(&config, &socket).is_ok());
    }

    #[test]
    fn canonical_config_and_socket_aliases_share_the_same_lock() {
        let root = Root::new();
        let target = root.0.join("config.json");
        root.write(&target, b"{}");
        let alias = root.0.join("aliases");
        fs::create_dir(&alias).unwrap();
        symlink(&target, alias.join("config-link.json")).unwrap();
        symlink(&root.0, alias.join("socket-parent")).unwrap();
        let journal = root.open();
        let error = Journal::open(
            &alias.join("config-link.json"),
            &alias.join("socket-parent/daemon.sock"),
        );
        assert!(error.is_err());
        drop(journal);
        let alias_journal = Journal::open(
            &alias.join("config-link.json"),
            &alias.join("socket-parent/daemon.sock"),
        )
        .unwrap();
        assert_eq!(alias_journal.path.parent(), Some(root.0.as_path()));
    }

    #[test]
    fn racing_openers_cannot_both_hold_the_lifetime_lock() {
        use std::sync::{Arc, Barrier, mpsc};
        let root = Root::new();
        let start = Arc::new(Barrier::new(3));
        let finish = Arc::new(Barrier::new(3));
        let (sender, receiver) = mpsc::channel();
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let config = root.0.join("config.json");
                let socket = root.0.join("daemon.sock");
                let start = start.clone();
                let finish = finish.clone();
                let sender = sender.clone();
                std::thread::spawn(move || {
                    start.wait();
                    let journal = Journal::open(&config, &socket);
                    sender.send(journal.is_ok()).unwrap();
                    finish.wait();
                    drop(journal);
                })
            })
            .collect();
        start.wait();
        let successes = [receiver.recv().unwrap(), receiver.recv().unwrap()]
            .into_iter()
            .filter(|opened| *opened)
            .count();
        finish.wait();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(successes, 1);
        assert!(Journal::open(&root.0.join("config.json"), &root.0.join("daemon.sock")).is_ok());
    }

    #[test]
    fn unsafe_parent_and_dangling_configuration_alias_are_explicit_errors() {
        let root = Root::new();
        symlink(root.0.join("missing.json"), root.0.join("config-link.json")).unwrap();
        assert!(
            Journal::open(
                &root.0.join("config-link.json"),
                &root.0.join("daemon.sock")
            )
            .is_err()
        );
        fs::set_permissions(&root.0, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(Journal::open(&root.0.join("config.json"), &root.0.join("daemon.sock")).is_err());
    }

    #[test]
    fn different_sockets_are_independent_and_stable() {
        let root = Root::new();
        let mut first = root.open();
        first.replace(vec![root.record("first")]).unwrap();
        let mut second =
            Journal::open(&root.0.join("config.json"), &root.0.join("other.sock")).unwrap();
        assert_ne!(first.path, second.path);
        assert_ne!(first.agent_store(), second.agent_store());
        assert!(second.records().is_empty());
        second.replace(vec![root.record("second")]).unwrap();
        let first_path = first.path.clone();
        drop(first);
        assert_eq!(root.open().path, first_path);
        assert_eq!(root.open().records()[0].id, "first");
    }

    #[test]
    fn no_follow_rejects_journal_and_lock_symlinks_without_modifying_targets() {
        for lock in [false, true] {
            let root = Root::new();
            let journal = root.open();
            let path = if lock {
                journal
                    .path
                    .with_file_name(journal.lock_name.to_str().unwrap())
            } else {
                journal.path.clone()
            };
            drop(journal);
            if lock {
                fs::remove_file(&path).unwrap();
            }
            let target = root.0.join("target");
            root.write(&target, b"do not touch");
            symlink(&target, &path).unwrap();
            assert!(
                Journal::open(&root.0.join("config.json"), &root.0.join("daemon.sock")).is_err()
            );
            assert_eq!(fs::read(&target).unwrap(), b"do not touch");
        }
    }

    #[test]
    fn hardlink_aliases_and_permissive_files_are_rejected_not_repaired() {
        for lock in [false, true] {
            for hardlink in [false, true] {
                let root = Root::new();
                let mut journal = root.open();
                journal.replace(vec![root.record("one")]).unwrap();
                let path = if lock {
                    journal
                        .path
                        .with_file_name(journal.lock_name.to_str().unwrap())
                } else {
                    journal.path.clone()
                };
                drop(journal);
                if hardlink {
                    fs::hard_link(&path, root.0.join("alias")).unwrap();
                } else {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
                }
                let before = fs::read(&path).unwrap();
                assert!(
                    Journal::open(&root.0.join("config.json"), &root.0.join("daemon.sock"))
                        .is_err()
                );
                assert_eq!(fs::read(&path).unwrap(), before);
                if !hardlink {
                    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o644);
                }
            }
        }
    }

    #[test]
    fn fifo_and_directory_are_rejected_without_blocking() {
        for fifo in [false, true] {
            let root = Root::new();
            let journal = root.open();
            let path = journal.path.clone();
            drop(journal);
            if fifo {
                let path = CString::new(path.as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
            } else {
                fs::create_dir(&path).unwrap();
            }
            assert!(
                Journal::open(&root.0.join("config.json"), &root.0.join("daemon.sock")).is_err()
            );
        }
    }

    #[test]
    fn corrupt_and_unknown_schema_records_are_preserved() {
        let cases = [
            b"".as_slice(),
            b"{",
            b"{\"version\":1,\"records\":[],\"prompt\":\"secret\"}",
            b"{\"version\":2,\"records\":[]}",
            b"{\"version\":1,\"version\":1,\"records\":[]}",
            b"{\"version\":1,\"records\":[]} trailing",
        ];
        for bytes in cases {
            let root = Root::new();
            let journal = root.open();
            let path = journal.path.clone();
            drop(journal);
            root.write(&path, bytes);
            assert!(
                Journal::open(&root.0.join("config.json"), &root.0.join("daemon.sock")).is_err()
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn external_corruption_is_not_overwritten_even_by_an_identical_update() {
        let root = Root::new();
        let mut journal = root.open();
        let records = vec![root.record("one")];
        journal.replace(records.clone()).unwrap();
        fs::write(&journal.path, b"broken").unwrap();
        assert!(journal.replace(records).is_err());
        assert_eq!(fs::read(&journal.path).unwrap(), b"broken");
        assert_eq!(journal.records()[0].id, "one");
    }

    #[test]
    fn replacing_the_lock_inode_prevents_further_writes() {
        let root = Root::new();
        let mut journal = root.open();
        let path = journal
            .path
            .with_file_name(journal.lock_name.to_str().unwrap());
        fs::remove_file(&path).unwrap();
        root.write(&path, b"");
        assert!(journal.replace(vec![root.record("one")]).is_err());
        assert!(!journal.path.exists());
    }

    #[test]
    fn exact_duplicates_collapse_but_conflicting_identities_never_choose_a_winner() {
        let root = Root::new();
        let mut journal = root.open();
        let record = root.record("one");
        assert!(
            journal
                .replace(vec![record.clone(), record.clone()])
                .unwrap()
        );
        assert_eq!(journal.records(), vec![record.clone()]);
        let before = fs::read(&journal.path).unwrap();
        let mut same_id = record.clone();
        same_id.session_id = "different".into();
        assert!(journal.replace(vec![same_id, record.clone()]).is_err());
        let mut same_conversation = record.clone();
        same_conversation.id = "two".into();
        assert!(
            journal
                .replace(vec![same_conversation.clone(), record.clone()])
                .is_err()
        );
        assert!(journal.replace(vec![record, same_conversation]).is_err());
        assert_eq!(fs::read(&journal.path).unwrap(), before);
    }

    #[test]
    fn profile_aliases_cannot_duplicate_a_conversation_but_changed_launch_namespaces_can() {
        let root = Root::new();
        let mut journal = root.open();
        let a = root.record("a");
        let mut b = a.clone();
        b.id = "b".into();
        b.agent = Agent::Acp {
            profile: "alias".into(),
        };
        assert!(journal.replace(vec![a.clone(), b.clone()]).is_err());
        b.launch_hash = Some("1".repeat(64));
        journal.replace(vec![a, b]).unwrap();
        assert_eq!(journal.records().len(), 2);
    }

    #[test]
    fn codex_model_is_not_a_new_conversation_identity() {
        let root = Root::new();
        let mut journal = root.open();
        let mut a = root.record("a");
        a.agent = Agent::Codex {
            model: "model-a".into(),
        };
        a.launch_hash = None;
        let mut b = a.clone();
        b.id = "b".into();
        b.agent = Agent::Codex {
            model: "model-b".into(),
        };
        assert!(journal.replace(vec![a, b]).is_err());
    }

    #[test]
    fn record_validation_rejects_content_authority_and_invalid_metadata() {
        let root = Root::new();
        let mut journal = root.open();
        let good = root.record("good");
        let mut invalid = Vec::new();
        for settings in [
            json!({"prompt":"secret"}),
            json!({"env":{"TOKEN":"secret"}}),
            json!({"options":{"x":[]}}),
            json!({"model":null}),
        ] {
            let mut record = good.clone();
            record.settings = settings;
            invalid.push(record);
        }
        let mut record = good.clone();
        record.cwd = PathBuf::from("relative");
        invalid.push(record);
        let mut record = good.clone();
        record.name = "bad\nname".into();
        invalid.push(record);
        let mut record = good.clone();
        record.session_id = "x".repeat(1025);
        invalid.push(record);
        let mut record = good.clone();
        record.agent = Agent::Acp {
            profile: "../command".into(),
        };
        invalid.push(record);
        let mut record = good.clone();
        record.peer = Some(Peer {
            pid: -1,
            start: [1, 0],
            boot: "bad".into(),
        });
        invalid.push(record);
        let mut record = good.clone();
        record.agent = Agent::Omp;
        record.launch_hash = None;
        record.settings = json!({"model":"x"});
        invalid.push(record);
        for record in invalid {
            assert!(journal.replace(vec![record]).is_err());
        }
        assert!(!journal.path.exists());
        let mut encoded = serde_json::to_value(good).unwrap();
        encoded["authority"] = json!("old-owner");
        let bytes = serde_json::to_vec(&json!({"version":1,"records":[encoded]})).unwrap();
        let path = journal.path.clone();
        drop(journal);
        root.write(&path, &bytes);
        assert!(Journal::open(&root.0.join("config.json"), &root.0.join("daemon.sock")).is_err());
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn duplicate_settings_and_option_keys_are_corruption() {
        let root = Root::new();
        for settings in [
            r#"{"model":"one","model":"two"}"#,
            r#"{"options":{"key":"one","key":"two"}}"#,
        ] {
            let encoded = serde_json::to_string(&root.record("one")).unwrap();
            let encoded = encoded.replace("\"settings\":{}", &format!("\"settings\":{settings}"));
            assert!(serde_json::from_str::<Record>(&encoded).is_err());
        }
    }

    #[test]
    fn every_agent_shape_rejects_unknown_authority_and_launch_fields() {
        for encoded in [
            r#"{"kind":"omp","argv":["secret"]}"#,
            r#"{"kind":"omp","profile":null}"#,
            r#"{"kind":"acp","profile":"fixture","prompt":"secret"}"#,
            r#"{"kind":"codex","model":"test","owner":"old"}"#,
        ] {
            assert!(serde_json::from_str::<Agent>(encoded).is_err());
        }
        assert_eq!(
            serde_json::from_str::<Agent>(r#"{"kind":"omp"}"#).unwrap(),
            Agent::Omp
        );
    }

    #[test]
    fn record_count_and_file_size_have_hard_budgets() {
        let root = Root::new();
        let mut journal = root.open();
        let too_many: Vec<_> = (0..=MAX_RECORDS)
            .map(|id| root.record(&id.to_string()))
            .collect();
        assert!(journal.replace(too_many.clone()).is_err());
        let encoded = serde_json::to_vec(&Disk {
            version: 1,
            records: too_many,
        })
        .unwrap();
        let path = journal.path.clone();
        drop(journal);
        root.write(&path, &encoded);
        assert!(Journal::open(&root.0.join("config.json"), &root.0.join("daemon.sock")).is_err());
        fs::remove_file(&path).unwrap();
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        file.set_len(MAX_BYTES as u64 + 1).unwrap();
        assert!(Journal::open(&root.0.join("config.json"), &root.0.join("daemon.sock")).is_err());
        assert_eq!(fs::metadata(path).unwrap().len(), MAX_BYTES as u64 + 1);
    }

    #[test]
    fn encoding_budget_failure_preserves_the_last_durable_checkpoint() {
        let root = Root::new();
        let mut journal = root.open();
        let original = root.record("original");
        journal.replace(vec![original.clone()]).unwrap();
        let before = fs::read(&journal.path).unwrap();
        let options: serde_json::Map<String, Value> = (0..64)
            .map(|id| {
                (
                    format!("{id:03}{}", "k".repeat(253)),
                    Value::String("v".repeat(256)),
                )
            })
            .collect();
        let records = (0..128)
            .map(|id| {
                let mut record = root.record(&id.to_string());
                record.settings = json!({"options":options});
                record
            })
            .collect();
        assert!(journal.replace(records).is_err());
        assert_eq!(journal.records(), vec![original]);
        assert_eq!(fs::read(&journal.path).unwrap(), before);
        assert!(fs::read_dir(&root.0).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .as_bytes()
                .starts_with(b".amux-recovery-")
        }));
    }

    #[test]
    fn journal_accepts_native_terminal_limit_plus_acp_capacity() {
        let root = Root::new();
        let mut journal = root.open();
        let records: Vec<_> = (0..264).map(|id| root.record(&id.to_string())).collect();
        assert!(journal.replace(records).unwrap());
        drop(journal);
        assert_eq!(root.open().records().len(), 264);
    }

    #[test]
    fn peer_pin_requires_exact_start_and_boot_and_rejects_invalid_pids() {
        let peer = Peer::capture(std::process::id() as i32).expect("own native process identity");
        assert!(peer.status().unwrap());
        let mut reused = peer.clone();
        reused.start[0] += 1;
        assert!(!reused.status().unwrap());
        let mut rebooted = peer.clone();
        let prefix = if peer.boot.starts_with("darwin:") {
            "darwin:"
        } else {
            ""
        };
        let different = format!("{prefix}00000000-0000-0000-0000-000000000000");
        rebooted.boot = if peer.boot != different {
            different
        } else {
            format!("{prefix}11111111-1111-1111-1111-111111111111")
        };
        assert!(!rebooted.status().unwrap());
        assert!(Peer::capture(0).is_none());
        assert!(Peer::capture(-1).is_none());
        let mut invalid = peer;
        invalid.pid = -1;
        assert!(invalid.status().is_err());
    }

    #[test]
    fn unknown_native_probe_never_becomes_a_gone_peer() {
        assert!(process_status([1, 0], None, || false).is_err());
        assert!(!process_status([1, 0], None, || true).unwrap());
        assert!(
            process_status([1, 0], Some([1, 0]), || {
                panic!("an exact native stamp needs no existence probe")
            })
            .unwrap()
        );
        assert!(
            !process_status([1, 0], Some([2, 0]), || {
                panic!("a reused native stamp needs no existence probe")
            })
            .unwrap()
        );
    }

    #[test]
    fn reaped_peer_is_definitely_gone_without_signaling_it() {
        let mut child = Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pin = Peer::capture(child.id() as i32);
        drop(child.stdin.take());
        let status = child.wait().unwrap();
        assert!(status.success());
        let pin = pin.expect("owned waiting child identity");
        assert!(!pin.status().unwrap());
    }
}
