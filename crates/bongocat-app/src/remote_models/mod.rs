//! The remote model library worker: one thread that fetches the catalog
//! document, fetches its preview images, and turns one chosen entry into an
//! unpacked package the settings service can import.
//!
//! The worker owns no business state. Everything it learns is published into a
//! [`RemoteModelsState`] the settings service reads when it builds a snapshot,
//! and the one handoff that crosses threads — a downloaded package ready to be
//! imported — goes back through the settings command channel, because the model
//! store may only be touched on the thread that owns it.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use bongocat_model::{ModelPackageLimits, RemoteModelEntry, entry_id};
use bongocat_ui_protocol::{
    SettingsClient, SettingsRemoteCatalogStatus, SettingsRemoteModelEntry,
    SettingsRemoteModelFailure, SettingsRemoteModelImportRequest, SettingsRemoteModelStatus,
    SettingsRemoteModels, SettingsRemotePreview, SettingsRemotePreviewImage,
};

use crate::app_log::{
    ApplicationLogCode, ApplicationLogContext, ApplicationLogEvent, ApplicationLogHandle,
};

use self::catalog::CatalogFetcher;
use self::download::{DownloadFailure, download_package, extract_package};

mod catalog;
pub(crate) mod download;

/// The directory under the environment's storage root where downloads unpack.
pub(crate) const REMOTE_MODELS_DIRECTORY_NAME: &str = "remote-models";

/// How often the worker wakes to check its stop flag between jobs.
const JOB_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// The smallest gap between two download-progress publications.
const PROGRESS_PUBLISH_INTERVAL: Duration = Duration::from_millis(300);

/// The bounded shapes one downloaded model package may take. The model store
/// re-validates everything on import; these bounds stop an archive from turning
/// into unbounded disk usage before the store ever sees it.
fn package_limits() -> ModelPackageLimits {
    ModelPackageLimits::default()
}

/// The last published remote model library state, and the version that tells the
/// snapshot clock when it moved.
///
/// The worker is the only writer; the settings service clones the published
/// state into a snapshot and compares versions while deciding whether the
/// revision moved. Alongside the projection the worker keeps the catalog it
/// parsed, because the URLs an entry is fetched from are worker facts the
/// settings window never needs.
#[derive(Clone, Default)]
pub(crate) struct RemoteModelsState {
    inner: Arc<Mutex<RemoteModelsInner>>,
}

#[derive(Default)]
struct RemoteModelsInner {
    version: u64,
    state: SettingsRemoteModels,
    catalog: Vec<RemoteModelEntry>,
}

impl RemoteModelsState {
    pub(crate) fn snapshot(&self) -> SettingsRemoteModels {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .clone()
    }

    pub(crate) fn version(&self) -> u64 {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .version
    }

    /// The parsed catalog entry behind a published identity, with its URLs.
    pub(crate) fn catalog_entry(&self, id: u64) -> Option<RemoteModelEntry> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .catalog
            .iter()
            .find(|entry| entry_id(&entry.download_url) == id)
            .cloned()
    }

    fn update(&self, apply: impl FnOnce(&mut RemoteModelsInner)) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        apply(&mut inner);
        inner.version = inner.version.saturating_add(1);
    }

    pub(crate) fn set_entry_status(&self, id: u64, status: SettingsRemoteModelStatus) {
        self.update(|inner| {
            if let Some(entry) = inner.state.entries.iter_mut().find(|entry| entry.id == id) {
                entry.status = status;
            }
        });
    }

    pub(crate) fn set_entry_preview(&self, id: u64, preview: SettingsRemotePreview) {
        self.update(|inner| {
            if let Some(entry) = inner.state.entries.iter_mut().find(|entry| entry.id == id) {
                entry.preview = preview;
            }
        });
    }
}

/// One unit of work the settings service hands the worker.
pub(crate) enum RemoteModelsJob {
    Refresh,
    Download { id: u64 },
}

/// Starts the worker thread. The service owns the join handle and the stop flag;
/// stopping is cooperative and bounded by the HTTP read timeouts.
pub(crate) fn spawn_remote_worker(
    downloads_root: PathBuf,
    jobs: async_channel::Receiver<RemoteModelsJob>,
    state: RemoteModelsState,
    client: SettingsClient,
    log: ApplicationLogHandle,
    stop: Arc<AtomicBool>,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("bongocat-remote-models".to_owned())
        .spawn(move || run_worker(downloads_root, jobs, state, client, log, stop))
}

