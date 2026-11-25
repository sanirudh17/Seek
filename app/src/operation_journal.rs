//! Append-only, checksummed operation evidence. Never replays filesystem mutations on startup.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::OpenOptionsExt,
    },
    path::{Path, PathBuf},
};
const LIMIT: u64 = 32 * 1024 * 1024;
const LINE_LIMIT: usize = 1024 * 1024;
const MARKER_ALLOWANCE: u64 = 4096;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct WirePath(pub Vec<u16>);
impl WirePath {
    pub fn new(path: &Path) -> Self {
        Self(path.as_os_str().encode_wide().collect())
    }
    pub fn path(&self) -> PathBuf {
        std::ffi::OsString::from_wide(&self.0).into()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Fingerprint {
    pub volume: u64,
    pub id: [u8; 16],
    pub bytes: u64,
    pub hash: [u8; 32],
    #[serde(default)]
    pub content_hash: Option<[u8; 32]>,
    #[serde(default)]
    pub directory: bool,
}
impl Fingerprint {
    pub fn at(path: &Path) -> Option<Self> {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(1 | 4)
            .custom_flags(0x02000000 | 0x00200000 | 0x00100000)
            .open(path)
            .ok()?;
        Self::read(&file).ok()
    }
    pub fn key(file: &File) -> Result<(u64, [u8; 16]), String> {
        use std::os::windows::io::AsRawHandle;
        let mut info = windows_sys::Win32::Storage::FileSystem::FILE_ID_INFO::default();
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx(
                file.as_raw_handle(),
                windows_sys::Win32::Storage::FileSystem::FileIdInfo,
                (&mut info as *mut windows_sys::Win32::Storage::FileSystem::FILE_ID_INFO).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok((info.VolumeSerialNumber, info.FileId.Identifier))
    }
    pub fn read(file: &File) -> Result<Self, String> {
        crate::operation_snapshot::read(file)
    }
    pub fn compatible(&self, expected: &Self) -> bool {
        if expected.content_hash.is_some() {
            self == expected
        } else {
            !self.directory
                && !expected.directory
                && self.volume == expected.volume
                && self.id == expected.id
                && self.bytes == expected.bytes
                && self.hash == expected.hash
        }
    }
    pub(crate) fn read_legacy(file: &File) -> Result<Self, String> {
        use std::os::windows::fs::MetadataExt;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.file_attributes() & (0x400 | 0x1000 | 0x40000 | 0x400000) != 0 {
            return Err("Undo requires readable local files without links or offline data".into());
        }
        let (volume, id) = Self::key(file)?;
        let mut input = file.try_clone().map_err(|e| e.to_string())?;
        input.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut bytes = 0u64;
        loop {
            crate::operation_control::check()?;
            let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            bytes += n as u64;
            digest.update(&buffer[..n]);
        }
        if bytes != meta.len() {
            return Err("File changed while reading its undo snapshot".into());
        }
        Ok(Self {
            volume,
            id,
            bytes,
            hash: digest.finalize().into(),
            content_hash: None,
            directory: false,
        })
    }
}
#[derive(Clone)]
pub struct RenamePlan {
    pub path: PathBuf,
    pub name: String,
    pub expected: Fingerprint,
}
#[derive(Clone, Debug)]
pub struct MovePlan {
    pub path: PathBuf,
    pub target: PathBuf,
    pub expected: Fingerprint,
}
#[derive(Clone)]
pub struct GuardedItem {
    pub path: PathBuf,
    pub expected: Fingerprint,
}
#[derive(Clone)]
pub struct InverseSet {
    pub items: Vec<GuardedItem>,
}
#[derive(Clone)]
pub struct RestoreItem {
    pub original: PathBuf,
    pub recycled: PathBuf,
    pub expected: Fingerprint,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    SegmentedHistory,
    ArchiveBoundary {
        last_job: u64,
        previous: String,
    },
    Begin {
        job: u64,
        label: String,
    },
    Before {
        job: u64,
        action: String,
        source: Option<WirePath>,
        folder: Option<WirePath>,
        name: Option<Vec<u16>>,
        #[serde(default)]
        proof: bool,
    },
    Proof {
        job: u64,
        action: String,
        source: Option<WirePath>,
        target: Option<WirePath>,
        identity: Option<Fingerprint>,
    },
    After {
        job: u64,
        action: String,
        source: Option<WirePath>,
        target: Option<WirePath>,
        parsing_name: Option<Vec<u16>>,
        hresult: i32,
        #[serde(default)]
        identity: Option<Fingerprint>,
    },
    End {
        job: u64,
        error: Option<String>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T = Event> {
    version: u32,
    seq: u64,
    previous: String,
    event: T,
    checksum: String,
}
fn digest(seq: u64, previous: &str, event: &impl Serialize) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(1u32, seq, previous, event)).map_err(|e| e.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub id: u64,
    pub label: String,
    pub outcomes: Vec<Event>,
    pub finished: bool,
    pub error: Option<String>,
    pub review: Vec<String>,
}
impl Job {
    fn verified<'a>(
        &'a self,
        action: &str,
        source: &Option<WirePath>,
        target: &Option<WirePath>,
        direct: &'a Option<Fingerprint>,
    ) -> Option<&'a Fingerprint> {
        self.outcomes
            .iter()
            .rev()
            .find_map(|event| match event {
                Event::Proof {
                    action: kind,
                    source: from,
                    target: to,
                    identity,
                    ..
                } if kind == action && from == source && to == target => identity.as_ref(),
                _ => None,
            })
            .or(direct.as_ref())
    }
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub jobs: Vec<Job>,
    pub warning: Option<String>,
    pub archived_file: Option<PathBuf>,
    pub bytes_used: u64,
    base_job: u64,
    previous_archive: Option<String>,
    pending_items: u64,
    pending_proofs: u64,
    segmented: bool,
    restored: std::collections::HashSet<(WirePath, Fingerprint)>,
}
impl Snapshot {
    pub fn latest_action(&self, ignore: Option<u64>) -> Option<&Job> {
        self.jobs
            .iter()
            .rev()
            .filter(|job| Some(job.id) != ignore)
            .find(|job| {
                !job.finished || job.error.is_none() || job.outcomes.iter().any(|event|
                matches!(event, Event::After { hresult, .. } if item_completed(*hresult)))
            })
    }
    /// Folders the latest Organize job created; its undo removes them when empty.
    pub fn organize_created_folders(&self, ignore: Option<u64>) -> Vec<PathBuf> {
        let Some(job) = self
            .latest_action(ignore)
            .filter(|job| job.label == crate::file_ops::ORGANIZE_LABEL)
        else {
            return vec![];
        };
        job.outcomes
            .iter()
            .filter_map(|event| match event {
                Event::After {
                    action,
                    target: Some(target),
                    hresult,
                    ..
                } if action == "new_folder" && item_completed(*hresult) => Some(target.path()),
                _ => None,
            })
            .collect()
    }
    pub fn batch_rename_plan(&self, redo: bool, ignore: Option<u64>) -> Option<Vec<MovePlan>> {
        let job = self.latest_action(ignore)?;
        if !job.finished
            || job.error.is_some()
            || if redo {
                job.label != "Undo batch rename"
            } else {
                !matches!(job.label.as_str(), "Batch rename" | "Redo batch rename")
            }
        {
            return None;
        }
        let mut chains: Vec<MovePlan> = vec![];
        for event in &job.outcomes {
            if let Event::After {
                action,
                source,
                target,
                hresult,
                identity,
                ..
            } = event
            {
                if action != "move" || !item_completed(*hresult) {
                    return None;
                }
                let identity = identity.as_ref()?;
                let source = source.as_ref()?.path();
                let target = target.as_ref()?.path();
                if let Some(chain) = chains.iter_mut().find(|chain| {
                    chain.expected.volume == identity.volume && chain.expected.id == identity.id
                }) {
                    if chain.path != source || chain.expected != *identity {
                        return None;
                    }
                    chain.path = target;
                } else {
                    chains.push(MovePlan {
                        path: target,
                        target: source,
                        expected: identity.clone(),
                    });
                }
            }
        }
        chains.retain(|chain| chain.path != chain.target);
        (!chains.is_empty()).then_some(chains)
    }
    pub fn copy_undo_plan(&self, ignore: Option<u64>) -> Option<InverseSet> {
        let job = self.latest_action(ignore)?;
        let action = match job.label.as_str() {
            "Copy files" | "Paste files" => "copy",
            "Redo copy" => "restore",
            _ => return None,
        };
        if !job.finished || job.error.is_some() {
            return None;
        }
        let mut items = job
            .outcomes
            .iter()
            .filter_map(|event| match event {
                Event::After {
                    action: kind,
                    source,
                    target,
                    hresult,
                    identity,
                    ..
                } if kind == action && item_completed(*hresult) => Some(GuardedItem {
                    path: target.as_ref()?.path(),
                    expected: job.verified(kind, source, target, identity)?.clone(),
                }),
                _ => None,
            })
            .collect::<Vec<_>>();
        let paths = items
            .iter()
            .map(|item| item.path.clone())
            .collect::<Vec<_>>();
        items.retain(|item| {
            !paths
                .iter()
                .any(|parent| item.path != *parent && item.path.starts_with(parent))
        });
        items.sort_by(|a, b| a.path.cmp(&b.path));
        items.dedup_by(|a, b| a.path == b.path);
        if items.is_empty() {
            return None;
        }
        // Descendant callbacks are covered by a verified whole-directory result;
        // an unknown/overwritten root is never silently treated as a fresh copy.
        for event in &job.outcomes {
            if let Event::After {
                target,
                hresult,
                action: kind,
                ..
            } = event
                && item_completed(*hresult)
            {
                let target = target.as_ref()?.path();
                if kind != action
                    || !items
                        .iter()
                        .any(|item| target == item.path || target.starts_with(&item.path))
                {
                    return None;
                }
            }
        }
        Some(InverseSet { items })
    }
    pub fn copy_redo_job(&self, ignore: Option<u64>) -> Option<u64> {
        let job = self.latest_action(ignore)?;
        (job.label == "Undo copy" && self.put_back_plan(job.id).is_some()).then_some(job.id)
    }
    pub fn recycle_redo_plan(&self, ignore: Option<u64>) -> Option<InverseSet> {
        let job = self.latest_action(ignore)?;
        if job.label != "Put Back" || !job.finished || job.error.is_some() {
            return None;
        }
        let mut items = vec![];
        for event in &job.outcomes {
            if let Event::After {
                action,
                target,
                hresult,
                identity,
                ..
            } = event
                && item_completed(*hresult)
            {
                if action != "restore" {
                    return None;
                }
                items.push(GuardedItem {
                    path: target.as_ref()?.path(),
                    expected: identity.as_ref()?.clone(),
                });
            }
        }
        (!items.is_empty()).then_some(InverseSet { items })
    }
    pub fn put_back_plan(&self, job: u64) -> Option<Vec<RestoreItem>> {
        let job = &self.jobs[self.jobs.binary_search_by_key(&job, |row| row.id).ok()?];
        if !job.finished
            || !matches!(
                job.label.as_str(),
                "Move to Recycle Bin" | "Redo recycle" | "Undo copy"
            )
        {
            return None;
        }
        let mut items = vec![];
        for event in &job.outcomes {
            let Event::After {
                action,
                source,
                target,
                parsing_name,
                hresult,
                identity,
                ..
            } = event
            else {
                continue;
            };
            if action != "delete" || !item_completed(*hresult) {
                continue;
            }
            let original = source.as_ref()?.path();
            let recycled = target.as_ref()?.path();
            let expected = identity.as_ref()?;
            // Index successful restores while reading/appending the journal,
            // keeping UI eligibility checks free of repeated history scans.
            if self
                .restored
                .contains(&(target.as_ref()?.clone(), expected.clone()))
            {
                continue;
            }
            if !original.is_absolute()
                || !recycled.is_absolute()
                || !crate::file_ops::valid_name(original.file_name()?.to_str()?)
            {
                return None;
            }
            let parsing_name = parsing_name.as_ref()?;
            if parsing_name.is_empty() || parsing_name.contains(&0) {
                return None;
            }
            items.push(RestoreItem {
                original,
                recycled,
                expected: expected.clone(),
            });
        }
        (!items.is_empty()).then_some(items)
    }
    pub fn latest_put_back(&self) -> Option<u64> {
        self.jobs
            .iter()
            .rev()
            .find(|job| self.put_back_plan(job.id).is_some())
            .map(|job| job.id)
    }
    pub fn recycle_undo_job(&self) -> Option<u64> {
        let job = self.jobs.iter().rev().find(|job| {
            !job.finished || job.error.is_none() || job.outcomes.iter().any(|event|
                matches!(event, Event::After { hresult, .. } if item_completed(*hresult)))
        })?;
        matches!(job.label.as_str(), "Move to Recycle Bin" | "Redo recycle")
            .then(|| self.put_back_plan(job.id).map(|_| job.id))
            .flatten()
    }
    fn validate(&self, event: &Event) -> Result<(), String> {
        match event {
            Event::SegmentedHistory => {
                if self.segmented {
                    Err("Duplicate segmented-history guard".into())
                } else {
                    Ok(())
                }
            }
            Event::ArchiveBoundary { previous, .. } => {
                if !self.jobs.is_empty()
                    || self.previous_archive.is_some()
                    || !crate::file_ops::valid_name(previous)
                {
                    Err("Invalid history archive boundary".into())
                } else {
                    Ok(())
                }
            }
            Event::Begin { job, .. }
                if self.jobs.last().map_or(self.base_job, |last| last.id) >= *job =>
            {
                Err("Operation identifiers must increase".into())
            }
            Event::Begin { .. } => Ok(()),
            Event::Before { job, .. }
            | Event::After { job, .. }
            | Event::Proof { job, .. }
            | Event::End { job, .. } => {
                let row = self
                    .jobs
                    .iter()
                    .rev()
                    .find(|row| row.id == *job)
                    .ok_or("Operation has no durable intent")?;
                if row.finished {
                    Err("Operation already finished".into())
                } else {
                    Ok(())
                }
            }
        }
    }
    fn apply(&mut self, event: &Event) -> Result<(), String> {
        self.validate(event)?;
        if let Event::After {
            action,
            source: Some(source),
            hresult,
            identity: Some(identity),
            ..
        } = event
            && action == "restore"
            && item_completed(*hresult)
        {
            self.restored.insert((source.clone(), identity.clone()));
        }
        match event {
            Event::SegmentedHistory => {
                self.segmented = true;
            }
            Event::ArchiveBoundary { last_job, previous } => {
                self.base_job = *last_job;
                self.previous_archive = Some(previous.clone());
            }
            Event::Begin { job, label } => {
                self.pending_items = 0;
                self.pending_proofs = 0;
                self.jobs.push(Job {
                    id: *job,
                    label: label.clone(),
                    outcomes: vec![],
                    review: vec![],
                    finished: false,
                    error: None,
                });
            }
            Event::Before { job, .. }
            | Event::After { job, .. }
            | Event::Proof { job, .. }
            | Event::End { job, .. } => {
                let row = self
                    .jobs
                    .iter_mut()
                    .rev()
                    .find(|j| j.id == *job)
                    .ok_or("Operation has no durable intent")?;
                if row.finished {
                    return Err("Operation already finished".into());
                }
                match event {
                    Event::End { error, .. } => {
                        self.pending_items = 0;
                        self.pending_proofs = 0;
                        row.finished = true;
                        row.error = error.clone();
                    }
                    Event::Before { proof, .. } => {
                        self.pending_items = self.pending_items.saturating_add(1);
                        if *proof {
                            self.pending_proofs = self.pending_proofs.saturating_add(1);
                        }
                        row.outcomes.push(event.clone());
                    }
                    Event::After { .. } => {
                        self.pending_items = self.pending_items.saturating_sub(1);
                        row.outcomes.push(event.clone());
                    }
                    Event::Proof { .. } => {
                        self.pending_proofs = self.pending_proofs.saturating_sub(1);
                        row.outcomes.push(event.clone());
                    }
                    _ => unreachable!(),
                }
            }
        }
        Ok(())
    }
    pub fn rename_plan(&self, redo: bool, ignore: Option<u64>) -> Option<RenamePlan> {
        let job=self.jobs.iter().rev().filter(|job|Some(job.id)!=ignore).find(|job|!job.finished||job.error.is_none()||job.outcomes.iter().any(|event|matches!(event,Event::After{hresult,..} if item_completed(*hresult))))?;
        if !job.finished || job.error.is_some() {
            return None;
        }
        let expected_label = if redo { "Undo rename" } else { "Rename" };
        if job.label != expected_label && (redo || job.label != "Redo rename") {
            return None;
        }
        let mut completed = job.outcomes.iter().filter_map(|event| match event {
            Event::After {
                action,
                source: Some(source),
                target: Some(target),
                hresult,
                identity: Some(identity),
                ..
            } if action == "rename" && item_completed(*hresult) => Some((source, target, identity)),
            _ => None,
        });
        let (source, target, identity) = completed.next()?;
        if completed.next().is_some() {
            return None;
        }
        let original = source.path();
        let path = target.path();
        if original.parent() != path.parent() {
            return None;
        }
        let name = original.file_name()?.to_str()?.to_owned();
        Some(RenamePlan {
            path,
            name,
            expected: identity.clone(),
        })
    }
    pub fn review_interrupted(&mut self) -> Result<(), String> {
        for job in self.jobs.iter_mut().filter(|job| !job.finished) {
            let mut review = vec![];
            for (index, event) in job.outcomes.iter().enumerate() {
                crate::operation_control::check()?;
                match event {
                    Event::Before { action, source, .. } => {
                        let reported = job.outcomes[index + 1..].iter().any(|event| matches!(event,
                            Event::After { action: kind, source: from, .. } if kind == action && from == source));
                        if !reported {
                            let name = source
                                .as_ref()
                                .map(|path| path.path().display().to_string())
                                .unwrap_or_else(|| {
                                    "an item whose path Windows did not report".into()
                                });
                            review.push(format!("{action}: no completion was recorded for {name}. Its outcome is unknown; inspect the source and destination before retrying."));
                        }
                    }
                    Event::After {
                        action,
                        source,
                        target,
                        hresult,
                        identity,
                        ..
                    } if item_completed(*hresult) => {
                        let expected = job.verified(action, source, target, identity);
                        let message = match (target, expected) {
                            (Some(path), Some(expected)) => {
                                let path = path.path();
                                let state = if Fingerprint::at(&path)
                                    .is_some_and(|current| current.compatible(expected))
                                {
                                    "still matches the recorded result"
                                } else {
                                    "is missing, changed or unavailable; no inverse was authorized"
                                };
                                format!("{action}: {} {state}.", path.display())
                            }
                            _ => format!(
                                "{action}: Windows recorded completion without a usable final snapshot. Inspect the actual files; no inverse was authorized."
                            ),
                        };
                        review.push(message);
                    }
                    _ => {}
                }
                if review.len() == 100 {
                    review.push("Review limited to 100 results. Archive the history file to inspect the complete evidence.".into());
                    break;
                }
            }
            if review.is_empty() {
                review.push("No item completion was recorded. No filesystem changes were inferred or replayed.".into());
            }
            job.review = review;
        }
        Ok(())
    }
    pub fn interrupted(&self) -> usize {
        self.jobs.iter().filter(|j| !j.finished).count()
    }
    #[cfg(test)]
    pub fn move_plan(&self, redo: bool, ignore: Option<u64>) -> Option<MovePlan> {
        let mut plans = self.move_plans(redo, ignore)?;
        (plans.len() == 1).then(|| plans.remove(0))
    }
    pub fn move_plans(&self, redo: bool, ignore: Option<u64>) -> Option<Vec<MovePlan>> {
        let job = self.latest_action(ignore)?;
        if !job.finished
            || job.error.is_some()
            || if redo {
                job.label != "Undo move"
            } else {
                !matches!(
                    job.label.as_str(),
                    "Move files"
                        | "Paste files"
                        | "Redo move"
                        | "New folder with selection"
                        | crate::file_ops::ORGANIZE_LABEL
                )
            }
        {
            return None;
        }
        let mut plans = vec![];
        for event in &job.outcomes {
            if let Event::After {
                action,
                source,
                target,
                hresult,
                identity,
                ..
            } = event
                && item_completed(*hresult)
            {
                if action == "new_folder"
                    && matches!(
                        job.label.as_str(),
                        "New folder with selection" | crate::file_ops::ORGANIZE_LABEL
                    )
                {
                    continue;
                }
                if action != "move" {
                    return None;
                }
                if let Some(expected) = job.verified(action, source, target, identity) {
                    let original = source.as_ref()?.path();
                    let path = target.as_ref()?.path();
                    if original == path
                        || !crate::file_ops::valid_name(original.file_name()?.to_str()?)
                    {
                        return None;
                    }
                    plans.push(MovePlan {
                        path,
                        target: original,
                        expected: expected.clone(),
                    });
                }
            }
        }
        let roots = plans
            .iter()
            .map(|plan| (plan.path.clone(), plan.target.clone()))
            .collect::<Vec<_>>();
        plans.retain(|plan| {
            !roots.iter().any(|(path, target)| {
                plan.path != *path && plan.path.starts_with(path) && plan.target.starts_with(target)
            })
        });
        plans.sort_by(|a, b| a.path.cmp(&b.path));
        plans.dedup_by(|a, b| a.path == b.path && a.target == b.target);
        if plans.is_empty() {
            return None;
        }
        for event in &job.outcomes {
            if let Event::After {
                action,
                source,
                target,
                hresult,
                ..
            } = event
                && item_completed(*hresult)
            {
                if action == "new_folder"
                    && matches!(
                        job.label.as_str(),
                        "New folder with selection" | crate::file_ops::ORGANIZE_LABEL
                    )
                {
                    continue;
                }
                let source = source.as_ref()?.path();
                let target = target.as_ref()?.path();
                if !plans.iter().any(|plan| {
                    target
                        .strip_prefix(&plan.path)
                        .ok()
                        .zip(source.strip_prefix(&plan.target).ok())
                        .is_some_and(|(a, b)| a == b)
                }) {
                    return None;
                }
            }
        }
        Some(plans)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    active: String,
}
pub struct Journal {
    file: File,
    root: PathBuf,
    active: PathBuf,
    anchor: Option<File>,
    _parents: Vec<File>,
    seq: u64,
    previous: String,
    bytes: u64,
    pub snapshot: Snapshot,
    failed: bool,
}
impl Journal {
    pub fn open(path: &Path) -> Result<Self, String> {
        let absolute = std::path::absolute(path).map_err(|e| e.to_string())?;
        let path = absolute.as_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let parents = crate::safe_rename::stable_parent(
            path.parent().ok_or("History needs a parent folder")?,
        )
        .map_err(|e| e.to_string())?;
        let anchor = Self::locked_file(path, true)?;
        let pointer = path.with_extension("current.json");
        let active = match Self::locked_file(&pointer, false) {
            Ok(mut file) => {
                if file.metadata().map_err(|e| e.to_string())?.len() > 4096 {
                    return Err("History pointer is too large".into());
                }
                let mut data = vec![];
                file.read_to_end(&mut data).map_err(|e| e.to_string())?;
                let manifest: Manifest = serde_json::from_slice(&data)
                    .map_err(|e| format!("History pointer is damaged: {e}"))?;
                if manifest.version != 1 || !Self::segment_name(path, &manifest.active) {
                    return Err("History pointer has an invalid segment name".into());
                }
                Some(path.with_file_name(manifest.active))
            }
            Err(error) => {
                if std::fs::symlink_metadata(&pointer).is_ok()
                    || !std::fs::symlink_metadata(&pointer)
                        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                {
                    return Err(error);
                }
                None
            }
        };
        let mut journal = if let Some(active) = active {
            if anchor.metadata().map_err(|e| e.to_string())?.len() == 0 {
                return Err("The original history anchor is missing or empty".into());
            }
            let file = Self::locked_file(&active, false)?;
            let mut journal = Self::read_segment(&active, file)?;
            if journal.snapshot.previous_archive.is_none() {
                return Err("Active history has no archive boundary".into());
            }
            journal.anchor = Some(anchor);
            journal
        } else {
            Self::read_segment(path, anchor)?
        };
        journal.root = path.into();
        journal._parents = parents;
        journal.resolve_archive()?;
        Ok(journal)
    }
    fn segment_name(root: &Path, name: &str) -> bool {
        root.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|base| {
                crate::file_ops::valid_name(name)
                    && name.starts_with(&format!("{base}.segment-"))
                    && name.ends_with(".jsonl")
            })
    }
    fn resolve_archive(&mut self) -> Result<(), String> {
        if let Some(previous) = &self.snapshot.previous_archive {
            if self.root.file_name().and_then(|n| n.to_str()) != Some(previous.as_str())
                && !Self::segment_name(&self.root, previous)
            {
                return Err("Invalid previous history segment".into());
            }
            self.snapshot.archived_file = Some(self.root.with_file_name(previous));
            if self.snapshot.warning.is_none() {
                self.snapshot.warning = Some("Previous history was preserved. Undo and Put Back now apply to the new history only.".into());
            }
        }
        self.snapshot.bytes_used = self.bytes;
        Ok(())
    }
    fn locked_file(path: &Path, create: bool) -> Result<File, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let file = OpenOptions::new()
            .create(create)
            .read(true)
            .write(true)
            .truncate(false)
            .share_mode(1)
            .custom_flags(0x00200000 | 0x00100000)
            .open(path)
            .map_err(|e| format!("Cannot open operation history: {e}"))?;
        use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file()
            || metadata.file_attributes() & (0x400 | 0x1000 | 0x40000 | 0x400000) != 0
        {
            return Err("Operation history must be a local regular file".into());
        }
        let mut standard = windows_sys::Win32::Storage::FileSystem::FILE_STANDARD_INFO::default();
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx(
                file.as_raw_handle(),
                windows_sys::Win32::Storage::FileSystem::FileStandardInfo,
                (&mut standard as *mut windows_sys::Win32::Storage::FileSystem::FILE_STANDARD_INFO)
                    .cast(),
                std::mem::size_of_val(&standard) as u32,
            )
        } == 0
            || standard.NumberOfLinks != 1
        {
            return Err("Cannot validate operation history identity".into());
        }
        if metadata.len() > LIMIT + MARKER_ALLOWANCE {
            return Err(
                "Operation history is full; preserve it before starting a new history".into(),
            );
        }
        Ok(file)
    }
    fn read_segment(path: &Path, mut file: File) -> Result<Self, String> {
        let mut data = vec![];
        file.read_to_end(&mut data).map_err(|e| e.to_string())?;
        let mut journal = Self {
            file,
            root: path.into(),
            active: path.into(),
            anchor: None,
            _parents: vec![],
            seq: 0,
            previous: String::new(),
            bytes: data.len() as u64,
            snapshot: Snapshot::default(),
            failed: false,
        };
        let mut good = 0;
        for line in data.split_inclusive(|b| *b == b'\n') {
            if line.last() != Some(&b'\n') {
                // A process can die during a write. Preserve its tail before trimming.
                let recovery_id = unsafe { windows::Win32::System::Com::CoCreateGuid() }
                    .map_err(|e| e.to_string())?;
                let tail = path.with_extension(format!("interrupted-{recovery_id:?}.json"));
                let mut recovery = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&tail)
                    .map_err(|e| e.to_string())?;
                recovery
                    .write_all(line)
                    .and_then(|_| recovery.sync_all())
                    .map_err(|e| e.to_string())?;
                journal
                    .file
                    .set_len(good)
                    .and_then(|_| journal.file.sync_all())
                    .map_err(|e| e.to_string())?;
                journal
                    .file
                    .seek(SeekFrom::End(0))
                    .map_err(|e| e.to_string())?;
                journal.bytes = good;
                journal.snapshot.warning = Some(format!(
                    "An incomplete history record was preserved at {}. Interrupted operations were not replayed.",
                    tail.display()
                ));
                break;
            }
            if line.len() > LINE_LIMIT {
                return Err("Operation history record is too large".into());
            }
            let envelope: Envelope<Box<serde_json::value::RawValue>> = serde_json::from_slice(line)
                .map_err(|e| {
                    format!("Operation history is damaged; preserve it for recovery: {e}")
                })?;
            let next = journal
                .seq
                .checked_add(1)
                .ok_or("Operation history sequence overflow")?;
            if envelope.version != 1
                || envelope.seq != next
                || envelope.previous != journal.previous
                || envelope.checksum != digest(envelope.seq, &envelope.previous, &envelope.event)?
            {
                return Err(
                    "Operation history integrity check failed; no operations were replayed".into(),
                );
            }
            // Check the exact serialized event before applying serde defaults.
            // New optional fields must not invalidate earlier checksummed records.
            let event: Event = serde_json::from_str(envelope.event.get())
                .map_err(|e| format!("Operation history contains an unsupported event: {e}"))?;
            journal.snapshot.apply(&event)?;
            journal.seq = envelope.seq;
            journal.previous = envelope.checksum;
            good += line.len() as u64;
        }
        if journal.snapshot.interrupted() != 0 && journal.snapshot.warning.is_none() {
            journal.snapshot.warning = Some(format!(
                "{} interrupted operation(s) need review. No changes were replayed.",
                journal.snapshot.interrupted()
            ));
        }
        Ok(journal)
    }
    pub fn next_job(&self) -> Result<u64, String> {
        self.snapshot
            .jobs
            .last()
            .map(|j| j.id)
            .unwrap_or(self.snapshot.base_job)
            .checked_add(1)
            .ok_or_else(|| "Operation identifier overflow".into())
    }
    pub fn archive(&mut self) -> Result<PathBuf, String> {
        if self.failed {
            return Err(
                "Cannot archive after a history write failure; preserve the files for recovery"
                    .into(),
            );
        }
        if self.snapshot.jobs.is_empty() {
            return Err("There is no current history to archive".into());
        }
        let id =
            unsafe { windows::Win32::System::Com::CoCreateGuid() }.map_err(|e| e.to_string())?;
        let base = self
            .root
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("History filename cannot be represented")?
            .to_owned();
        let name = format!("{base}.segment-{id:?}.jsonl");
        let active = self.root.with_file_name(&name);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .share_mode(1)
            .custom_flags(0x00200000 | 0x00100000)
            .open(&active)
            .map_err(|e| e.to_string())?;
        let mut fresh = Self::read_segment(&active, file)?;
        let previous = self
            .active
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("History filename cannot be represented")?
            .to_owned();
        fresh.append(Event::ArchiveBoundary {
            last_job: self
                .snapshot
                .jobs
                .last()
                .map_or(self.snapshot.base_job, |job| job.id),
            previous,
        })?;
        // A complete unknown event makes older builds fail closed, rather than
        // writing the former active journal after this profile switches files.
        // Append the guard durably before publishing the first segment pointer.
        if self.anchor.is_none() && !self.snapshot.segmented {
            self.append(Event::SegmentedHistory)?;
        }
        // Write and flush the entire new segment before publishing its pointer.
        // Old journals and the writer anchor are never truncated or replaced.
        let pending = self
            .root
            .with_file_name(format!("{base}.pointer-{id:?}.tmp"));
        let mut pointer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .custom_flags(0x00200000 | 0x00100000)
            .open(&pending)
            .map_err(|e| e.to_string())?;
        let manifest = serde_json::to_vec(&Manifest {
            version: 1,
            active: name,
        })
        .map_err(|e| e.to_string())?;
        pointer
            .write_all(&manifest)
            .and_then(|_| pointer.sync_all())
            .map_err(|e| e.to_string())?;
        drop(pointer);
        let destination = self.root.with_extension("current.json");
        let from = seek_engine::paths::extended(&seek_engine::paths::internal(&pending));
        let to = seek_engine::paths::extended(&seek_engine::paths::internal(&destination));
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        if unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(format!(
                "History was preserved, but its active pointer could not be changed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let archived = self.active.clone();
        let old = std::mem::replace(&mut self.file, fresh.file);
        if self.anchor.is_none() {
            self.anchor = Some(old);
        }
        self.active = active;
        self.seq = fresh.seq;
        self.previous = fresh.previous;
        self.bytes = fresh.bytes;
        self.snapshot = fresh.snapshot;
        self.resolve_archive()?;
        self.snapshot.warning = Some(
            "Previous history was preserved. Undo and Put Back now apply to the new history only."
                .into(),
        );
        Ok(archived)
    }
    pub fn append(&mut self, event: Event) -> Result<(), String> {
        if self.failed {
            return Err("Operation history is unavailable after an earlier write failure".into());
        }
        self.snapshot.validate(&event)?;
        let seq = self
            .seq
            .checked_add(1)
            .ok_or("Operation history sequence overflow")?;
        let checksum = digest(seq, &self.previous, &event)?;
        let envelope = Envelope {
            version: 1,
            seq,
            previous: self.previous.clone(),
            event,
            checksum: checksum.clone(),
        };
        let mut bytes = serde_json::to_vec(&envelope).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        let pending = self.snapshot.pending_items;
        let proofs = self.snapshot.pending_proofs;
        let reserve = match &envelope.event {
            Event::Begin { .. } => 3 * LINE_LIMIT as u64,
            Event::Before { proof, .. } => {
                (pending + proofs + 2 + u64::from(*proof)) * LINE_LIMIT as u64
            }
            Event::After { .. } => (pending.max(1) + proofs) * LINE_LIMIT as u64,
            Event::Proof { .. } => proofs.max(1) * LINE_LIMIT as u64,
            _ => 0,
        };
        let limit = LIMIT
            + if matches!(&envelope.event, Event::SegmentedHistory) {
                MARKER_ALLOWANCE
            } else {
                0
            };
        if bytes.len() > LINE_LIMIT || self.bytes + bytes.len() as u64 + reserve > limit {
            return Err("Operation history is nearly full; archive it before starting more work. The next item was stopped before mutation".into());
        }
        if let Err(error) = self
            .file
            .write_all(&bytes)
            .and_then(|_| self.file.sync_all())
        {
            self.failed = true;
            return Err(format!("Cannot record operation safely: {error}"));
        }
        self.seq = seq;
        self.previous = checksum;
        self.bytes += bytes.len() as u64;
        self.snapshot.bytes_used = self.bytes;
        self.snapshot.apply(&envelope.event)?;
        Ok(())
    }
}
/// Completed Shell results; USER_IGNORED/PENDING remain distinct, not inferred successes.
pub fn item_completed(hr: i32) -> bool {
    matches!(
        hr,
        0 | 0x00270006 | 0x00270008 | 0x0027000a | 0x0027000c | 0x0027000e
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn begin(job: u64) -> Event {
        Event::Begin {
            job,
            label: "Move files".into(),
        }
    }
    #[test]
    fn interrupted_review_checks_evidence_without_replaying_or_editing_history() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("history.jsonl");
        let file = temp.path().join("result.txt");
        std::fs::write(&file, b"recorded bytes").unwrap();
        let identity = Fingerprint::at(&file).unwrap();
        let mut journal = Journal::open(&path).unwrap();
        journal.append(begin(1)).unwrap();
        journal
            .append(Event::Before {
                job: 1,
                action: "move".into(),
                source: Some(WirePath::new(&temp.path().join("old.txt"))),
                folder: None,
                name: None,
                proof: false,
            })
            .unwrap();
        journal
            .append(Event::After {
                job: 1,
                action: "move".into(),
                source: Some(WirePath::new(&temp.path().join("old.txt"))),
                target: Some(WirePath::new(&file)),
                parsing_name: None,
                hresult: 0,
                identity: Some(identity),
            })
            .unwrap();
        journal
            .append(Event::Before {
                job: 1,
                action: "move".into(),
                source: Some(WirePath::new(&temp.path().join("unreported.txt"))),
                folder: None,
                name: None,
                proof: false,
            })
            .unwrap();
        drop(journal);
        let bytes = std::fs::read(&path).unwrap();
        let mut journal = Journal::open(&path).unwrap();
        journal.snapshot.review_interrupted().unwrap();
        let notes = &journal.snapshot.jobs[0].review;
        assert_eq!(notes.len(), 2);
        assert!(notes[0].contains("still matches"));
        assert!(notes[1].contains("outcome is unknown"));
        assert!(journal.snapshot.move_plans(false, None).is_none());
        std::fs::write(&file, b"changed").unwrap();
        journal.snapshot.review_interrupted().unwrap();
        assert!(journal.snapshot.jobs[0].review[0].contains("changed or unavailable"));
        assert!(!journal.snapshot.jobs[0].finished);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(std::fs::read(&file).unwrap(), b"changed");
        assert!(!temp.path().join("old.txt").exists());
    }
    #[test]
    fn legacy_records_keep_their_checksum_when_new_fields_default() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("legacy.jsonl");
        let events = [
            r#"{"event":"begin","job":1,"label":"Move files"}"#,
            r#"{"event":"before","job":1,"action":"move","source":null,"folder":null,"name":null}"#,
            r#"{"event":"after","job":1,"action":"move","source":null,"target":null,"parsing_name":null,"hresult":0,"identity":{"volume":1,"id":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"bytes":0,"hash":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}}"#,
            r#"{"event":"end","job":1,"error":null}"#,
        ];
        let mut previous = String::new();
        let mut data = vec![];
        for (index, event) in events.iter().enumerate() {
            let event: Box<serde_json::value::RawValue> = serde_json::from_str(event).unwrap();
            let seq = index as u64 + 1;
            let checksum = digest(seq, &previous, &event).unwrap();
            let envelope = Envelope {
                version: 1,
                seq,
                previous,
                event,
                checksum: checksum.clone(),
            };
            serde_json::to_writer(&mut data, &envelope).unwrap();
            data.push(b'\n');
            previous = checksum;
        }
        std::fs::write(&path, data).unwrap();
        let journal = Journal::open(&path).unwrap();
        assert!(journal.snapshot.jobs[0].finished);
        assert!(matches!(
            &journal.snapshot.jobs[0].outcomes[0],
            Event::Before { proof: false, .. }
        ));
    }
    #[test]
    fn restart_retains_completed_and_interrupted_jobs() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("history.jsonl");
        {
            let mut j = Journal::open(&p).unwrap();
            j.append(begin(1)).unwrap();
            j.append(Event::End {
                job: 1,
                error: None,
            })
            .unwrap();
            j.append(begin(2)).unwrap();
        }
        let j = Journal::open(&p).unwrap();
        assert!(j.snapshot.jobs[0].finished);
        assert_eq!(j.snapshot.interrupted(), 1);
        assert_eq!(j.next_job().unwrap(), 3);
    }
    #[test]
    fn truncated_tail_is_preserved_without_replaying_anything() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("history.jsonl");
        {
            let mut j = Journal::open(&p).unwrap();
            j.append(begin(1)).unwrap();
        }
        OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(b"{broken")
            .unwrap();
        {
            let mut j = Journal::open(&p).unwrap();
            assert_eq!(j.snapshot.interrupted(), 1);
            assert!(j.snapshot.warning.is_some());
            j.append(begin(2)).unwrap();
        }
        assert_eq!(Journal::open(&p).unwrap().snapshot.jobs.len(), 2);
        assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 2);
    }
    #[test]
    fn corruption_in_complete_record_is_not_silently_repaired() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("history.jsonl");
        {
            let mut j = Journal::open(&p).unwrap();
            j.append(begin(1)).unwrap();
        }
        let bytes = std::fs::read(&p).unwrap();
        let mut damaged = bytes.clone();
        let i = damaged.iter().position(|b| *b == b'M').unwrap();
        damaged[i] = b'X';
        std::fs::write(&p, &damaged).unwrap();
        assert!(Journal::open(&p).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), damaged);
    }
    #[test]
    fn bad_order_and_duplicate_completion_do_not_reach_disk() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("history.jsonl");
        let mut j = Journal::open(&p).unwrap();
        assert!(
            j.append(Event::End {
                job: 1,
                error: None
            })
            .is_err()
        );
        assert_eq!(std::fs::metadata(&p).unwrap().len(), 0);
        j.append(begin(1)).unwrap();
        assert!(j.append(begin(1)).is_err());
        j.append(Event::End {
            job: 1,
            error: None,
        })
        .unwrap();
        let len = std::fs::metadata(&p).unwrap().len();
        assert!(
            j.append(Event::End {
                job: 1,
                error: None
            })
            .is_err()
        );
        assert_eq!(std::fs::metadata(&p).unwrap().len(), len);
    }
    #[test]
    fn non_unicode_windows_paths_round_trip_losslessly() {
        let path = WirePath(vec![67, 58, 92, 0xd800, 46, 116, 120, 116]);
        let decoded: WirePath =
            serde_json::from_slice(&serde_json::to_vec(&path).unwrap()).unwrap();
        assert_eq!(WirePath::new(&decoded.path()), path);
    }
    #[test]
    fn linked_history_is_rejected_without_writing_the_target() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("original");
        std::fs::write(&original, b"keep").unwrap();
        let linked = temp.path().join("history");
        std::fs::hard_link(&original, &linked).unwrap();
        assert!(Journal::open(&linked).is_err());
        assert_eq!(std::fs::read(original).unwrap(), b"keep");
    }
    #[test]
    fn shell_completion_codes_distinguish_real_results_from_skips() {
        assert!(item_completed(0));
        assert!(item_completed(0x00270008));
        assert!(item_completed(0x0027000e));
        assert!(!item_completed(0x00270005));
        assert!(!item_completed(0x00270010));
        assert!(!item_completed(-2147024891));
    }
    #[test]
    fn oversized_records_leave_history_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("history.jsonl");
        let mut journal = Journal::open(&path).unwrap();
        assert!(
            journal
                .append(Event::Begin {
                    job: 1,
                    label: "x".repeat(LINE_LIMIT)
                })
                .is_err()
        );
        assert_eq!(std::fs::metadata(path).unwrap().len(), 0);
        assert!(journal.snapshot.jobs.is_empty());
    }
    #[test]
    fn two_writers_cannot_share_one_history() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("history.jsonl");
        let _j = Journal::open(&p).unwrap();
        assert!(Journal::open(&p).is_err());
    }

    #[test]
    fn format_guard_blocks_a_legacy_parser_without_erasing_old_records() {
        #[derive(Deserialize)]
        #[serde(tag = "event", rename_all = "snake_case")]
        enum LegacyEvent {
            Begin,
            End,
        }
        #[derive(Deserialize)]
        struct LegacyEnvelope {
            event: LegacyEvent,
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("history.jsonl");
        let mut journal = Journal::open(&root).unwrap();
        completed(&mut journal, 1);
        let old = std::fs::read(&root).unwrap();
        for line in old.split(|b| *b == b'\n').filter(|line| !line.is_empty()) {
            let parsed: LegacyEnvelope = serde_json::from_slice(line).unwrap();
            assert!(matches!(
                parsed.event,
                LegacyEvent::Begin | LegacyEvent::End
            ));
        }
        journal.archive().unwrap();
        let archived = std::fs::read(&root).unwrap();
        assert!(archived.starts_with(&old));
        let guard = archived[old.len()..]
            .split(|b| *b == b'\n')
            .find(|line| !line.is_empty())
            .unwrap();
        assert!(serde_json::from_slice::<LegacyEnvelope>(guard).is_err());
    }
    #[test]
    fn legacy_full_capacity_still_allows_the_archive_guard() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("history.jsonl");
        let mut journal = Journal::open(&root).unwrap();
        completed(&mut journal, 1);
        journal.bytes = LIMIT;
        journal.archive().unwrap();
        assert!(journal.snapshot.jobs.is_empty());
        assert_eq!(journal.next_job().unwrap(), 2);
        assert!(journal.snapshot.bytes_used < 4096);
    }
    fn completed(journal: &mut Journal, job: u64) {
        journal.append(begin(job)).unwrap();
        journal.append(Event::End { job, error: None }).unwrap();
    }
    #[test]
    fn archive_preserves_bytes_ids_and_writer_lock_across_multiple_segments() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("history.jsonl");
        let mut journal = Journal::open(&root).unwrap();
        completed(&mut journal, 1);
        let original = std::fs::read(&root).unwrap();
        assert_eq!(journal.archive().unwrap(), root);
        assert!(std::fs::read(&root).unwrap().starts_with(&original));
        assert!(journal.snapshot.jobs.is_empty());
        assert_eq!(journal.next_job().unwrap(), 2);
        assert!(
            Journal::open(&root).is_err(),
            "the original writer anchor must remain locked"
        );
        let first_segment = journal.active.clone();
        completed(&mut journal, 2);
        let first_bytes = std::fs::read(&first_segment).unwrap();
        assert_eq!(journal.archive().unwrap(), first_segment);
        assert_eq!(std::fs::read(&first_segment).unwrap(), first_bytes);
        assert_eq!(journal.next_job().unwrap(), 3);
        completed(&mut journal, 3);
        drop(journal);
        let mut reopened = Journal::open(&root).unwrap();
        assert_eq!(reopened.snapshot.jobs.len(), 1);
        assert_eq!(reopened.snapshot.jobs[0].id, 3);
        assert_eq!(reopened.next_job().unwrap(), 4);
        assert_eq!(
            reopened.snapshot.archived_file.as_ref(),
            Some(&first_segment)
        );
        completed(&mut reopened, 4);
        drop(reopened);
        assert_eq!(Journal::open(&root).unwrap().snapshot.jobs.len(), 2);
        assert!(std::fs::read(&root).unwrap().starts_with(&original));
    }
    #[test]
    fn failed_archive_publication_leaves_current_history_usable() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("history.jsonl");
        let mut journal = Journal::open(&root).unwrap();
        completed(&mut journal, 1);
        let original = std::fs::read(&root).unwrap();
        std::fs::create_dir(root.with_extension("current.json")).unwrap();
        assert!(journal.archive().is_err());
        assert!(std::fs::read(&root).unwrap().starts_with(&original));
        completed(&mut journal, 2);
        drop(journal);
        std::fs::remove_dir(root.with_extension("current.json")).unwrap();
        let reopened = Journal::open(&root).unwrap();
        assert_eq!(reopened.snapshot.jobs.len(), 2);
        assert_eq!(reopened.next_job().unwrap(), 3);
    }
    #[test]
    fn damaged_traversing_and_missing_segment_pointers_fail_without_repairing_data() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("history.jsonl");
        let mut journal = Journal::open(&root).unwrap();
        completed(&mut journal, 1);
        journal.archive().unwrap();
        let active = journal.active.clone();
        drop(journal);
        let root_bytes = std::fs::read(&root).unwrap();
        let active_bytes = std::fs::read(&active).unwrap();
        let pointer = root.with_extension("current.json");
        let valid = std::fs::read(&pointer).unwrap();
        let missing = "history.jsonl.segment-missing.jsonl";
        for data in [
            b"{broken".to_vec(),
            serde_json::to_vec(&Manifest {
                version: 1,
                active: "../outside.jsonl".into(),
            })
            .unwrap(),
            serde_json::to_vec(&Manifest {
                version: 1,
                active: missing.into(),
            })
            .unwrap(),
        ] {
            std::fs::write(&pointer, data).unwrap();
            assert!(Journal::open(&root).is_err());
            assert_eq!(std::fs::read(&root).unwrap(), root_bytes);
            assert_eq!(std::fs::read(&active).unwrap(), active_bytes);
            assert!(!root.with_file_name(missing).exists());
        }
        std::fs::write(pointer, valid).unwrap();
        assert_eq!(Journal::open(&root).unwrap().next_job().unwrap(), 2);
    }
    #[test]
    fn unpublished_segment_is_ignored_and_interrupted_records_are_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("history.jsonl");
        let mut journal = Journal::open(&root).unwrap();
        journal.append(begin(1)).unwrap();
        let bytes = std::fs::read(&root).unwrap();
        // Models a process ending after writing a segment but before publishing its pointer.
        let orphan = root.with_file_name("history.jsonl.segment-unpublished.jsonl");
        let file = Journal::locked_file(&orphan, true).unwrap();
        let mut unpublished = Journal::read_segment(&orphan, file).unwrap();
        unpublished
            .append(Event::ArchiveBoundary {
                last_job: 1,
                previous: "history.jsonl".into(),
            })
            .unwrap();
        drop(unpublished);
        drop(journal);
        let mut reopened = Journal::open(&root).unwrap();
        assert_eq!(reopened.snapshot.interrupted(), 1);
        assert_eq!(reopened.archive().unwrap(), root);
        assert!(std::fs::read(&root).unwrap().starts_with(&bytes));
        assert!(orphan.exists());
        assert!(reopened.snapshot.jobs.is_empty());
        assert_eq!(reopened.next_job().unwrap(), 2);
    }
    #[test]
    fn capacity_reserves_completion_space_for_pending_items() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("history.jsonl");
        let mut journal = Journal::open(&root).unwrap();
        journal.append(begin(1)).unwrap();
        // Simulate the logical capacity boundary without requiring disk exhaustion.
        journal.bytes = LIMIT - 4 * LINE_LIMIT as u64;
        let before = || Event::Before {
            job: 1,
            action: "move".into(),
            source: None,
            folder: None,
            name: None,
            proof: false,
        };
        journal.append(before()).unwrap();
        journal.append(before()).unwrap();
        let written = std::fs::read(&root).unwrap();
        assert!(journal.append(before()).is_err());
        assert_eq!(std::fs::read(&root).unwrap(), written);
        for _ in 0..2 {
            journal
                .append(Event::After {
                    job: 1,
                    action: "move".into(),
                    source: None,
                    target: None,
                    parsing_name: None,
                    hresult: 0,
                    identity: None,
                })
                .unwrap();
        }
        journal
            .append(Event::End {
                job: 1,
                error: None,
            })
            .unwrap();
        assert!(journal.snapshot.jobs[0].finished);
        assert_eq!(journal.snapshot.pending_items, 0);
    }
    #[test]
    fn archive_boundary_cannot_reset_ids_or_be_inserted_into_live_history() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("history.jsonl");
        let mut journal = Journal::open(&root).unwrap();
        completed(&mut journal, 5);
        assert!(
            journal
                .append(Event::ArchiveBoundary {
                    last_job: 0,
                    previous: "history.jsonl".into()
                })
                .is_err()
        );
        journal.archive().unwrap();
        assert!(journal.append(begin(5)).is_err());
        assert!(
            journal
                .append(Event::ArchiveBoundary {
                    last_job: 0,
                    previous: "history.jsonl".into()
                })
                .is_err()
        );
        completed(&mut journal, 6);
        assert_eq!(journal.next_job().unwrap(), 7);
    }
}

#[derive(Clone)]
pub struct Context {
    pub journal: std::sync::Arc<std::sync::Mutex<Journal>>,
    pub job: u64,
}
impl Context {
    pub fn append(&self, event: Event) -> windows::core::Result<()> {
        self.journal
            .lock()
            .map_err(|_| {
                windows::core::Error::new(
                    windows::Win32::Foundation::E_ABORT,
                    "Operation history lock failed",
                )
            })?
            .append(event)
            .map_err(|e| windows::core::Error::new(windows::Win32::Foundation::E_ABORT, e))
    }
}
