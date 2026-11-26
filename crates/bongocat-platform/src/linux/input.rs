use crate::PlatformInputError;
use bongocat_input::*;
use bongocat_input_helper::protocol::{ButtonState, HEADER, InputMessage, PACKET_SIZE};
use std::{
    fs::{self, File},
    io::{ErrorKind, Read},
    os::fd::AsRawFd,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Instant,
};

pub struct LinuxInputService {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<PlatformInputDiagnostics, PlatformInputError>>>,
}
impl LinuxInputService {
    pub fn start_with_diagnostics(
        producer: InputProducer,
        cursor: CursorProducer,
        axes: GamepadAxisProducer,
        diagnostics: PlatformInputDiagnosticsProducer,
    ) -> Result<Self, PlatformInputError> {
        let executable =
            std::env::current_exe().map_err(|_| PlatformInputError::BackendUnavailable)?;
        // The helper runs unprivileged whenever this user can open the devices
        // itself, which is what membership of the `input` group (or a udev rule
        // granting the same access) provides: no authentication dialog on every
        // start, and no process holding root for the whole session. Only a user
        // who cannot read a single event device pays for pkexec. The probe is
        // the one upstream PR #1096 takes for its own evdev reader — list the
        // event nodes and try to open one — and it is asked here rather than
        // left to the helper because libinput silently skips devices its
        // interface cannot open: a helper with no readable device would idle
        // forever instead of failing, so the parent decides before spawning
        // which launch the user's permissions can support.
        let mut command = if any_openable_event_device(Path::new(INPUT_DEVICES_DIRECTORY)) {
            // Same executable, same helper, same argument: only the launcher
            // differs, and only the elevated one goes through polkit.
            Command::new(&executable)
        } else {
            let mut elevated = Command::new("/usr/bin/pkexec");
            elevated.arg("--disable-internal-agent").arg(&executable);
            elevated
        };
        command.arg("--input-helper");
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| PlatformInputError::BackendUnavailable)?;
        let control = child.stdin.take();
        let mut output = child
            .stdout
            .take()
            .ok_or(PlatformInputError::BackendUnavailable)?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("bongocat-linux-input".into())
            .spawn(move || {
                let mut state = PlatformInputDiagnostics {
                    service_start_attempts: 1,
                    ..Default::default()
                };
                let _started = Instant::now();
                let mut gamepad = None;
                let result = (|| {
                    let mut header = [0; HEADER.len()];
                    if !read_packet(&mut output, &mut header, &worker_stop)? {
                        return Ok(());
                    }
                    if &header != HEADER {
                        return Err(PlatformInputError::BackendUnavailable);
                    }
                    super::INPUT_AUTHORIZED.store(true, Ordering::Release);
                    state.service_status = PlatformInputServiceStatus::Running;
                    let _ = diagnostics.publish(state);
                    gamepad = Some(crate::gilrs_gamepad::GilrsGamepad::new(
                        producer.clone(),
                        axes,
                    ));
                    let gamepad = gamepad.as_mut().unwrap();
                    let mut packet = [0; PACKET_SIZE];
                    let mut enabled = false;
                    let mut position = CursorPosition { x: 0.5, y: 0.5 };
                    while read_packet(&mut output, &mut packet, &worker_stop)? {
                        let at = MonotonicMillis::now();
                        let message = InputMessage::decode(&packet)
                            .ok_or(PlatformInputError::BackendUnavailable)?;
                        if let InputMessage::Reset { enabled: active } = message {
                            enabled = active;
                        }
                        let event = match message {
                            InputMessage::Reset { .. } => Some(InputEvent::Reset {
                                reason: InputResetReason::PermissionChanged,
                                at,
                            }),
                            InputMessage::Key { code, state } => key_usage(code).map(|usage| {
                                edge(
                                    InputControl::Key(PhysicalKey::from_hid_usage(usage)),
                                    state == ButtonState::Pressed,
                                    at,
                                )
                            }),
                            InputMessage::Button { code, state } => {
                                mouse_button(code).map(|button| {
                                    edge(
                                        InputControl::Mouse(button),
                                        state == ButtonState::Pressed,
                                        at,
                                    )
                                })
                            }
                            InputMessage::Motion { dx, dy } => {
                                // Accumulate each axis independently and discard excess travel
                                // at the boundary, so reversing direction responds immediately.
                                let sensitivity =
                                    f64::from(super::POINTER_SENSITIVITY.load(Ordering::Relaxed))
                                        / 100.0;
                                let next_position = CursorPosition {
                                    x: (position.x + dx * sensitivity / 40.0).clamp(0.0, 1.0),
                                    y: (position.y + dy * sensitivity / 40.0).clamp(0.0, 1.0),
                                };
                                if let Ok(sample) = CursorSample::new(
                                    next_position,
                                    CursorViewport {
                                        origin: CursorPosition { x: 0.0, y: 0.0 },
                                        width: 1.0,
                                        height: 1.0,
                                    },
                                    at,
                                ) {
                                    position = next_position;
                                    let _ = cursor.publish(sample);
                                }
                                None
                            }
                            InputMessage::Heartbeat { .. } => None,
                        };
                        if let Some(event) = event {
                            state.captured_edges +=
                                u64::from(matches!(event, InputEvent::Edge { .. }));
                            match producer.publish(event) {
                                Ok(_) => state.queued_edges += 1,
                                Err(InputPublishError::QueueFull(_)) => {
                                    state.runtime_queue_overflows += 1;
                                    return Err(PlatformInputError::BackendUnavailable);
                                }
                                Err(InputPublishError::RuntimeStopped(_)) => {
                                    return Err(PlatformInputError::RuntimeStopped);
                                }
                            }
                        }
                        if matches!(message, InputMessage::Reset { enabled: true }) {
                            gamepad
                                .reseed(at, &mut state)
                                .map_err(|_| PlatformInputError::RuntimeStopped)?;
                        }
                        if enabled {
                            gamepad
                                .drain(at, &mut state)
                                .map_err(|_| PlatformInputError::RuntimeStopped)?;
                        }
                        let _ = diagnostics.publish(state);
                    }
                    Ok(())
                })();
                if let Some(gamepad) = gamepad.as_mut() {
                    let shutdown = gamepad.shutdown();
                    state.gamepad_disconnections += shutdown.disconnected;
                    state.clean_shutdown = shutdown.backend_clean;
                }
                super::INPUT_AUTHORIZED.store(false, Ordering::Release);
                // A full bounded runtime queue must not swallow the final release reset.
                let deadline = Instant::now() + std::time::Duration::from_secs(1);
                loop {
                    match producer.recover(InputResetReason::ServiceRestart, MonotonicMillis::now())
                    {
                        Ok(_) => break,
                        Err(InputPublishError::RuntimeStopped(_)) => break,
                        Err(InputPublishError::QueueFull(_)) => {
                            state.runtime_queue_overflows += 1;
                            if Instant::now() >= deadline {
                                state.clean_shutdown = false;
                                break;
                            }
                            thread::sleep(std::time::Duration::from_millis(2));
                        }
                    }
                }
                state.service_status = match &result {
                    Ok(()) => PlatformInputServiceStatus::Stopped,
                    Err(PlatformInputError::PermissionDenied) => {
                        PlatformInputServiceStatus::PermissionDenied
                    }
                    Err(PlatformInputError::BackendUnavailable) => {
                        PlatformInputServiceStatus::BackendUnavailable
                    }
                    Err(_) => PlatformInputServiceStatus::Failed,
                };
                state.service_error_code = result.as_ref().err().map(|e| e.as_str());
                let _ = diagnostics.publish(state);
                drop(control);
                drop(output);
                reap(&mut child);
                result.map(|()| state)
            })
            .map_err(|_| PlatformInputError::WorkerPanicked)?;
        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }
    pub fn stop(&mut self) -> Result<PlatformInputDiagnostics, PlatformInputError> {
        self.stop.store(true, Ordering::Release);
        match self.worker.take() {
            Some(worker) => worker
                .join()
                .map_err(|_| PlatformInputError::WorkerPanicked)?,
            None => Ok(PlatformInputDiagnostics::default()),
        }
    }
}
impl Drop for LinuxInputService {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
/// Where the kernel exposes the evdev devices the helper's libinput context
/// opens. Probed before the helper is spawned, so the constant lives next to
/// the launch decision that consumes it.
const INPUT_DEVICES_DIRECTORY: &str = "/dev/input";

/// Whether this user can open the evdev devices without any elevation.
///
/// The startup flow asks this before showing the permission explanation: a
/// user whose group membership already opens the devices needs neither the
/// explanation nor the pkexec authentication the confirmation would go on to
/// start, so the answer decides whether either appears at all. This mirrors
/// upstream PR #1096, whose Linux flow never prompts and treats readable evdev
/// access as simply the state input runs in.
pub fn unprivileged_input_is_possible() -> bool {
    any_openable_event_device(Path::new(INPUT_DEVICES_DIRECTORY))
}

/// Whether at least one event device under `directory` opens for this user.
///
/// Probed with a plain open because that is the exact permission the helper's
/// libinput context needs. Nothing here checks group names: a udev rule can
/// grant the same access without a group, and a group the devices are not
/// actually readable through would be worthless to trust.
fn any_openable_event_device(directory: &Path) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("event"))
        })
        .any(|path| File::open(&path).is_ok())
}

