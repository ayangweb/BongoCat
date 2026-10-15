//! Real Windows overlay smoke. Uses no persistent application storage.
#![forbid(unsafe_code)]

#[cfg(target_os = "windows")]
#[path = "../online_desktop.rs"]
mod online_desktop;

#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
    use bongocat_online_client::{OnlineOwner, OnlinePhase};
    use bongocat_online_protocol::ActivitySignal;
    use bongocat_overlay::{OverlaySessionOptions, ProductOverlaySession};
    use bongocat_runtime::{RuntimeCommand, RuntimeOwner};
    use std::{
        io,
        path::Path,
        sync::{Arc, mpsc},
        thread,
        time::{Duration, Instant},
    };

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
    let (address, receiver) = mpsc::channel();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let server = bongocat_room_server::RoomServer::bind("127.0.0.1:0".parse().unwrap())
                .await
                .unwrap();
            address.send(server.address().unwrap()).unwrap();
            server
                .run(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
    });
    let endpoint = format!("ws://{}/ws", receiver.recv_timeout(Duration::from_secs(5))?);
    let model = Arc::new(
        PresetModelCatalog::open(&root, ModelPackageLimits::default())?
            .load(&ModelId::parse("standard")?)?,
    );
    let (runtime, frames) = RuntimeOwner::start_with_rendering(true, 32);
    let runtime_client = runtime.client();
    let sequence = runtime_client
        .send(RuntimeCommand::ActivateModel(model))
        .map_err(|error| io::Error::other(format!("local model command: {error:?}")))?;
    runtime_client
        .wait_for_model_preparation(sequence, Duration::from_secs(2))
        .ok_or_else(|| io::Error::other("local model preparation deadline"))?;
    let mut overlay = ProductOverlaySession::start(
        runtime_client.clone(),
        runtime.input_producer(),
        runtime_client.cursor_producer(),
        runtime_client.gamepad_axis_producer(),
        frames,
        OverlaySessionOptions::default(),
    )?;
    let mut desktop =
        online_desktop::OnlineDesktop::new(&runtime_client, root).map_err(io::Error::other)?;
    let handle = desktop.settings_handle();
    handle
        .join(endpoint.clone())
        .map_err(|_| io::Error::other("join local room"))?;
    let mut observers = Vec::new();
    for _ in 0..7 {
        let signal = ActivitySignal::default();
        let owner = OnlineOwner::start(signal.clone())
            .map_err(|_| io::Error::other("simulated client startup"))?;
        owner
            .client()
            .join(endpoint.clone())
            .map_err(|_| io::Error::other("simulated join"))?;
        observers.push((owner, signal));
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut visible_frames = 0;
    while Instant::now() < deadline {
        overlay.run_for(Duration::from_millis(40))?;
        if desktop.tick() == 7 {
            visible_frames += 1;
        }
        for (owner, signal) in &observers {
            if owner.client().snapshot().phase == OnlinePhase::Connected {
                signal.mark();
            }
        }
        if visible_frames >= 30 {
            break;
        }
    }
    let snapshot = handle.snapshot();
    let mut errors = Vec::new();
    if visible_frames < 30 || snapshot.members.len() != 8 || !snapshot.render_failures.is_empty() {
        errors.push("eight-member room did not present seven labeled remote cats".to_owned());
    }
    // Exercise rendering retry through the same settings handle.
    handle.retry_rendering();
    handle.leave();
    overlay.run_for(Duration::from_millis(40))?;
    if desktop.tick() != 0 {
        errors.push("remote cats remained after leaving".to_owned());
    }
    desktop.request_stop();
    if let Err(error) = desktop.shutdown() {
        errors.push(error);
    }
    for (mut owner, _) in observers {
        if owner.shutdown().is_err() {
            errors.push("simulated client did not join".to_owned());
        }
    }
    overlay.stop_input()?;
    if runtime.shutdown(Duration::from_secs(2)).is_err() {
        errors.push("local runtime shutdown".to_owned());
    }
    overlay.finish_after_runtime_shutdown()?;
    let _ = stop.send(());
    server
        .join()
        .map_err(|_| io::Error::other("room server shutdown"))?;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(errors.join("; ")).into())
    }
}

#[cfg(target_os = "macos")]
fn main() -> Result<(), std::io::Error> {
    Err(std::io::Error::other(
        "online-smoke requires Windows x86_64",
    ))
}
