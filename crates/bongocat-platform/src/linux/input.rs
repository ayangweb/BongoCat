use crate::PlatformInputError;
use bongocat_input::*;
use bongocat_input_helper::protocol::{ButtonState, HEADER, InputMessage, PACKET_SIZE};
use std::{
    io::{ErrorKind, Read},
    os::fd::AsRawFd,
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
    shortcuts:
        Arc<std::sync::Mutex<Option<(bongocat_config::ShortcutTable, crate::ShortcutDispatcher)>>>,
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
        let mut child = Command::new("/usr/bin/pkexec")
            .arg("--disable-internal-agent")
            .arg(executable)
            .arg("--input-helper")
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
        let shortcuts = Arc::new(std::sync::Mutex::new(
            None::<(bongocat_config::ShortcutTable, crate::ShortcutDispatcher)>,
        ));
        let worker_shortcuts = shortcuts.clone();
        let worker = thread::Builder::new()
            .name("bongocat-linux-input".into())
            .spawn(move || {
                let mut state = PlatformInputDiagnostics {
                    service_start_attempts: 1,
                    ..Default::default()
                };
                let started = Instant::now();
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
                    let mut pressed = std::collections::BTreeSet::new();
                    let mut enabled = false;
                    let mut position = CursorPosition { x: 0.5, y: 0.5 };
                    while read_packet(&mut output, &mut packet, &worker_stop)? {
                        let at = MonotonicMillis::new(started.elapsed().as_millis() as u64);
                        let message = InputMessage::decode(&packet)
                            .ok_or(PlatformInputError::BackendUnavailable)?;
                        if let InputMessage::Reset { enabled: active } = message {
                            pressed.clear();
                            enabled = active;
                        }
                        if let InputMessage::Key {
                            code,
                            state: key_state,
                        } = message
                            && let Some(usage) = key_usage(code)
                        {
                            let fresh = if key_state == ButtonState::Pressed {
                                pressed.insert(usage)
                            } else {
                                pressed.remove(&usage);
                                false
                            };
                            if fresh {
                                let mut bits = 0;
                                for (keys, bit) in [
                                    ([0xe0, 0xe4], 1),
                                    ([0xe2, 0xe6], 2),
                                    ([0xe1, 0xe5], 4),
                                    ([0xe3, 0xe7], 8),
                                ] {
                                    if keys.iter().any(|k| pressed.contains(k)) {
                                        bits |= bit;
                                    }
                                }
                                if let Some((table, dispatcher)) =
                                    &*worker_shortcuts.lock().unwrap()
                                    && let Some(binding) = table.load().resolve_hid_usage(
                                        bongocat_config::ShortcutModifiers::from_bits(bits)
                                            .unwrap(),
                                        usage,
                                    )
                                {
                                    let _ = dispatcher.execute(binding.target());
                                }
                            }
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
                    match producer.recover(
                        InputResetReason::ServiceRestart,
                        MonotonicMillis::new(started.elapsed().as_millis() as u64),
                    ) {
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
            shortcuts,
            worker: Some(worker),
        })
    }
    pub fn set_shortcuts(
        &self,
        table: bongocat_config::ShortcutTable,
        dispatcher: crate::ShortcutDispatcher,
    ) {
        *self.shortcuts.lock().unwrap() = Some((table, dispatcher));
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