fn run_worker(
    downloads_root: PathBuf,
    jobs: async_channel::Receiver<RemoteModelsJob>,
    state: RemoteModelsState,
    client: SettingsClient,
    log: ApplicationLogHandle,
    stop: Arc<AtomicBool>,
) {
    let _ = fs::create_dir_all(&downloads_root);
    clear_stale_downloads(&downloads_root);
    let agent = catalog::build_agent();
    let download_in_flight = Arc::new(AtomicBool::new(false));
    let mut download_handles: Vec<thread::JoinHandle<()>> = Vec::new();
    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }
        match jobs.try_recv() {
            Ok(RemoteModelsJob::Refresh) => {
                refresh_catalog(&agent, &state, &stop);
            }
            Ok(RemoteModelsJob::Download { id }) => {
                if !download_in_flight.load(Ordering::Acquire) {
                    download_in_flight.store(true, Ordering::Release);
                    let session = DownloadSession {
                        agent: agent.clone(),
                        state: state.clone(),
                        client: client.clone(),
                        log: log.clone(),
                        stop: Arc::clone(&stop),
                        downloads_root: downloads_root.clone(),
                        in_flight: Arc::clone(&download_in_flight),
                    };
                    download_handles.push(thread::spawn(move || session.run(id)));
                }
            }
            Err(async_channel::TryRecvError::Empty) => {
                thread::sleep(JOB_POLL_INTERVAL);
            }
            Err(async_channel::TryRecvError::Closed) => break,
        }
    }
    stop.store(true, Ordering::Release);
    for handle in download_handles.drain(..) {
        let _ = handle.join();
    }
}

/// Downloads from a previous run never survive into this one: the directory is
/// per-entry and disposable, and an unpacked package the settings service never
/// imported is worthless on the next launch.
fn clear_stale_downloads(downloads_root: &Path) {
    let Ok(entries) = fs::read_dir(downloads_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let _ = fs::remove_dir_all(path);
        } else {
            let _ = fs::remove_file(path);
        }
    }
}

fn refresh_catalog(agent: &ureq::Agent, state: &RemoteModelsState, stop: &AtomicBool) {
    state.update(|inner| inner.state.catalog = SettingsRemoteCatalogStatus::Loading);
    let fetcher = CatalogFetcher::new(agent);
    let Some(document) = fetcher.fetch_document(&is_stopped(stop)) else {
        state.update(|inner| inner.state.catalog = SettingsRemoteCatalogStatus::Failed);
        return;
    };
    let entries = bongocat_model::parse_remote_library(&document);
    let previous = state.snapshot();
    let projected: Vec<SettingsRemoteModelEntry> = entries
        .iter()
        .map(|entry| project_entry(entry, &previous))
        .collect();
    state.update(|inner| {
        inner.state.catalog = SettingsRemoteCatalogStatus::Ready;
        inner.state.entries = projected;
        inner.catalog = entries;
    });
    fetch_previews(&fetcher, state, stop);
}

/// Carries in-flight and finished facts across a refresh: a download that is
/// running or a model that already installed is about this session, not about
/// the document revision, so a refresh must not reset them.
fn project_entry(
    entry: &RemoteModelEntry,
    previous: &SettingsRemoteModels,
) -> SettingsRemoteModelEntry {
    let id = entry_id(&entry.download_url);
    let previous = previous.entry(id);
    let status = match previous.map(|previous| &previous.status) {
        Some(
            status @ (SettingsRemoteModelStatus::Downloading { .. }
            | SettingsRemoteModelStatus::Importing(_)
            | SettingsRemoteModelStatus::Installed),
        ) => status.clone(),
        _ => SettingsRemoteModelStatus::Available,
    };
    let preview = previous.map_or(SettingsRemotePreview::Pending, |previous| {
        previous.preview.clone()
    });
    SettingsRemoteModelEntry {
        id,
        name: entry.name.clone(),
        author: entry.author.clone(),
        preview,
        status,
    }
}

fn fetch_previews(fetcher: &CatalogFetcher, state: &RemoteModelsState, stop: &AtomicBool) {
    let entries = state.snapshot().entries;
    for entry in entries {
        if stop.load(Ordering::Acquire) {
            return;
        }
        if !matches!(entry.preview, SettingsRemotePreview::Pending) {
            continue;
        }
        let Some(url) = state
            .catalog_entry(entry.id)
            .and_then(|catalog| catalog.preview_url)
        else {
            state.set_entry_preview(entry.id, SettingsRemotePreview::Unavailable);
            continue;
        };
        let preview = match fetcher.fetch_preview(&url, &is_stopped(stop)) {
            Some((format, bytes)) => {
                SettingsRemotePreview::Ready(SettingsRemotePreviewImage::new(format, bytes))
            }
            None => SettingsRemotePreview::Unavailable,
        };
        state.set_entry_preview(entry.id, preview);
    }
}

