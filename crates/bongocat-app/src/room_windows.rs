//! Main-thread window/runtime owners for other room members.
use bongocat_app::{RoomMemberModel, RoomSceneHandle};
use bongocat_overlay::{OverlaySessionOptions, OverlayWindowBounds, ProductOverlaySession};
use bongocat_render::RenderConsumer;
use bongocat_runtime::{RuntimeCommand, RuntimeOwner};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

struct MemberWindow {
    plan: RoomMemberModel,
    runtime: Option<RuntimeOwner>,
    frames: Option<RenderConsumer>,
    overlay: Option<ProductOverlaySession>,
    presentation: Option<(String, Option<bongocat_runtime::RoomModelProgress>)>,
}

impl MemberWindow {
    fn prepare(plan: RoomMemberModel) -> Result<Self, String> {
        let (runtime, frames) = RuntimeOwner::start_with_rendering(true, 512);
        runtime
            .client()
            .send(RuntimeCommand::ActivateModelWithBindings {
                model: Arc::clone(&plan.model),
                input_bindings: Arc::clone(&plan.input_bindings),
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            plan,
            runtime: Some(runtime),
            frames: Some(frames),
            overlay: None,
            presentation: None,
        })
    }

    fn stop(mut self) -> Result<(), String> {
        let mut failure = None;
        if let Some(overlay) = self.overlay.as_mut() {
            failure = overlay.stop_input().err().map(|error| error.to_string());
        }
        let result = self
            .runtime
            .take()
            .expect("member runtime owner")
            .shutdown(Duration::from_secs(2))
            .map_err(|error| error.to_string());
        if let Some(overlay) = self.overlay.take()
            && let Err(error) = overlay.finish_after_runtime_shutdown()
        {
            failure.get_or_insert_with(|| error.to_string());
        }
        if let Some(failure) = failure {
            Err(failure)
        } else {
            result.map(|_| ())
        }
    }
}

pub(crate) struct RoomWindows {
    scene: RoomSceneHandle,
    revision: u64,
    members: BTreeMap<String, MemberWindow>,
    origin: OverlayWindowBounds,
}

impl RoomWindows {
    pub(crate) fn new(scene: RoomSceneHandle, origin: OverlayWindowBounds) -> Self {
        Self {
            scene,
            revision: u64::MAX,
            members: BTreeMap::new(),
            origin,
        }
    }

    pub(crate) fn tick(&mut self) -> Vec<String> {
        let mut failures = Vec::new();
        if let Some((revision, plans)) = self.scene.models_since(self.revision) {
            self.revision = revision;
            let obsolete: Vec<_> = self
                .members
                .iter()
                .filter(|(id, _)| !plans.iter().any(|plan| plan.member_id == **id))
                .map(|(id, _)| id.clone())
                .collect();
            for id in obsolete {
                self.scene.unregister(&id);
                if let Some(window) = self.members.remove(&id)
                    && let Err(error) = window.stop()
                {
                    failures.push(error);
                }
            }
            for plan in plans {
                if let Some(window) = self.members.get_mut(&plan.member_id) {
                    if !Arc::ptr_eq(&plan.model, &window.plan.model) {
                        let client = window
                            .runtime
                            .as_ref()
                            .expect("member runtime owner")
                            .client();
                        if let Err(error) = client.send(RuntimeCommand::ActivateModelWithBindings {
                            model: Arc::clone(&plan.model),
                            input_bindings: Arc::clone(&plan.input_bindings),
                        }) {
                            failures.push(error.to_string());
                        } else {
                            window.plan = plan;
                        }
                    }
                    continue;
                }
                if !self.members.contains_key(&plan.member_id) {
                    let id = plan.member_id.clone();
                    match MemberWindow::prepare(plan) {
                        Ok(window) => {
                            self.members.insert(id, window);
                        }
                        Err(error) => failures.push(error),
                    }
                }
            }
        }
        let mut failed = Vec::new();
        let display = bongocat_platform::display_bounds_for_window(
            self.origin.x as f32,
            self.origin.y as f32,
            self.origin.width as f32,
            self.origin.height as f32,
        );
        let member_count = self.members.len();
        for (index, (id, window)) in self.members.iter_mut().enumerate() {
            let client = window
                .runtime
                .as_ref()
                .expect("member runtime owner")
                .client();
            if window.overlay.is_none() {
                let snapshot = client.snapshot();
                if snapshot.last_command_failure.is_some() {
                    failed.push(id.clone());
                    continue;
                }
                if snapshot.pending_model.is_none() {
                    continue;
                }
                let options = OverlaySessionOptions {
                    window_bounds: Some(member_bounds(self.origin, display, index, member_count)),
                    ..OverlaySessionOptions::default()
                };
                match ProductOverlaySession::start_remote(
                    client.clone(),
                    window.frames.take().expect("member render consumer"),
                    options,
                ) {
                    Ok(overlay) => {
                        window.overlay = Some(overlay);
                        if !self.scene.register(id, client.clone()) {
                            failed.push(id.clone());
                            continue;
                        }
                    }
                    Err(error) => {
                        failures.push(error.to_string());
                        failed.push(id.clone());
                        continue;
                    }
                }
            }
            let presentation = self.scene.member_presentation(id);
            if window.presentation.as_ref() != Some(&presentation)
                && client
                    .send(RuntimeCommand::SetRoomMemberPresentation {
                        name: presentation.0.clone(),
                        progress: presentation.1,
                    })
                    .is_ok()
            {
                window.presentation = Some(presentation);
            }
            let _ = client.send(RuntimeCommand::Tick);
            if let Some(overlay) = window.overlay.as_mut()
                && let Err(error) = overlay.tick()
            {
                failures.push(error.to_string());
                failed.push(id.clone());
            }
        }
        for id in failed {
            self.scene.unregister(&id);
            if let Some(window) = self.members.remove(&id)
                && let Err(error) = window.stop()
            {
                failures.push(error);
            }
        }
        failures
    }

    pub(crate) fn shutdown(&mut self) -> Vec<String> {
        let mut failures = Vec::new();
        for (id, window) in std::mem::take(&mut self.members) {
            self.scene.unregister(&id);
            if let Err(error) = window.stop() {
                failures.push(error);
            }
        }
        failures
    }
}

fn member_bounds(
    origin: OverlayWindowBounds,
    display: Option<bongocat_platform::DisplayBounds>,
    index: usize,
    count: usize,
) -> OverlayWindowBounds {
    let Some(display) = display else {
        return origin;
    };
    let columns = count.clamp(1, 4);
    let rows = count.div_ceil(columns).max(1);
    let width = origin
        .width
        .min((display.width / columns as f32) as u32)
        .max(1);
    let height = origin
        .height
        .min((display.height / rows as f32) as u32)
        .max(1);
    OverlayWindowBounds::new(
        display.x as i32 + (index % columns) as i32 * width as i32,
        display.y as i32 + (index / columns) as i32 * height as i32,
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_windows_fit_negative_display_even_when_local_cat_is_at_edge() {
        let display = bongocat_platform::DisplayBounds {
            display_id: None,
            x: -1920.0,
            y: -200.0,
            width: 1920.0,
            height: 1080.0,
        };
        for index in 0..32 {
            let bounds = member_bounds(
                OverlayWindowBounds::new(-300, 700, 400, 300),
                Some(display),
                index,
                32,
            );
            assert!(bounds.x >= -1920 && bounds.y >= -200);
            assert!(bounds.x + bounds.width as i32 <= 0);
            assert!(bounds.y + bounds.height as i32 <= 880);
        }
    }
}
