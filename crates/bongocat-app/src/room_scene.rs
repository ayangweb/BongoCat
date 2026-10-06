//! Prepared room models and member-ID routing. GPU/window owners stay on the main thread.
use crate::Application;
use bongocat_model::{CommittedModel, ModelCatalogEntry, ModelId, ModelOrigin};
use bongocat_runtime::{RuntimeClient, RuntimeCommand};
use bongocat_ui_protocol::{SettingsModelKey, SettingsModelOrigin, SettingsRoomView};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct RoomMemberModel {
    pub member_id: String,
    pub model: Arc<CommittedModel>,
    pub input_bindings: Arc<bongocat_input::InputBindings>,
}

#[derive(Default)]
struct Scene {
    room_id: Option<String>,
    models: BTreeMap<String, RoomMemberModel>,
    advertised: BTreeMap<String, Option<SettingsModelKey>>,
    clients: BTreeMap<String, RuntimeClient>,
    pending_chat: BTreeMap<String, (String, String)>,
    revision: u64,
    names: BTreeMap<String, String>,
    hidden: std::collections::BTreeSet<String>,
    self_name: String,
}

#[derive(Clone, Default)]
pub struct RoomSceneHandle(
    Arc<Mutex<Scene>>,
    pub(crate) crate::room_assets::ModelTransfers,
);

