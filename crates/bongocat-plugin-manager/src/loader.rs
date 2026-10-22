//! One owned process with bounded request/response exchanges.
use bongocat_plugin_sdk::{
    Capability, HostMessage, PROTOCOL_VERSION, PluginMetadata, PluginOutput, Request, Response,
    UiNode, read_frame, write_frame,
};
use std::{
    io,
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(crate) struct Process {
    child: Child,
    input: Option<ChildStdin>,
    responses: Receiver<io::Result<Response>>,
    reader: Option<JoinHandle<()>>,
    sequence: u64,
    icon_text: bool,
    push_output: bool,
}
impl Process {
    pub(crate) fn start(root: &Path, metadata: &PluginMetadata) -> io::Result<Self> {
        let mut command = Command::new(entry_path(root, &metadata.entry));
        command
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn()?;
        let input = child.stdin.take();
        let mut output = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("plugin stdout unavailable"))?;
        let (sender, responses) = mpsc::sync_channel(2);
        let owner = thread::current();
        let push = metadata.capabilities.contains(&Capability::PushOutputV1);
        let reader = thread::spawn(move || {
            loop {
                let response = read_frame::<Response>(&mut output);
                let failed = response.is_err();
                let update = push
                    && response
                        .as_ref()
                        .is_ok_and(|response| response.sequence == 0);
                // Unsolicited output cannot block process cleanup or grow memory.
                if sender.try_send(response).is_err() || failed {
                    break;
                }
                if update {
                    owner.unpark();
                }
            }
        });
        let mut process = Self {
            child,
            input,
            responses,
            reader: Some(reader),
            sequence: 0,
            push_output: push,
            icon_text: metadata.capabilities.contains(&Capability::IconTextV1),
        };
        process.exchange(HostMessage::Handshake {
            protocol_version: PROTOCOL_VERSION,
            id: metadata.id.clone(),
        })?;
        Ok(process)
    }
    pub(crate) fn exchange(&mut self, message: HostMessage) -> io::Result<PluginOutput> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| io::Error::other("sequence exhausted"))?;
        // Writing is also bounded: a watchdog kills a non-reading child, unblocking
        // the pipe. The watchdog is joined before returning from this request.
        let input = self
            .input
            .take()
            .ok_or_else(|| io::Error::other("plugin stopped"))?;
        let request = Request {
            sequence: self.sequence,
            message,
        };
        let (written, completion) = mpsc::sync_channel(1);
        let writer = thread::spawn(move || {
            let mut input = input;
            let result = write_frame(&mut input, &request);
            let _ = written.send((input, result));
        });
        let deadline = Instant::now() + Duration::from_millis(750);
        let completed = completion.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        if completed.is_err() {
            let _ = self.child.kill();
        }
        writer
            .join()
            .map_err(|_| io::Error::other("plugin writer failed"))?;
        let (input, result) = completed
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "plugin write deadline"))?;
        self.input = Some(input);
        result?;
        let response = loop {
            let response = self
                .responses
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::TimedOut, "plugin response deadline")
                })??;
            if response.sequence == 0 && self.push_output {
                self.validate_update(response)?;
                continue;
            }
            break response;
        };
        if response.sequence != self.sequence {
            return Err(io::Error::other("plugin response sequence mismatch"));
        }
        // A valid refusal consumed its sequence and leaves the protocol usable.
        // Transport/protocol errors, unlike invalid user configuration, are fatal.
        let output = response
            .result
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        if !output.validate() {
            return Err(io::Error::other("plugin output exceeds limits"));
        }
        if !self.icon_text
            && output
                .model
                .nodes
                .iter()
                .chain(&output.settings.nodes)
                .any(|node| matches!(node, UiNode::IconText { .. }))
        {
            return Err(io::Error::other(
                "plugin published undeclared icon_text_v1 nodes",
            ));
        }
        Ok(output)
    }
    fn validate_update(&self, response: Response) -> io::Result<PluginOutput> {
        let output = response.result.map_err(io::Error::other)?;
        if response.sequence != 0 || !output.validate() {
            return Err(io::Error::other("invalid plugin update"));
        }
        if !self.icon_text
            && output
                .model
                .nodes
                .iter()
                .chain(&output.settings.nodes)
                .any(|node| matches!(node, UiNode::IconText { .. }))
        {
            return Err(io::Error::other("undeclared icon_text_v1"));
        }
        Ok(output)
    }
    pub(crate) fn update(&mut self) -> io::Result<Option<PluginOutput>> {
        let mut latest = None;
        for _ in 0..2 {
            match self.responses.try_recv() {
                Ok(response) => latest = Some(self.validate_update(response?)?),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(io::Error::other("plugin reader stopped"));
                }
            }
        }
        Ok(latest)
    }
    pub(crate) fn stop(&mut self) -> io::Result<PluginOutput> {
        let output = self.exchange(HostMessage::Stop);
        self.input.take();
        let deadline = Instant::now() + Duration::from_millis(750);
        while self.child.try_wait()?.is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let status = self.child.try_wait()?;
        if status.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "plugin shutdown deadline",
            ));
        }
        if !status.is_some_and(|status| status.success()) {
            return Err(io::Error::other("plugin exited unsuccessfully"));
        }
        output
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// The file a manifest's `entry` names inside its package.
///
/// The manifest names the executable without a platform suffix, and a Windows
/// package always carries the `.exe` the toolchain writes, so the suffix is the
/// platform's fact rather than something a package may omit.
pub(crate) fn entry_path(root: &Path, entry: &str) -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    return root.join(format!("{entry}.exe"));
    #[cfg(not(target_os = "windows"))]
    root.join(entry)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn metadata() -> PluginMetadata {
        let mut metadata = serde_json::from_str::<crate::PluginManifest>(include_str!(
            "../../../plugins/pomodoro/plugin.json"
        ))
        .unwrap()
        .metadata;
        metadata.entry = "plugin".into();
        metadata
    }

    fn script(body: &str) -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("plugin");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        directory
    }

    fn frame(sequence: u64) -> String {
        let mut bytes = Vec::new();
        write_frame(
            &mut bytes,
            &Response {
                sequence,
                result: Ok(PluginOutput::default()),
            },
        )
        .unwrap();
        bytes.iter().map(|byte| format!("\\{byte:03o}")).collect()
    }

    #[test]
    fn push_updates_do_not_consume_request_sequences() {
        let fixture = script(&format!(
            "printf '{}'\nsleep 0.05\nprintf '{}{}'\nexec sleep 10",
            frame(1),
            frame(0),
            frame(2)
        ));
        let mut metadata = metadata();
        metadata.capabilities.push(Capability::PushOutputV1);
        let mut process = Process::start(fixture.path(), &metadata).unwrap();
        process
            .exchange(HostMessage::Tick { elapsed_ms: 1 })
            .unwrap();
        assert_eq!(process.sequence, 2);
    }

    #[test]
    fn unsolicited_updates_require_the_capability() {
        let fixture = script(&format!("printf '{}{}'\nexec sleep 10", frame(1), frame(0)));
        let mut process = Process::start(fixture.path(), &metadata()).unwrap();
        assert!(
            process
                .exchange(HostMessage::Tick { elapsed_ms: 1 })
                .is_err()
        );
    }

    #[test]
    fn inline_artwork_requires_the_manifest_capability() {
        let output = PluginOutput {
            model: bongocat_plugin_sdk::PluginView {
                nodes: vec![UiNode::IconText {
                    icon: bongocat_plugin_sdk::StatusIcon::Badge("A".into()),
                    text: "ABC".into(),
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bytes = Vec::new();
        write_frame(
            &mut bytes,
            &Response {
                sequence: 2,
                result: Ok(output),
            },
        )
        .unwrap();
        let response: String = bytes.iter().map(|byte| format!("\\{byte:03o}")).collect();
        let fixture = script(&format!("printf '{}{response}'\nexec sleep 10", frame(1)));
        for declared in [false, true] {
            let mut metadata = metadata();
            if declared {
                metadata.capabilities.push(Capability::IconTextV1);
            }
            let mut process = Process::start(fixture.path(), &metadata).unwrap();
            let result = process.exchange(HostMessage::Start {
                config: Default::default(),
                state: vec![],
            });
            assert_eq!(result.is_ok(), declared);
        }
    }

    #[test]
    fn malformed_sequences_and_nonresponsive_children_fail_closed() {
        let metadata = metadata();
        let wrong_sequence = script(&format!("printf '{}'\nexec sleep 10", frame(99)));
        assert!(Process::start(wrong_sequence.path(), &metadata).is_err());
        let unresponsive = script("exec sleep 10");
        let error = Process::start(unresponsive.path(), &metadata)
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn stop_requires_a_successful_exit_not_only_a_valid_response() {
        let metadata = metadata();
        let request_size = |sequence, message| {
            let mut bytes = Vec::new();
            write_frame(&mut bytes, &Request { sequence, message }).unwrap();
            bytes.len()
        };
        let handshake = request_size(
            1,
            HostMessage::Handshake {
                protocol_version: PROTOCOL_VERSION,
                id: metadata.id.clone(),
            },
        );
        let stop = request_size(2, HostMessage::Stop);
        let directory = script(&format!(
            "dd bs=1 count={handshake} >/dev/null 2>/dev/null\nprintf '{}'\ndd bs=1 count={stop} >/dev/null 2>/dev/null\nprintf '{}'\nexit 7",
            frame(1),
            frame(2)
        ));
        let mut process = Process::start(directory.path(), &metadata).unwrap();
        let error = process.stop().unwrap_err();
        assert_eq!(error.to_string(), "plugin exited unsuccessfully");
    }
}
