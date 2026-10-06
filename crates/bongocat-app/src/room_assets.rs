//! Bounded model acquisition on a worker separate from input transport.
use crate::remote_models::download::{download_package, extract_package, resolve_package_root};
use bongocat_config::ModelInputMode;
use bongocat_model::{ModelId, ModelPackageLimits};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub(crate) const MODEL_CHANNEL: &str = "bongocat-model-v1";
const CHUNK: usize = 1536;
const QUEUE: usize = 128;
const MAX_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PEERS: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Advertisement {
    pub id: String,
    pub title: String,
    pub input_mode: ModelInputMode,
    pub library_url: Option<String>,
    #[serde(default)]
    pub cache_id: Option<String>,
}
impl Advertisement {
    fn cache_key(&self) -> String {
        format!(
            "{:?}:{}",
            self.input_mode,
            self.library_url
                .as_deref()
                .unwrap_or_else(|| self.cache_id.as_deref().unwrap_or(&self.id))
        )
    }
    fn valid(&self) -> bool {
        ModelId::parse(&self.id).is_ok()
            && self
                .cache_id
                .as_ref()
                .is_none_or(|id| ModelId::parse(id).is_ok())
            && !self.title.trim().is_empty()
            && self.title.chars().count() <= 128
            && self
                .library_url
                .as_ref()
                .is_none_or(|url| valid_library_url(url))
    }
}
pub(crate) fn valid_library_url(url: &str) -> bool {
    url.len() <= 2048
        && !url.contains(['\\', '\r', '\n', '#'])
        && [
            "https://github.com/",
            "https://raw.githubusercontent.com/",
            "https://cdn.jsdelivr.net/gh/",
        ]
        .iter()
        .any(|prefix| url.starts_with(prefix))
        && url.ends_with(".zip")
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ModelFrame {
    Offer {
        model: Advertisement,
    },
    Request {
        id: String,
    },
    Begin {
        id: String,
        size: u64,
    },
    Chunk {
        id: String,
        offset: u64,
        data: Vec<u8>,
    },
    End {
        id: String,
    },
    Failed {
        id: String,
    },
}

struct Job {
    room: String,
    member: String,
    frame: ModelFrame,
}
#[derive(Default)]
struct Shared {
    root: Option<PathBuf>,
    room: Option<String>,
    members: BTreeSet<String>,
    local: Option<(Advertisement, PathBuf)>,
    ready_local: Option<Advertisement>,
    inbound: VecDeque<Job>,
    outbound: BTreeMap<String, VecDeque<ModelFrame>>,
    ready: VecDeque<AcquiredModel>,
    installed: BTreeMap<(String, String), String>,
    advertisements: BTreeMap<String, Advertisement>,
    cache: BTreeMap<String, String>,
    progress: BTreeMap<String, (bongocat_runtime::RoomModelProgress, Instant)>,
    generation: u64,
}
#[derive(Clone, Default)]
pub(crate) struct ModelTransfers(Arc<Mutex<Shared>>);
pub(crate) struct AcquiredModel {
    pub room: String,
    pub member: String,
    pub model: Advertisement,
    pub source: PathBuf,
    pub(crate) generation: u64,
    _directory: WorkDirectory,
}
impl ModelTransfers {
    pub fn set_cache(&self, entries: impl IntoIterator<Item = (Advertisement, String)>) {
        let mut s = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        s.cache = entries
            .into_iter()
            .map(|(ad, id)| (ad.cache_key(), id))
            .collect();
    }
    pub fn progress(&self, member: &str) -> Option<bongocat_runtime::RoomModelProgress> {
        let mut s = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if s.progress
            .get(member)
            .is_some_and(|(_, updated)| updated.elapsed() >= Duration::from_secs(30))
        {
            s.progress.remove(member);
        }
        s.progress.get(member).map(|(progress, _)| *progress)
    }
    pub fn failed(&self, room: &str, member: &str) {
        self.report(room, member, None);
    }

    fn report(
        &self,
        room: &str,
        member: &str,
        progress: Option<bongocat_runtime::RoomModelProgress>,
    ) {
        let mut s = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if s.room.as_deref() != Some(room) || !s.members.contains(member) {
            return;
        }
        if let Some(progress) = progress {
            s.progress
                .insert(member.to_owned(), (progress, Instant::now()));
        } else {
            s.progress.remove(member);
        }
    }
    fn cached(&self, member: &str, model: &Advertisement) -> bool {
        let mut s = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(id) = s.cache.get(&model.cache_key()).cloned() {
            s.installed
                .insert((member.to_owned(), model.id.clone()), id);
            s.progress.remove(member);
            return true;
        }
        false
    }
    pub fn configure(&self, root: PathBuf) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .root = Some(root);
    }
    pub fn room(&self, room: Option<&str>, members: BTreeSet<String>) {
        let mut s = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if s.room.as_deref() != room {
            s.generation = s.generation.saturating_add(1);
            s.inbound.clear();
            s.outbound.clear();
            s.ready.clear();
            s.installed.clear();
            s.advertisements.clear();
            s.progress.clear();
        }
        s.room = room.map(str::to_owned);
        s.members = members;
        let members = s.members.clone();
        s.outbound.retain(|id, _| members.contains(id));
        s.progress.retain(|id, _| members.contains(id));
        s.advertisements.retain(|id, _| members.contains(id));
        s.installed.retain(|(id, _), _| members.contains(id));
        s.ready.retain(|item| members.contains(&item.member));
    }
    pub fn remove_member(&self, member: &str) {
        let mut s = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        s.members.remove(member);
        s.progress.remove(member);
        s.advertisements.remove(member);
        s.outbound.remove(member);
        s.inbound.retain(|job| job.member != member);
        s.ready.retain(|item| item.member != member);
        s.installed.retain(|(id, _), _| id != member);
    }
    pub fn advertise_remote(&self, room: &str, member: &str, model: Advertisement) {
        if !model.valid() {
            return;
        }
        let mut s = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if s.room.as_deref() != Some(room)
            || !s.members.contains(member)
            || s.advertisements.get(member) == Some(&model)
            || s.inbound.len() >= QUEUE
        {
            return;
        }
        s.advertisements.insert(member.to_owned(), model.clone());
        s.inbound.push_back(Job {
            room: room.to_owned(),
            member: member.to_owned(),
            frame: ModelFrame::Offer { model },
        });
    }
    pub fn set_local(&self, model: Option<(Advertisement, PathBuf)>) {
        let mut s = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if s.local != model {
            s.local = model;
            s.ready_local = None;
        }
    }
    pub fn local(&self) -> Option<Advertisement> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ready_local
            .clone()
    }
    pub fn receive(&self, room: &str, member: &str, frame: ModelFrame) -> bool {
        let mut s = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if s.room.as_deref() != Some(room)
            || !s.members.contains(member)
            || s.inbound.len() >= QUEUE
        {
            return false;
        }
        s.inbound.push_back(Job {
            room: room.to_owned(),
            member: member.to_owned(),
            frame,
        });
        true
    }
    pub fn outgoing(&self, member: &str) -> Vec<ModelFrame> {
        let mut s = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let q = s.outbound.entry(member.to_owned()).or_default();
        let count = q.len().min(8);
        q.drain(..count).collect()
    }
    pub fn retry(&self, member: &str, frames: Vec<ModelFrame>) {
        let mut s = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let q = s.outbound.entry(member.to_owned()).or_default();
        for frame in frames.into_iter().rev() {
            if q.len() < QUEUE + 8 {
                q.push_front(frame);
            }
        }
    }
    fn send(&self, room: &str, member: &str, frame: ModelFrame) -> bool {
        let mut s = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if s.room.as_deref() != Some(room) || !s.members.contains(member) {
            return false;
        }
        let q = s.outbound.entry(member.to_owned()).or_default();
        if q.len() >= QUEUE {
            return false;
        }
        q.push_back(frame);
        true
    }
    pub fn acquired(&self) -> Vec<AcquiredModel> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ready
            .drain(..)
            .collect()
    }
    pub fn current(&self, item: &AcquiredModel) -> bool {
        let s = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        s.generation == item.generation
            && s.room.as_deref() == Some(&item.room)
            && s.members.contains(&item.member)
    }
    pub fn installed(&self, member: &str, remote: &str, local: String) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .progress
            .remove(member);
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .installed
            .insert((member.to_owned(), remote.to_owned()), local);
    }
    pub fn local_id(&self, member: &str, remote: &str) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .installed
            .get(&(member.to_owned(), remote.to_owned()))
            .cloned()
    }
    pub fn run(&self, stop: &AtomicBool) {
        let mut worker = TransferWorker::default();
        while !stop.load(Ordering::Acquire) {
            let (root, local, generation, room, members, job) = {
                let mut s = self
                    .0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                (
                    s.root.clone(),
                    s.local.clone(),
                    s.generation,
                    s.room.clone(),
                    s.members.clone(),
                    s.inbound.pop_front(),
                )
            };
            if let Some(root) = root {
                worker.tick(
                    self,
                    &root,
                    local,
                    generation,
                    room.as_deref(),
                    &members,
                    job,
                    stop,
                );
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

struct WorkDirectory(PathBuf);
impl WorkDirectory {
    fn new(root: &Path) -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        fs::create_dir_all(root)?;
        if fs::symlink_metadata(root)?.file_type().is_symlink() {
            return Err(std::io::Error::other("linked transfer root"));
        }
        for _ in 0..1024 {
            let path = root.join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::other("transfer directory limit"))
    }
}
impl Drop for WorkDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Receiving {
    model: Advertisement,
    file: fs::File,
    directory: WorkDirectory,
    size: u64,
    received: u64,
    updated: Instant,
    generation: u64,
    room: String,
}
struct Sending {
    id: String,
    file: fs::File,
    offset: u64,
    size: u64,
    room: String,
    updated: Instant,
}
struct TransferWorker {
    http: ureq::Agent,
    local: Option<(Advertisement, PathBuf)>,
    offers: BTreeMap<String, Advertisement>,
    receiving: BTreeMap<String, Receiving>,
    sending: BTreeMap<String, Sending>,
    package: Option<WorkDirectory>,
    generation: u64,
}
impl Default for TransferWorker {
    fn default() -> Self {
        Self {
            http: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(2))
                .timeout_read(Duration::from_secs(2))
                .timeout(Duration::from_secs(10))
                .build(),
            local: None,
            package: None,
            offers: BTreeMap::new(),
            receiving: BTreeMap::new(),
            sending: BTreeMap::new(),
            generation: 0,
        }
    }
}
impl TransferWorker {
    #[allow(clippy::too_many_arguments)]
    fn tick(
        &mut self,
        hub: &ModelTransfers,
        root: &Path,
        local: Option<(Advertisement, PathBuf)>,
        generation: u64,
        room: Option<&str>,
        members: &BTreeSet<String>,
        job: Option<Job>,
        stop: &AtomicBool,
    ) {
        if generation != self.generation {
            self.offers.clear();
            self.receiving.clear();
            self.sending.clear();
            self.generation = generation;
        }
        self.receiving.retain(|id, item| {
            members.contains(id) && item.updated.elapsed() < Duration::from_secs(30)
        });
        self.sending.retain(|id, item| {
            members.contains(id) && item.updated.elapsed() < Duration::from_secs(30)
        });
        self.offers.retain(|id, _| members.contains(id));
        if local != self.local {
            self.sending.clear();
            self.local = local.clone();
            self.package = None;
            if let Some((model, path)) = &local {
                let ready = if model.library_url.is_some() {
                    model.valid()
                } else {
                    WorkDirectory::new(root)
                        .and_then(|dir| {
                            package_directory(path, &dir.0.join("model.zip"), || {
                                stop.load(Ordering::Acquire)
                            })?;
                            self.package = Some(dir);
                            Ok(())
                        })
                        .is_ok()
                };
                if ready {
                    let mut s = hub
                        .0
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if s.local == local {
                        s.ready_local = Some(model.clone());
                    }
                }
            }
        }
        if let Some(job) =
            job.filter(|job| Some(job.room.as_str()) == room && members.contains(&job.member))
        {
            let member = job.member;
            match job.frame {
                ModelFrame::Offer { model } if model.valid() => {
                    if self.offers.get(&member) != Some(&model)
                        && (self.offers.contains_key(&member) || self.offers.len() < MAX_PEERS)
                    {
                        self.receiving.remove(&member);
                        self.offers.insert(member.clone(), model.clone());
                        if hub.cached(&member, &model) {
                            return;
                        }
                        hub.report(
                            &job.room,
                            &member,
                            Some(bongocat_runtime::RoomModelProgress::Downloading {
                                percent: None,
                            }),
                        );
                        if let Some(url) = &model.library_url {
                            if let Ok(dir) = WorkDirectory::new(root) {
                                let archive = dir.0.join("model.zip");
                                if download_package(
                                    &self.http,
                                    std::slice::from_ref(url),
                                    &archive,
                                    MAX_BYTES,
                                    |received, total| {
                                        hub.report(
                                            &job.room,
                                            &member,
                                            Some(
                                                bongocat_runtime::RoomModelProgress::Downloading {
                                                    percent: total.filter(|total| *total > 0).map(
                                                        |total| {
                                                            ((received.saturating_mul(100) / total)
                                                                .min(100))
                                                                as u8
                                                        },
                                                    ),
                                                },
                                            ),
                                        )
                                    },
                                    || stop.load(Ordering::Acquire),
                                )
                                .is_ok()
                                {
                                    self.complete(hub, &job.room, &member, model, dir, generation);
                                } else {
                                    hub.report(&job.room, &member, None);
                                }
                            } else {
                                hub.report(&job.room, &member, None);
                            }
                        } else {
                            let _ =
                                hub.send(&job.room, &member, ModelFrame::Request { id: model.id });
                        }
                    }
                }
                ModelFrame::Request { id } => {
                    if self
                        .local
                        .as_ref()
                        .is_some_and(|(model, _)| model.id == id && model.library_url.is_none())
                        && self.sending.len() < MAX_PEERS
                        && let Some(dir) = &self.package
                        && let Ok(file) = fs::File::open(dir.0.join("model.zip"))
                        && let Ok(meta) = file.metadata()
                        && hub.send(
                            &job.room,
                            &member,
                            ModelFrame::Begin {
                                id: id.clone(),
                                size: meta.len(),
                            },
                        )
                    {
                        self.sending.insert(
                            member,
                            Sending {
                                id,
                                file,
                                offset: 0,
                                size: meta.len(),
                                room: job.room,
                                updated: Instant::now(),
                            },
                        );
                    }
                }
                ModelFrame::Begin { id, size } if size > 0 && size <= MAX_BYTES => {
                    if self.receiving.values().map(|item| item.size).sum::<u64>() + size
                        <= MAX_BYTES
                        && let Some(model) = self
                            .offers
                            .get(&member)
                            .filter(|model| model.id == id && model.library_url.is_none())
                            .cloned()
                    {
                        self.receiving.remove(&member);
                        if let Ok(dir) = WorkDirectory::new(root)
                            && let Ok(file) = fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(dir.0.join("model.zip"))
                        {
                            self.receiving.insert(
                                member,
                                Receiving {
                                    model,
                                    directory: dir,
                                    file,
                                    size,
                                    received: 0,
                                    updated: Instant::now(),
                                    generation,
                                    room: job.room,
                                },
                            );
                        }
                    }
                }
                ModelFrame::Chunk { id, offset, data } => {
                    if let Some(item) = self.receiving.get_mut(&member)
                        && item.model.id == id
                    {
                        if offset != item.received
                            || data.is_empty()
                            || data.len() > CHUNK
                            || item.received + data.len() as u64 > item.size
                            || item.file.write_all(&data).is_err()
                        {
                            self.receiving.remove(&member);
                            hub.report(&job.room, &member, None);
                        } else {
                            item.received += data.len() as u64;
                            item.updated = Instant::now();
                            hub.report(
                                &item.room,
                                &member,
                                Some(bongocat_runtime::RoomModelProgress::Downloading {
                                    percent: Some(
                                        (item.received.saturating_mul(100) / item.size).min(100)
                                            as u8,
                                    ),
                                }),
                            );
                        }
                    }
                }
                ModelFrame::End { id } => {
                    if self
                        .receiving
                        .get(&member)
                        .is_some_and(|item| item.model.id == id)
                        && let Some(item) = self.receiving.remove(&member)
                        && item.received == item.size
                    {
                        let Receiving {
                            model,
                            directory,
                            mut file,
                            generation,
                            room,
                            ..
                        } = item;
                        if file.flush().is_ok() {
                            drop(file);
                            self.complete(hub, &room, &member, model, directory, generation);
                        }
                    }
                }
                ModelFrame::Failed { id }
                    if self
                        .receiving
                        .get(&member)
                        .is_some_and(|item| item.model.id == id) =>
                {
                    self.receiving.remove(&member);
                    hub.report(&job.room, &member, None);
                }
                _ => {}
            }
        }
        let ids: Vec<_> = self.sending.keys().cloned().collect();
        for member in ids {
            let item = self.sending.get_mut(&member).expect("sender entry");
            for _ in 0..8 {
                let frame = if item.offset == item.size {
                    ModelFrame::End {
                        id: item.id.clone(),
                    }
                } else {
                    let mut data = vec![0; CHUNK.min((item.size - item.offset) as usize)];
                    if item.file.read_exact(&mut data).is_err() {
                        self.sending.remove(&member);
                        break;
                    }
                    ModelFrame::Chunk {
                        id: item.id.clone(),
                        offset: item.offset,
                        data,
                    }
                };
                if !hub.send(&item.room, &member, frame.clone()) {
                    // Leave the file offset unchanged when queue pressure delays this chunk.
                    use std::io::{Seek, SeekFrom};
                    let _ = item.file.seek(SeekFrom::Start(item.offset));
                    break;
                }
                item.updated = Instant::now();
                if matches!(frame, ModelFrame::End { .. }) {
                    self.sending.remove(&member);
                    break;
                }
                if let ModelFrame::Chunk { data, .. } = frame {
                    item.offset += data.len() as u64;
                }
            }
        }
    }
    fn complete(
        &self,
        hub: &ModelTransfers,
        room: &str,
        member: &str,
        model: Advertisement,
        dir: WorkDirectory,
        generation: u64,
    ) {
        hub.report(
            room,
            member,
            Some(bongocat_runtime::RoomModelProgress::Installing),
        );
        let package = dir.0.join("package");
        if extract_package(&dir.0.join("model.zip"), &package, &limits()).is_err() {
            hub.report(room, member, None);
            return;
        }
        let source = resolve_package_root(&package);
        let mut s = hub
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if s.room.as_deref() == Some(room)
            && s.generation == generation
            && s.members.contains(member)
            && s.ready.len() < MAX_PEERS
        {
            s.ready.push_back(AcquiredModel {
                room: room.to_owned(),
                member: member.to_owned(),
                model,
                source,
                generation,
                _directory: dir,
            });
        }
    }
}
fn limits() -> ModelPackageLimits {
    ModelPackageLimits {
        maximum_package_bytes: MAX_BYTES,
        maximum_file_bytes: MAX_BYTES,
        ..Default::default()
    }
}
fn package_directory(
    root: &Path,
    destination: &Path,
    is_stopped: impl Fn() -> bool,
) -> std::io::Result<()> {
    let root = root.canonicalize()?;
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut zip = zip::ZipWriter::new(file);
    let mut pending = vec![root.clone()];
    let mut count = 0;
    let mut total = 0u64;
    while let Some(dir) = pending.pop() {
        if is_stopped() {
            return Err(std::io::Error::other("sharing stopped"));
        }
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            let meta = fs::symlink_metadata(&path)?;
            let relative = path.strip_prefix(&root).map_err(std::io::Error::other)?;
            if meta.file_type().is_symlink() || relative.components().count() > 32 {
                return Err(std::io::Error::other("invalid shared path"));
            }
            count += 1;
            if count > 4096 {
                return Err(std::io::Error::other("shared entry limit"));
            }
            if meta.is_dir() {
                pending.push(path);
                continue;
            }
            if !meta.is_file() {
                return Err(std::io::Error::other("invalid shared file"));
            }
            total = total.saturating_add(meta.len());
            if count > 4096 || total > MAX_BYTES {
                return Err(std::io::Error::other("shared package limit"));
            }
            zip.start_file(
                relative.to_string_lossy().replace('\\', "/"),
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(std::io::Error::other)?;
            let mut input = fs::File::open(&path)?.take(MAX_BYTES + 1);
            let mut copied = 0;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                if is_stopped() {
                    return Err(std::io::Error::other("sharing stopped"));
                }
                let read = input.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                copied += read as u64;
                if copied > meta.len() {
                    return Err(std::io::Error::other("shared file changed"));
                }
                zip.write_all(&buffer[..read])?;
            }
            if copied != meta.len() {
                return Err(std::io::Error::other("shared file changed"));
            }
        }
    }
    let file = zip.finish().map_err(std::io::Error::other)?;
    if file.metadata()?.len() > MAX_BYTES {
        return Err(std::io::Error::other("archive limit"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pump(hub: &ModelTransfers, worker: &mut TransferWorker) {
        let (root, local, generation, room, members, job) = {
            let mut s = hub.0.lock().unwrap();
            (
                s.root.clone().unwrap(),
                s.local.clone(),
                s.generation,
                s.room.clone(),
                s.members.clone(),
                s.inbound.pop_front(),
            )
        };
        worker.tick(
            hub,
            &root,
            local,
            generation,
            room.as_deref(),
            &members,
            job,
            &AtomicBool::new(false),
        );
    }

    #[test]
    fn shared_package_installs_with_new_id_and_displays_without_changing_local_selection() {
        use crate::BUILD_ENVIRONMENT;
        use bongocat_config::StorageLayout;
        use bongocat_ui_protocol::{
            SettingsModelKey, SettingsModelOrigin, SettingsRoomMember, SettingsRoomView,
        };
        let temp = tempfile::tempdir().unwrap();
        let sender = ModelTransfers::default();
        let scene = crate::RoomSceneHandle::default();
        let receiver = scene.1.clone();
        sender.configure(temp.path().join("send"));
        receiver.configure(temp.path().join("receive"));
        sender.room(Some("room"), BTreeSet::from(["guest".into()]));
        receiver.room(Some("room"), BTreeSet::from(["host".into()]));
        let model = Advertisement {
            id: "remote-model".into(),
            title: "Shared cat".into(),
            input_mode: ModelInputMode::Standard,
            library_url: None,
            cache_id: None,
        };
        sender.set_local(Some((
            model.clone(),
            crate::tests::repository_preset_root().join("standard"),
        )));
        let mut tx = TransferWorker::default();
        let mut rx = TransferWorker::default();
        pump(&sender, &mut tx);
        assert_eq!(sender.local(), Some(model.clone()));
        assert!(receiver.receive("room", "host", ModelFrame::Offer { model }));
        for _ in 0..10000 {
            pump(&sender, &mut tx);
            for _ in 0..8 {
                pump(&receiver, &mut rx);
            }
            for frame in sender.outgoing("guest") {
                assert!(receiver.receive("room", "host", frame));
            }
            for frame in receiver.outgoing("host") {
                assert!(sender.receive("room", "guest", frame));
            }
            if !receiver.0.lock().unwrap().ready.is_empty() {
                break;
            }
        }
        assert_eq!(
            receiver.0.lock().unwrap().ready.len(),
            1,
            "complete model, not only metadata"
        );
        let layout = StorageLayout::under(temp.path().join("app"), BUILD_ENVIRONMENT);
        let mut app = crate::Application::start_with_layout(layout.clone()).unwrap();
        let selection = app.config().model.selected_model.clone();
        let room = SettingsRoomView {
            room_id: "room".into(),
            name: "room".into(),
            member_count: 1,
            max_members: 32,
            has_password: false,
            members: vec![SettingsRoomMember {
                model_visible: true,
                model_download: None,
                id: "host".into(),
                name: "Host".into(),
                model_name: Some("Shared cat".into()),
                model_key: Some(SettingsModelKey {
                    id: "remote-model".into(),
                    origin: SettingsModelOrigin::Imported,
                }),
                is_host: true,
                is_self: false,
            }],
        };
        scene.synchronize(&mut app, Some(&room));
        let plans = scene.models_since(0).unwrap().1;
        assert_eq!(plans.len(), 1);
        assert_eq!(
            plans[0].model.origin(),
            bongocat_model::ModelOrigin::Installed
        );
        assert_ne!(plans[0].model.id().as_str(), "remote-model");
        let local_id = plans[0].model.id().as_str().to_owned();
        assert_eq!(app.config().model.selected_model, selection);
        assert!(
            app.config()
                .model
                .imported_models
                .iter()
                .any(|record| record.id == local_id && record.title == "Shared cat")
        );
        scene.synchronize(&mut app, Some(&room));
        assert_eq!(app.config().model.imported_models.len(), 1);
        assert_eq!(
            app.config().model.imported_models[0]
                .shared_model_id
                .as_deref(),
            Some("remote-model")
        );
        app.shutdown().unwrap();
        let mut app = crate::Application::start_with_layout(layout.clone()).unwrap();
        let cached = ModelTransfers::default();
        cached.configure(temp.path().join("cached"));
        app.refresh_room_cache(&cached);
        cached.room(Some("next"), BTreeSet::from(["another-member".into()]));
        let cached_model = Advertisement {
            id: "remote-model".into(),
            title: "Shared cat".into(),
            input_mode: ModelInputMode::Standard,
            library_url: None,
            cache_id: None,
        };
        cached.receive(
            "next",
            "another-member",
            ModelFrame::Offer {
                model: cached_model,
            },
        );
        pump(&cached, &mut TransferWorker::default());
        assert_eq!(
            cached.local_id("another-member", "remote-model").as_deref(),
            Some(local_id.as_str())
        );
        assert!(
            cached.outgoing("another-member").is_empty(),
            "cached P2P model needs no request"
        );
        assert!(cached.acquired().is_empty());
        assert!(cached.progress("another-member").is_none());
        assert_eq!(app.config().model.imported_models.len(), 1);
        app.record_library_source(
            &local_id,
            Some("https://github.com/a/b/raw/main/cat.zip".into()),
        )
        .unwrap();
        app.shutdown().unwrap();
        let app = crate::Application::start_with_layout(layout).unwrap();
        assert_eq!(
            app.config().model.imported_models[0].library_url.as_deref(),
            Some("https://github.com/a/b/raw/main/cat.zip")
        );
        let cached = ModelTransfers::default();
        cached.configure(temp.path().join("cached-url"));
        app.refresh_room_cache(&cached);
        cached.room(Some("next"), BTreeSet::from(["host".into()]));
        cached.receive(
            "next",
            "host",
            ModelFrame::Offer {
                model: Advertisement {
                    id: "different-id".into(),
                    title: "Shared cat".into(),
                    input_mode: ModelInputMode::Standard,
                    library_url: Some("https://github.com/a/b/raw/main/cat.zip".into()),
                    cache_id: None,
                },
            },
        );
        pump(&cached, &mut TransferWorker::default());
        assert_eq!(
            cached.local_id("host", "different-id").as_deref(),
            Some(local_id.as_str())
        );
        assert!(cached.outgoing("host").is_empty());
        assert!(
            cached.acquired().is_empty(),
            "URL cache avoids network and repeated import"
        );
        app.shutdown().unwrap();
    }

    #[test]
    fn library_offer_downloads_without_peer_request_and_failure_keeps_default() {
        use std::net::TcpListener;
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("model.json"), b"{}").unwrap();
        let archive = temp.path().join("library.zip");
        package_directory(&source, &archive, || false).unwrap();
        let bytes = fs::read(archive).unwrap();
        for success in [true, false] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let local_url = format!("http://{}/model.zip", listener.local_addr().unwrap());
            let body = bytes.clone();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request).unwrap();
                let status = if success {
                    "200 OK"
                } else {
                    "500 Internal Server Error"
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            });
            let url = "https://github.com/author/models/raw/main/cat.zip";
            // ureq's middleware contract requires its unboxed external error type.
            #[allow(clippy::result_large_err)]
            let http = ureq::AgentBuilder::new()
                .middleware(move |request: ureq::Request, next: ureq::MiddlewareNext| {
                    assert_eq!(request.url(), url);
                    next.handle(ureq::get(&local_url))
                })
                .build();
            let hub = ModelTransfers::default();
            hub.configure(temp.path().join(if success { "good" } else { "bad" }));
            hub.room(Some("r"), BTreeSet::from(["host".into()]));
            let model = Advertisement {
                id: "library-cat".into(),
                title: "Catalog cat".into(),
                input_mode: ModelInputMode::Standard,
                library_url: Some(url.into()),
                cache_id: None,
            };
            hub.advertise_remote("r", "host", model);
            let mut worker = TransferWorker {
                http,
                ..Default::default()
            };
            pump(&hub, &mut worker);
            server.join().unwrap();
            let acquired = hub.acquired();
            assert_eq!(acquired.len(), usize::from(success));
            assert!(
                hub.outgoing("host").is_empty(),
                "library download must not require P2P"
            );
            if let Some(item) = acquired.first() {
                assert_eq!(item.model.library_url.as_deref(), Some(url));
                assert_eq!(fs::read(item.source.join("model.json")).unwrap(), b"{}");
            }
        }
    }

    #[test]
    fn rejects_untrusted_urls_and_bounds_membership_queue() {
        for url in [
            "http://github.com/a.zip",
            "https://github.com.evil/a.zip",
            "https://127.0.0.1/a.zip",
            "https://github.com/a.zip#x",
        ] {
            assert!(!valid_library_url(url));
        }
        assert!(valid_library_url(
            "https://github.com/a/b/raw/main/model.zip"
        ));
        let hub = ModelTransfers::default();
        hub.room(Some("r"), BTreeSet::from(["a".into()]));
        assert!(!hub.receive("old", "a", ModelFrame::Request { id: "x".into() }));
        assert!(!hub.receive("r", "stranger", ModelFrame::Request { id: "x".into() }));
        for _ in 0..QUEUE {
            assert!(hub.receive("r", "a", ModelFrame::Request { id: "x".into() }));
        }
        assert!(!hub.receive("r", "a", ModelFrame::Request { id: "x".into() }));
        hub.room(None, BTreeSet::new());
        assert!(hub.0.lock().unwrap().inbound.is_empty());
    }
    #[test]
    fn package_roundtrip_and_incomplete_transfer_do_not_publish() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("model.json"), b"{}").unwrap();
        let archive = temp.path().join("model.zip");
        package_directory(&source, &archive, || false).unwrap();
        let output = temp.path().join("out");
        extract_package(&archive, &output, &limits()).unwrap();
        assert_eq!(fs::read(output.join("model.json")).unwrap(), b"{}");
        let hub = ModelTransfers::default();
        hub.configure(temp.path().join("transfers"));
        hub.room(Some("r"), BTreeSet::from(["a".into()]));
        let model = Advertisement {
            id: "model".into(),
            title: "Cat".into(),
            input_mode: ModelInputMode::Standard,
            library_url: None,
            cache_id: None,
        };
        let mut worker = TransferWorker::default();
        let stop = AtomicBool::new(false);
        let root = temp.path().join("transfers");
        for frame in [
            ModelFrame::Offer { model },
            ModelFrame::Begin {
                id: "model".into(),
                size: 10,
            },
            ModelFrame::Chunk {
                id: "model".into(),
                offset: 1,
                data: vec![1],
            },
            ModelFrame::End { id: "model".into() },
        ] {
            worker.tick(
                &hub,
                &root,
                None,
                1,
                Some("r"),
                &BTreeSet::from(["a".into()]),
                Some(Job {
                    room: "r".into(),
                    member: "a".into(),
                    frame,
                }),
                &stop,
            );
        }
        assert!(hub.acquired().is_empty());
        assert!(worker.receiving.is_empty());
        assert!(hub.progress("a").is_none());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        let chunk = ModelFrame::Chunk {
            id: "x".repeat(128),
            offset: MAX_BYTES,
            data: vec![255; CHUNK],
        };
        assert!(serde_json::to_vec(&chunk).unwrap().len() <= 8192);
    }
}