impl RoomSceneHandle {
    #[cfg(test)]
    pub(crate) fn register_test_client(&self, member_id: String, client: RuntimeClient) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clients
            .insert(member_id, client);
    }
    pub(crate) fn member_client(&self, member_id: &str) -> Option<RuntimeClient> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clients
            .get(member_id)
            .cloned()
    }
    pub(crate) fn set_member_visible(&self, member: &str, visible: bool) {
        let mut scene = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let changed = if visible {
            scene.hidden.remove(member)
        } else {
            scene.hidden.insert(member.to_owned())
        };
        if changed {
            scene.revision = scene.revision.saturating_add(1);
        }
    }
    pub(crate) fn member_visible(&self, member: &str) -> bool {
        !self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .hidden
            .contains(member)
    }
    pub fn member_presentation(
        &self,
        member: &str,
    ) -> (String, Option<bongocat_runtime::RoomModelProgress>) {
        let name = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .names
            .get(member)
            .cloned()
            .unwrap_or_default();
        (name, self.1.progress(member))
    }
    pub fn models_since(&self, revision: u64) -> Option<(u64, Vec<RoomMemberModel>)> {
        let scene = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (scene.revision != revision).then(|| {
            (
                scene.revision,
                scene
                    .models
                    .iter()
                    .filter(|(id, _)| !scene.hidden.contains(*id))
                    .map(|(_, model)| model.clone())
                    .collect(),
            )
        })
    }

    pub fn register(&self, member_id: &str, client: RuntimeClient) -> bool {
        let mut scene = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !scene.models.contains_key(member_id) {
            return false;
        }
        if let Some((sender, content)) = scene.pending_chat.remove(member_id) {
            let _ = client.send(RuntimeCommand::ShowChatBubble { sender, content });
        }
        scene.clients.insert(member_id.to_owned(), client);
        true
    }

    pub fn unregister(&self, member_id: &str) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clients
            .remove(member_id);
    }

    pub(crate) fn show_chat(&self, member_id: &str, sender: String, content: String) {
        let mut scene = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !scene.advertised.contains_key(member_id) && scene.pending_chat.len() >= 32 {
            return;
        }
        if let Some(client) = scene.clients.get(member_id) {
            let _ = client.send(RuntimeCommand::ShowChatBubble { sender, content });
        } else {
            scene
                .pending_chat
                .insert(member_id.to_owned(), (sender, content));
        }
    }

    pub(crate) fn clear(&self) {
        self.1.room(None, Default::default());
        let mut scene = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let revision = scene.revision.saturating_add(1);
        let self_name = std::mem::take(&mut scene.self_name);
        *scene = Scene {
            revision,
            self_name,
            ..Scene::default()
        };
    }

    pub(crate) fn remove_member(&self, member_id: &str) {
        self.1.remove_member(member_id);
        let mut scene = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        scene.models.remove(member_id);
        scene.names.remove(member_id);
        scene.hidden.remove(member_id);
        scene.advertised.remove(member_id);
        scene.clients.remove(member_id);
        scene.pending_chat.remove(member_id);
        scene.revision = scene.revision.saturating_add(1);
    }

    /// Called by the settings owner. Filesystem/model parsing never runs on the UI executor.
    pub(crate) fn synchronize(
        &self,
        application: &mut Application,
        room: Option<&SettingsRoomView>,
    ) {
        let self_name = room
            .and_then(|room| room.self_member())
            .map(|member| member.name.clone())
            .unwrap_or_default();
        {
            let mut scene = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if scene.self_name != self_name
                && application
                    .runtime_client()
                    .send(RuntimeCommand::SetRoomMemberPresentation {
                        name: self_name.clone(),
                        progress: None,
                    })
                    .is_ok()
            {
                scene.self_name = self_name;
            }
        }
        self.1.room(
            room.map(|room| room.room_id.as_str()),
            room.into_iter()
                .flat_map(|room| room.members.iter())
                .filter(|member| !member.is_self)
                .take(32)
                .map(|member| member.id.clone())
                .collect(),
        );
        if room.is_some() {
            application.prepare_room_share(&self.1);
        } else {
            self.1.set_local(None);
        }
        let mut acquired_changed = false;
        for acquired in self.1.acquired() {
            if !self.1.current(&acquired)
                || !room.is_some_and(|room| {
                    room.members.iter().any(|member| {
                        member.id == acquired.member
                            && member.model_key.as_ref().is_some_and(|key| {
                                key.origin == SettingsModelOrigin::Imported
                                    && key.id == acquired.model.id
                            })
                    })
                })
            {
                continue;
            }
            match application.import_room_model(&acquired, &self.1) {
                Ok(id) => {
                    self.1.installed(&acquired.member, &acquired.model.id, id);
                    acquired_changed = true;
                }
                Err(error) => {
                    self.1.failed(&acquired.room, &acquired.member);
                    application.record_log(
                        crate::ApplicationLogEvent::new(
                            crate::ApplicationLogCode::ModelOperationFailed,
                        )
                        .with_context(crate::ApplicationLogContext::Operation("room_model_import"))
                        .with_context(crate::ApplicationLogContext::Reason(error.stable_code())),
                    );
                }
            }
        }
        let Some(room) = room else {
            let occupied = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .room_id
                .is_some();
            if occupied {
                self.clear();
            }
            return;
        };
        let advertised: BTreeMap<_, _> = room
            .members
            .iter()
            .filter(|member| !member.is_self)
            .take(32)
            .map(|member| (member.id.clone(), member.model_key.clone()))
            .collect();
        let old_models = {
            let mut scene = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            scene.names = room
                .members
                .iter()
                .filter(|m| !m.is_self)
                .map(|m| (m.id.clone(), m.name.clone()))
                .collect();
            if !acquired_changed
                && scene.room_id.as_deref() == Some(&room.room_id)
                && scene.advertised == advertised
                && advertised.iter().all(|(id, key)| {
                    key.as_ref().is_none_or(|key| {
                        self.1.local_id(id, &key.id).is_none_or(|local| {
                            scene
                                .models
                                .get(id)
                                .is_some_and(|model| model.model.id().as_str() == local)
                        })
                    })
                })
            {
                return;
            }
            if scene.room_id.as_deref() != Some(&room.room_id) {
                scene.clients.clear();
                if scene.room_id.is_some() {
                    scene.pending_chat.clear();
                }
                scene.models.clear();
                scene.hidden.clear();
            }
            scene.room_id = Some(room.room_id.clone());
            scene.advertised = advertised.clone();
            scene.clients.retain(|id, _| advertised.contains_key(id));
            scene
                .pending_chat
                .retain(|id, _| advertised.contains_key(id));
            scene.models.clone()
        };
        let catalog = application.model_catalog().unwrap_or_default();
        let default = ModelId::parse("standard").expect("fixed preset identity");
        let mut prepared = BTreeMap::new();
        for (id, name) in &advertised {
            let resolved = name
                .as_ref()
                .and_then(|key| self.1.local_id(id, &key.id))
                .map(|id| SettingsModelKey {
                    id,
                    origin: SettingsModelOrigin::Imported,
                });
            let candidate = select_local_model(&catalog, resolved.as_ref().or(name.as_ref()));
            let model = old_models
                .get(id)
                .filter(|old| old.model.id() == &candidate.1 && old.model.origin() == candidate.0)
                .map(|old| Arc::clone(&old.model))
                .or_else(|| {
                    application
                        .load_room_model(candidate.0, &candidate.1)
                        .ok()
                        .map(Arc::new)
                })
                .or_else(|| {
                    application
                        .load_room_model(ModelOrigin::Preset, &default)
                        .ok()
                        .map(Arc::new)
                });
            if let Some(model) = model {
                let input_bindings = Arc::new(
                    crate::model_input::input_bindings_for_committed_model(&model),
                );
                prepared.insert(
                    id.clone(),
                    RoomMemberModel {
                        member_id: id.clone(),
                        model,
                        input_bindings,
                    },
                );
            }
        }
        let mut scene = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // A disconnect during preparation invalidates the entire result.
        if scene.room_id.as_deref() == Some(&room.room_id) && scene.advertised == advertised {
            scene.models = prepared;
            scene.revision = scene.revision.saturating_add(1);
        }
    }
}

