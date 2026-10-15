//! Real pipe protocol; the audio-device test is opt-in for hardware hosts.
use bongocat_plugin_sdk::{
    ConfigValue, HostMessage, InputActivity, PluginOutput, Request, Response, UiNode, read_frame,
    write_frame,
};
use std::{
    io::{BufReader, BufWriter},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};
struct Process {
    child: Child,
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
    sequence: u64,
}
impl Process {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_bongocat-typing-sound"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = BufWriter::new(child.stdin.take().unwrap());
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut process = Self {
            child,
            input,
            output,
            sequence: 0,
        };
        process.exchange(HostMessage::Handshake {
            protocol_version: 1,
            id: "com.bongocat.typing-sound".into(),
        });
        process
    }
    fn exchange(&mut self, message: HostMessage) -> PluginOutput {
        self.sequence += 1;
        write_frame(
            &mut self.input,
            &Request {
                sequence: self.sequence,
                message,
            },
        )
        .unwrap();
        let response: Response = read_frame(&mut self.output).unwrap();
        assert_eq!(response.sequence, self.sequence);
        let output = response.result.unwrap();
        assert!(output.validate());
        output
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[test]
fn anonymous_activity_mute_and_graceful_shutdown_over_real_pipes() {
    let mut process = Process::start();
    process.exchange(HostMessage::Start {
        config: [("volume".into(), ConfigValue::Integer(0))].into(),
        state: Vec::new(),
    });
    // Muted, so a host without an audio device still exercises the whole path:
    // the counters move, the plugin admits each edge and resolves each set.
    for (elapsed_ms, activity) in [
        (
            0,
            InputActivity {
                keyboard_down: 1,
                ..InputActivity::default()
            },
        ),
        (
            50,
            InputActivity {
                keyboard_down: 1,
                keyboard_up: 1,
                ..InputActivity::default()
            },
        ),
        (
            100,
            InputActivity {
                keyboard_down: 1,
                keyboard_up: 1,
                mouse_down: 1,
                mouse_up: 1,
            },
        ),
    ] {
        let output = process.exchange(HostMessage::InputActivity {
            elapsed_ms,
            activity,
        });
        assert!(
            output
                .settings
                .nodes
                .iter()
                .any(|node| matches!(node, UiNode::Select { key, .. } if key == "keyboard_pack")),
            "the panel publishes the sound pickers"
        );
    }
    let stopped = process.exchange(HostMessage::Stop);
    assert!(stopped.model.nodes.is_empty());
    assert!(process.child.wait().unwrap().success());
}
#[test]
#[ignore = "requires a real audio output device; run explicitly on macOS/Windows"]
fn preview_and_device_activity_use_the_real_audio_output() {
    let mut process = Process::start();
    process.exchange(HostMessage::Start {
        config: [("mouse_enabled".into(), ConfigValue::Boolean(true))].into(),
        state: Vec::new(),
    });
    let preview = process.exchange(HostMessage::Action {
        id: "preview".into(),
    });
    assert!(
        previewing(&preview),
        "audio playback did not start: {:?}",
        preview.settings
    );
    process.exchange(HostMessage::Pause);
    let paused = process.exchange(HostMessage::InputActivity {
        elapsed_ms: 500,
        activity: InputActivity {
            keyboard_down: 1,
            keyboard_up: 1,
            ..InputActivity::default()
        },
    });
    assert!(!previewing(&paused));
    process.exchange(HostMessage::Resume);
    let typed = process.exchange(HostMessage::InputActivity {
        elapsed_ms: 1000,
        activity: InputActivity {
            keyboard_down: 2,
            keyboard_up: 2,
            mouse_down: 1,
            mouse_up: 1,
        },
    });
    assert!(
        previewing(&typed),
        "typing playback did not start: {:?}",
        typed.settings
    );
    std::thread::sleep(std::time::Duration::from_millis(300));
    process.exchange(HostMessage::Stop);
    assert!(process.child.wait().unwrap().success());
}

/// Whether the plugin reports its preview control as still carrying out the
/// audition, which is what the host draws as a loading button.
fn previewing(output: &PluginOutput) -> bool {
    output.settings.nodes.iter().any(|node| {
        matches!(
            node,
            UiNode::Button {
                id,
                loading: true,
                ..
            } if id == "preview"
        )
    })
}
