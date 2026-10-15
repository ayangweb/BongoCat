//! Windows room presentation. Native windows stay on the main thread; model
//! parsing, Cubism preparation and bounded runtime shutdown stay on workers.
use bongocat_model::{CommittedModel, ModelId, ModelPackageLimits, PresetModelCatalog};
use bongocat_online_client::{OnlineClient, OnlineOwner, OnlinePhase};
use bongocat_online_protocol::MemberId;
use bongocat_overlay::{OverlaySessionOptions, ProductOverlaySession};
use bongocat_render::RenderConsumer;
use bongocat_runtime::{OverlaySettings, RuntimeClient, RuntimeCommand, RuntimeOwner};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

type ModelResult = Result<Arc<CommittedModel>, ()>;
type Prepared = Result<(RuntimeClient, RenderConsumer), ()>;
struct RemoteWorker {
    ready: mpsc::Receiver<Prepared>,
    stop: mpsc::Sender<()>,
    done: mpsc::Receiver<bool>,
    join: Option<thread::JoinHandle<()>>,
}
impl RemoteWorker {
    fn start(model: Arc<CommittedModel>) -> Result<Self, ()> {
        let (ready, receiver) = mpsc::sync_channel(1);
        let (stop, stopping) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let join = thread::Builder::new()
            .name("bongocat-room-cat".into())
            .spawn(move || {
                let (owner, consumer) = RuntimeOwner::start_with_rendering(true, 32);
                let client = owner.client();
                let prepared = (|| {
                    client
                        .send(RuntimeCommand::SetMaximumFps(30))
                        .map_err(|_| ())?;
                    client
                        .send(RuntimeCommand::SetOverlaySettings(OverlaySettings {
                            scale_percent: 50,
                            ..OverlaySettings::default()
                        }))
                        .map_err(|_| ())?;
                    let sequence = client
                        .send(RuntimeCommand::ActivateModel(model))
                        .map_err(|_| ())?;
                    let snapshot = client
                        .wait_for_model_preparation(sequence, Duration::from_secs(2))
                        .ok_or(())?;
                    if snapshot.pending_model.is_none() || snapshot.last_command_failure.is_some() {
                        return Err(());
                    }
                    Ok((client, consumer))
                })();
                let successful = prepared.is_ok();
                if ready.send(prepared).is_ok() && successful {
                    let _ = stopping.recv();
                }
                let _ = finished.send(owner.shutdown(Duration::from_secs(2)).is_ok());
            })
            .map_err(|_| ())?;
        Ok(Self {
            ready: receiver,
            stop,
            done,
            join: Some(join),
        })
    }
    fn stop(&self) {
        let _ = self.stop.send(());
    }
    fn finished(&mut self) -> Option<bool> {
        let result = self.done.try_recv().ok()?;
        if let Some(join) = self.join.take() {
            return Some(join.join().is_ok() && result);
        }
        Some(result)
    }
}
struct Peer {
    worker: RemoteWorker,
    client: Option<RuntimeClient>,
    overlay: Option<ProductOverlaySession>,
    slot: usize,
}
impl Peer {
    fn stop(&mut self) {
        if let Some(overlay) = &mut self.overlay {
            let _ = overlay.hide_remote();
        }
        self.worker.stop();
    }
    fn release(&mut self) -> Result<(), String> {
        if let Some(overlay) = self.overlay.take() {
            overlay
                .finish_after_runtime_shutdown()
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
pub(crate) struct OnlineDesktop {
    network: OnlineOwner,
    client: OnlineClient,
    root: PathBuf,
    model: Option<Arc<CommittedModel>>,
    loading: Option<(mpsc::Receiver<ModelResult>, thread::JoinHandle<()>)>,
    peers: BTreeMap<MemberId, Peer>,
    retired: Vec<Peer>,
    generation: u64,
    last_tick: Instant,
}
impl OnlineDesktop {
    pub(crate) fn new(runtime: &RuntimeClient, root: PathBuf) -> Result<Self, String> {
        let network = OnlineOwner::start(runtime.activity_signal())
            .map_err(|_| "online worker unavailable".to_owned())?;
        let client = network.client();
        Ok(Self {
            network,
            client,
            root,
            model: None,
            loading: None,
            peers: BTreeMap::new(),
            retired: Vec::new(),
            generation: 0,
            last_tick: Instant::now(),
        })
    }
    pub(crate) fn settings_handle(&self) -> bongocat_ui_protocol::OnlineSettingsHandle {
        bongocat_ui_protocol::OnlineSettingsHandle::new(OnlineSettingsAdapter(self.client.clone()))
    }
    pub(crate) fn tick(&mut self) -> usize {
        if self.last_tick.elapsed() < Duration::from_millis(33) {
            return self
                .peers
                .values()
                .filter(|peer| peer.overlay.is_some())
                .count();
        }
        self.last_tick = Instant::now();
        let snapshot = self.client.snapshot();
        let active = snapshot.phase == OnlinePhase::Connected;
        let stale: Vec<_> = self
            .peers
            .keys()
            .copied()
            .filter(|id| {
                !active || snapshot.generation != self.generation || !snapshot.members.contains(id)
            })
            .collect();
        for id in stale {
            if let Some(mut peer) = self.peers.remove(&id) {
                peer.stop();
                self.retired.push(peer);
            }
        }
        self.generation = snapshot.generation;
        let mut index = 0;
        while index < self.retired.len() {
            if self.retired[index].worker.finished().is_some() {
                let mut peer = self.retired.swap_remove(index);
                let _ = peer.release();
            } else {
                index += 1;
            }
        }
        if !active {
            let _ = self.client.drain_actions();
            return 0;
        }
        if self.model.is_none() && self.loading.is_none() && snapshot.render_failures.is_empty() {
            let root = self.root.clone();
            let (tx, rx) = mpsc::sync_channel(1);
            if let Ok(join) = thread::Builder::new()
                .name("bongocat-room-model".into())
                .spawn(move || {
                    let result = PresetModelCatalog::open(root, ModelPackageLimits::default())
                        .and_then(|catalog| {
                            catalog.load(&ModelId::parse("standard").expect("fixed model ID"))
                        })
                        .map(Arc::new)
                        .map_err(|_| ());
                    let _ = tx.send(result);
                })
            {
                self.loading = Some((rx, join));
            } else {
                for id in &snapshot.members {
                    if Some(*id) != snapshot.you {
                        self.client.report_render_failure(*id);
                    }
                }
            }
        }
        if let Some((receiver, _)) = &self.loading
            && let Ok(result) = receiver.try_recv()
        {
            if let Some((_, join)) = self.loading.take() {
                let _ = join.join();
            }
            match result {
                Ok(model) => self.model = Some(model),
                Err(()) => {
                    for id in &snapshot.members {
                        if Some(*id) != snapshot.you {
                            self.client.report_render_failure(*id);
                        }
                    }
                }
            }
        }
        if let Some(model) = &self.model {
            for (slot, id) in snapshot
                .members
                .iter()
                .filter(|id| Some(**id) != snapshot.you)
                .enumerate()
            {
                if self.peers.contains_key(id) || snapshot.render_failures.contains(id) {
                    continue;
                }
                match RemoteWorker::start(model.clone()) {
                    Ok(worker) => {
                        self.peers.insert(
                            *id,
                            Peer {
                                worker,
                                client: None,
                                overlay: None,
                                slot,
                            },
                        );
                    }
                    Err(()) => self.client.report_render_failure(*id),
                }
            }
        }
        let mut failed = Vec::new();
        for (id, peer) in &mut self.peers {
            if peer.overlay.is_none()
                && let Ok(prepared) = peer.worker.ready.try_recv()
            {
                match prepared {
                    Ok((client, consumer)) => {
                        let options = OverlaySessionOptions {
                            maximum_fps: 30,
                            scale_percent: 50,
                            taskbar_icon_visible: false,
                            window_bounds: Some(ProductOverlaySession::remote_initial_bounds(
                                peer.slot,
                            )),
                            ..OverlaySessionOptions::default()
                        };
                        match ProductOverlaySession::start_remote(
                            client.clone(),
                            consumer,
                            options,
                            id.0,
                        ) {
                            Ok(overlay) => {
                                peer.client = Some(client);
                                peer.overlay = Some(overlay);
                            }
                            Err(_) => failed.push(*id),
                        }
                    }
                    Err(()) => failed.push(*id),
                }
            }
            if let Some(overlay) = &mut peer.overlay
                && overlay.tick().is_err()
            {
                failed.push(*id);
            }
        }
        for action in self.client.drain_actions() {
            if action.generation == self.generation
                && let Some(client) = self
                    .peers
                    .get(&action.member)
                    .and_then(|peer| peer.client.as_ref())
                && client
                    .send(RuntimeCommand::PlayRemotePaw(action.action))
                    .is_err()
            {
                failed.push(action.member);
            }
        }
        for id in failed {
            self.client.report_render_failure(id);
            if let Some(mut peer) = self.peers.remove(&id) {
                peer.stop();
                self.retired.push(peer);
            }
        }
        self.peers
            .values()
            .filter(|peer| peer.overlay.is_some())
            .count()
    }
    pub(crate) fn request_stop(&mut self) {
        self.client.leave();
        for peer in self.peers.values_mut() {
            if let Some(overlay) = &mut peer.overlay {
                let _ = overlay.hide_remote();
            }
        }
    }
    pub(crate) fn shutdown(mut self) -> Result<(), String> {
        self.close()
    }
    fn close(&mut self) -> Result<(), String> {
        self.client.leave();
        let mut errors = Vec::new();
        if self.network.shutdown().is_err() {
            errors.push("online network shutdown failed".to_owned());
        }
        for (_, mut peer) in std::mem::take(&mut self.peers) {
            peer.stop();
            self.retired.push(peer);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.retired.is_empty() && Instant::now() < deadline {
            let mut index = 0;
            while index < self.retired.len() {
                if let Some(ok) = self.retired[index].worker.finished() {
                    let mut peer = self.retired.swap_remove(index);
                    if !ok {
                        errors.push("remote runtime shutdown failed".to_owned());
                    }
                    if let Err(error) = peer.release() {
                        errors.push(error);
                    }
                } else {
                    index += 1;
                }
            }
            if !self.retired.is_empty() {
                thread::sleep(Duration::from_millis(5));
            }
        }
        if !self.retired.is_empty() {
            errors.push("remote workers did not exit within budget".to_owned());
        }
        if let Some((_, join)) = self.loading.take() {
            while !join.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            if join.is_finished() {
                let _ = join.join();
            } else {
                errors.push("remote model loader did not exit within budget".to_owned());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

struct OnlineSettingsAdapter(OnlineClient);
impl bongocat_ui_protocol::OnlineSettingsCapability for OnlineSettingsAdapter {
    fn snapshot(&self) -> bongocat_ui_protocol::OnlineSnapshot {
        self.0.snapshot()
    }
    fn join(&self, address: String) -> Result<(), bongocat_ui_protocol::OnlineError> {
        self.0.join(address)
    }
    fn leave(&self) {
        self.0.leave();
    }
    fn retry_rendering(&self) {
        self.0.retry_rendering();
    }
}

impl Drop for OnlineDesktop {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