fn select_local_model(
    catalog: &[ModelCatalogEntry],
    key: Option<&SettingsModelKey>,
) -> (ModelOrigin, ModelId) {
    let mut matches = catalog.iter().filter(|entry| {
        key.is_some_and(|key| {
            entry.id().as_str() == key.id
                && entry.origin()
                    == match key.origin {
                        SettingsModelOrigin::BuiltIn => ModelOrigin::Preset,
                        SettingsModelOrigin::Imported => ModelOrigin::Installed,
                    }
        }) && entry.snapshot().is_some()
    });
    if let Some(entry) = matches.next()
        && matches.next().is_none()
    {
        return (entry.origin(), entry.id().clone());
    }
    (
        ModelOrigin::Preset,
        ModelId::parse("standard").expect("fixed preset identity"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_runtime::RuntimeOwner;
    use std::time::Duration;

    fn plan(id: &str) -> RoomMemberModel {
        let model = Arc::new(
            bongocat_model::PresetModelCatalog::open(
                crate::tests::repository_preset_root(),
                bongocat_model::ModelPackageLimits::default(),
            )
            .unwrap()
            .load(&ModelId::parse("standard").unwrap())
            .unwrap(),
        );
        RoomMemberModel {
            member_id: id.to_owned(),
            input_bindings: Arc::new(crate::model_input::input_bindings_for_committed_model(
                &model,
            )),
            model,
        }
    }

    #[test]
    fn hiding_one_member_preserves_other_models_and_chat() {
        let scene = RoomSceneHandle::default();
        {
            let mut inner = scene.0.lock().unwrap();
            for id in ["a", "b"] {
                inner.models.insert(id.into(), plan(id));
                inner.advertised.insert(id.into(), None);
            }
        }
        scene.set_member_visible("a", false);
        let (revision, visible) = scene.models_since(0).unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].member_id, "b");
        scene.show_chat("a", "A".into(), "hidden message".into());
        assert!(scene.0.lock().unwrap().pending_chat.contains_key("a"));
        scene.set_member_visible("a", true);
        assert_eq!(scene.models_since(revision).unwrap().1.len(), 2);
        scene.remove_member("a");
        assert!(
            scene.member_visible("a"),
            "a departed identity cannot inherit hidden state"
        );
    }

    #[test]
    fn same_named_members_route_by_id_and_leave_clears_pending_messages() {
        let scene = RoomSceneHandle::default();
        for id in ["a", "b"] {
            let plan = plan(id);
            let mut inner = scene.0.lock().unwrap();
            inner.models.insert(id.to_owned(), plan);
            inner.advertised.insert(id.to_owned(), None);
        }
        scene.show_chat("a", "same-name".to_owned(), "first".to_owned());
        scene.show_chat("a", "same-name".to_owned(), "latest".to_owned());
        assert_eq!(scene.0.lock().unwrap().pending_chat["a"].1, "latest");
        let a = RuntimeOwner::start(false, 16);
        let b = RuntimeOwner::start(false, 16);
        assert!(scene.register("a", a.client()));
        assert!(scene.register("b", b.client()));
        assert_eq!(a.client().snapshot().command_transport.enqueued, 1);
        assert_eq!(b.client().snapshot().command_transport.enqueued, 0);
        scene.show_chat("b", "same-name".to_owned(), "hello".to_owned());
        assert_eq!(b.client().snapshot().command_transport.enqueued, 1);
        scene.remove_member("a");
        assert!(!scene.register("a", a.client()));
        assert!(!scene.0.lock().unwrap().pending_chat.contains_key("a"));
        scene.clear();
        assert!(scene.0.lock().unwrap().clients.is_empty());
        assert!(scene.models_since(0).unwrap().1.is_empty());
        a.shutdown(Duration::from_secs(2)).unwrap();
        b.shutdown(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn unknown_and_ambiguous_models_fall_back_to_standard() {
        let entry = |origin| ModelCatalogEntry::Ready {
            origin,
            snapshot: bongocat_model::ModelSnapshot {
                id: ModelId::parse("keyboard").unwrap(),
                entry: "model.model3.json".to_owned(),
                behaviors: Vec::new(),
            },
        };
        let installed = entry(ModelOrigin::Installed);
        assert_eq!(
            select_local_model(
                std::slice::from_ref(&installed),
                Some(&SettingsModelKey {
                    id: "keyboard".to_owned(),
                    origin: SettingsModelOrigin::Imported
                })
            )
            .0,
            ModelOrigin::Installed
        );
        for catalog in [
            vec![],
            vec![installed.clone()],
            vec![installed.clone(), installed],
        ] {
            let name = if catalog.len() == 2 {
                Some(&SettingsModelKey {
                    id: "keyboard".to_owned(),
                    origin: SettingsModelOrigin::Imported,
                })
            } else {
                Some(&SettingsModelKey {
                    id: "missing".to_owned(),
                    origin: SettingsModelOrigin::Imported,
                })
            };
            let selected = select_local_model(&catalog, name);
            assert_eq!(selected.0, ModelOrigin::Preset);
            assert_eq!(selected.1.as_str(), "standard");
        }
    }

    #[test]
    fn unregistered_chat_storage_is_bounded_and_cleared_on_disconnect() {
        let scene = RoomSceneHandle::default();
        for i in 0..100 {
            scene.show_chat(&i.to_string(), "sender".to_owned(), "content".to_owned());
        }
        assert_eq!(scene.0.lock().unwrap().pending_chat.len(), 32);
        scene.clear();
        assert!(scene.0.lock().unwrap().pending_chat.is_empty());
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "opens two real D3D11 member windows"]
    fn remote_windows_render_chat_without_starting_local_input() {
        use bongocat_overlay::{OverlaySessionOptions, OverlayWindowBounds, ProductOverlaySession};
        let scene = RoomSceneHandle::default();
        let mut sessions = Vec::new();
        for (index, id) in ["a", "b"].into_iter().enumerate() {
            let plan = plan(id);
            {
                let mut inner = scene.0.lock().unwrap();
                inner.models.insert(id.to_owned(), plan.clone());
                inner.advertised.insert(id.to_owned(), None);
            }
            let (runtime, frames) = RuntimeOwner::start_with_rendering(true, 64);
            let client = runtime.client();
            client
                .send(RuntimeCommand::ActivateModelWithBindings {
                    model: plan.model,
                    input_bindings: plan.input_bindings,
                })
                .unwrap();
            for _ in 0..100 {
                if client.snapshot().pending_model.is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(
                client.snapshot().pending_model.is_some(),
                "model prepared for GPU commit"
            );
            let overlay = ProductOverlaySession::start_remote(
                client.clone(),
                frames,
                OverlaySessionOptions {
                    window_bounds: Some(OverlayWindowBounds::new(
                        50 + index as i32 * 350,
                        50,
                        320,
                        320,
                    )),
                    ..OverlaySessionOptions::default()
                },
            )
            .unwrap();
            assert!(scene.register(id, client.clone()));
            client
                .send(RuntimeCommand::SetRoomMemberPresentation {
                    name: format!("玩家 {id}"),
                    progress: Some(bongocat_runtime::RoomModelProgress::Downloading {
                        percent: Some(50 + index as u8 * 25),
                    }),
                })
                .unwrap();
            sessions.push((runtime, overlay));
        }
        scene.show_chat("a", "same".to_owned(), "A".to_owned());
        scene.show_chat("b", "same".to_owned(), "B".to_owned());
        for _ in 0..60 {
            for (runtime, overlay) in &mut sessions {
                runtime.client().send(RuntimeCommand::Tick).unwrap();
                overlay.tick().unwrap();
                assert!(overlay.is_visible());
                assert_eq!(
                    runtime
                        .client()
                        .snapshot()
                        .platform_input
                        .service_start_attempts,
                    0
                );
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        scene.clear();
        for (runtime, mut overlay) in sessions {
            overlay.stop_input().unwrap();
            runtime.shutdown(Duration::from_secs(2)).unwrap();
            let report = overlay.finish_after_runtime_shutdown().unwrap();
            assert!(report.frames_presented > 1);
        }
    }
}