fn is_stopped(stop: &AtomicBool) -> impl Fn() -> bool + '_ {
    move || stop.load(Ordering::Acquire)
}

/// One download, from the HTTP read to the handoff to the settings service.
struct DownloadSession {
    agent: ureq::Agent,
    state: RemoteModelsState,
    client: SettingsClient,
    log: ApplicationLogHandle,
    stop: Arc<AtomicBool>,
    downloads_root: PathBuf,
    in_flight: Arc<AtomicBool>,
}

impl DownloadSession {
    fn run(self, id: u64) {
        let Some(entry) = self.state.catalog_entry(id) else {
            // The refresh that raced this job removed the entry the request was
            // made against. Nothing is running, so the card must not read as a
            // download that never reports back.
            self.state
                .set_entry_status(id, SettingsRemoteModelStatus::Available);
            self.in_flight.store(false, Ordering::Release);
            return;
        };
        let result = self.download_and_unpack(id, &entry);
        match result {
            Ok(()) => {}
            // A stop is the application shutting down; the next launch cleans
            // the directory and the entry state is not worth publishing.
            Err(DownloadFailure::Stopped) => {}
            Err(failure) => {
                // The failure must land in the log before the entry reports it:
                // the card can only say "failed", and diagnosing a download is
                // otherwise impossible once the per-entry directory is gone.
                self.log.record(
                    ApplicationLogEvent::new(ApplicationLogCode::ModelOperationFailed)
                        .with_context(ApplicationLogContext::Operation("remote_model_download"))
                        .with_context(ApplicationLogContext::Reason(failure.as_str())),
                );
                self.state
                    .set_entry_status(id, SettingsRemoteModelStatus::Failed(failure.into()));
                let _ = fs::remove_dir_all(self.entry_dir(id));
            }
        }
        self.in_flight.store(false, Ordering::Release);
    }

    fn download_and_unpack(
        &self,
        id: u64,
        entry: &RemoteModelEntry,
    ) -> Result<(), DownloadFailure> {
        let entry_dir = self.entry_dir(id);
        let mut urls = vec![entry.download_url.clone()];
        if let Some(fallback) = catalog::fallback_download_url(&entry.download_url) {
            urls.push(fallback);
        }
        let zip_path = entry_dir.join("package.zip");
        let state = self.state.clone();
        let stop = Arc::clone(&self.stop);
        let mut last_publish: Option<Instant> = None;
        download_package(
            &self.agent,
            &urls,
            &zip_path,
            package_limits().maximum_package_bytes,
            |downloaded, total| {
                let due =
                    last_publish.is_none_or(|last| last.elapsed() >= PROGRESS_PUBLISH_INTERVAL);
                if due {
                    last_publish = Some(Instant::now());
                    state.set_entry_status(
                        id,
                        SettingsRemoteModelStatus::Downloading {
                            downloaded_bytes: downloaded,
                            total_bytes: total,
                        },
                    );
                }
            },
            is_stopped(&stop),
        )?;
        if self.stop.load(Ordering::Acquire) {
            return Err(DownloadFailure::Stopped);
        }
        let unpacked = entry_dir.join("package");
        extract_package(&zip_path, &unpacked, &package_limits())?;
        let source_root = download::resolve_package_root(&unpacked);
        self.client
            .notify_remote_model_downloaded(SettingsRemoteModelImportRequest {
                id,
                title: entry.name.clone(),
                source_root,
            })
            .map_err(|_| DownloadFailure::Stopped)
    }

    fn entry_dir(&self, id: u64) -> PathBuf {
        self.downloads_root.join(id.to_string())
    }
}

impl From<DownloadFailure> for SettingsRemoteModelFailure {
    fn from(failure: DownloadFailure) -> Self {
        match failure {
            DownloadFailure::DownloadFailed => Self::DownloadFailed,
            DownloadFailure::DownloadTooLarge => Self::DownloadTooLarge,
            DownloadFailure::InvalidPackage | DownloadFailure::Stopped => Self::ImportFailed,
        }
    }
}