fn reap(child: &mut Child) {
    // Cancels an outstanding pkexec authentication owned by this user. Once
    // elevated, stdin EOF instead terminates the helper; kill may be denied.
    let _ = child.kill();
    let _ = child.wait();
}
fn read_packet(
    reader: &mut std::process::ChildStdout,
    bytes: &mut [u8],
    stop: &AtomicBool,
) -> Result<bool, PlatformInputError> {
    let mut offset = 0;
    while offset < bytes.len() {
        if stop.load(Ordering::Acquire) {
            return Ok(false);
        }
        let mut fd = libc::pollfd {
            fd: reader.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: fd points to one initialized pollfd, borrowed for this call.
        let ready = unsafe { libc::poll(&mut fd, 1, 50) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == ErrorKind::Interrupted {
                continue;
            }
            return Err(PlatformInputError::BackendUnavailable);
        }
        if ready == 0 {
            continue;
        }
        match reader.read(&mut bytes[offset..]) {
            Ok(0) => {
                return if stop.load(Ordering::Acquire) {
                    Ok(false)
                } else {
                    Err(PlatformInputError::PermissionDenied)
                };
            }
            Ok(count) => offset += count,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Err(PlatformInputError::BackendUnavailable),
        }
    }
    Ok(true)
}
fn edge(control: InputControl, down: bool, at: MonotonicMillis) -> InputEvent {
    InputEvent::Edge {
        control,
        edge: if down { InputEdge::Down } else { InputEdge::Up },
        source: InputSource::Capture,
        at,
    }
}
fn mouse_button(code: u32) -> Option<MouseButton> {
    Some(match code {
        272 => MouseButton::Left,
        273 => MouseButton::Right,
        274 => MouseButton::Middle,
        275 => MouseButton::Back,
        276 => MouseButton::Forward,
        _ => return None,
    })
}
fn key_usage(code: u32) -> Option<u16> {
    Some(match code {
        1 => 0x29,
        2..=10 => (code + 28) as u16,
        11 => 0x27,
        12 => 0x2d,
        13 => 0x2e,
        14 => 0x2a,
        15 => 0x2b,
        16..=25 => {
            [0x14, 0x1a, 0x08, 0x15, 0x17, 0x1c, 0x18, 0x0c, 0x12, 0x13][(code - 16) as usize]
        }
        26 => 0x2f,
        27 => 0x30,
        28 => 0x28,
        29 => 0xe0,
        30..=38 => [0x04, 0x16, 0x07, 0x09, 0x0a, 0x0b, 0x0d, 0x0e, 0x0f][(code - 30) as usize],
        39 => 0x33,
        40 => 0x34,
        41 => 0x35,
        42 => 0xe1,
        43 => 0x31,
        44..=50 => [0x1d, 0x1b, 0x06, 0x19, 0x05, 0x11, 0x10][(code - 44) as usize],
        51 => 0x36,
        52 => 0x37,
        53 => 0x38,
        54 => 0xe5,
        55 => 0x55,
        56 => 0xe2,
        57 => 0x2c,
        58 => 0x39,
        59..=68 => (code - 59 + 0x3a) as u16,
        69 => 0x53,
        70 => 0x47,
        71..=83 => [
            0x5f, 0x60, 0x61, 0x56, 0x5c, 0x5d, 0x5e, 0x57, 0x59, 0x5a, 0x5b, 0x62, 0x63,
        ][(code - 71) as usize],
        86 => 0x64,
        87 => 0x44,
        88 => 0x45,
        96 => 0x58,
        97 => 0xe4,
        98 => 0x54,
        99 => 0x46,
        100 => 0xe6,
        102 => 0x4a,
        103 => 0x52,
        104 => 0x4b,
        105 => 0x50,
        106 => 0x4f,
        107 => 0x4d,
        108 => 0x51,
        109 => 0x4e,
        110 => 0x49,
        111 => 0x4c,
        119 => 0x48,
        125 => 0xe3,
        126 => 0xe7,
        127 => 0x65,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_only_counts_event_devices() {
        let directory = std::env::temp_dir().join(format!("bongocat-probe-{}", std::process::id()));
        fs::create_dir_all(&directory).expect("the probe test directory to be created");
        fs::write(directory.join("event0"), b"").expect("the event device stand-in to be written");
        fs::write(directory.join("mouse0"), b"").expect("the mouse stand-in to be written");
        assert!(
            any_openable_event_device(&directory),
            "an openable event device makes the unprivileged launch possible"
        );
        fs::remove_dir_all(&directory).expect("the probe test directory to be removed");
    }

    #[test]
    fn the_probe_answers_no_for_a_missing_directory() {
        assert!(!any_openable_event_device(Path::new(
            "/nonexistent-bongocat-probe-directory"
        )));
    }
}
