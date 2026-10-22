//! Real pipe protocol; the audio-device test is opt-in for hardware hosts.
use bongocat_plugin_sdk::{
    ConfigValue, HostMessage, InputEventControl, PluginInputEvent, PluginOutput, Request, Response,
    read_frame, write_frame,
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
        let mut child = Command::new(env!("CARGO_BIN_EXE_bongocat-key-sounds"))
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
            id: "com.bongocat.key-sounds".into(),
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
fn locale_switches_relabel_the_whole_panel_without_changing_saved_settings() {
    let mut process = Process::start();
    let english = process.exchange(HostMessage::Start {
        config: [
            ("volume".into(), ConfigValue::Integer(0)),
            ("mouse_enabled".into(), ConfigValue::Boolean(true)),
        ]
        .into(),
        state: Vec::new(),
    });
    let chinese = process.exchange(HostMessage::Locale {
        locale: "zh-CN".into(),
    });
    assert_ne!(chinese.settings, english.settings);
    assert_eq!(chinese.config, english.config);
    assert!(!english.config.contains_key("cooldown_ms"));

    for locale in [
        "en-US", "zh-TW", "ar-SA", "vi-VN", "pt-BR", "ko-KR", "fr-FR", "unknown", "",
    ] {
        let fallback = process.exchange(HostMessage::Locale {
            locale: locale.into(),
        });
        assert_eq!(fallback.settings, english.settings, "{locale}");
        assert_eq!(fallback.config, english.config, "{locale}");
        assert_eq!(fallback.state, english.state, "{locale}");

        let localized = process.exchange(HostMessage::Locale {
            locale: "zh-CN".into(),
        });
        assert_eq!(localized.settings, chinese.settings, "{locale}");
        assert_eq!(localized.config, english.config, "{locale}");
    }
    process.exchange(HostMessage::Stop);
    assert!(process.child.wait().unwrap().success());
}

#[test]
fn ordered_edges_mute_pause_resume_and_stop_over_real_pipes() {
    let mut process = Process::start();
    process.exchange(HostMessage::Start {
        config: [("volume".into(), ConfigValue::Integer(0))].into(),
        state: Vec::new(),
    });
    for message in [
        HostMessage::InputEvents {
            events: vec![
                PluginInputEvent {
                    control: InputEventControl::Space,
                    pressed: true,
                    captured_ms: 0,
                },
                PluginInputEvent {
                    control: InputEventControl::Keyboard,
                    pressed: true,
                    captured_ms: 1,
                },
                PluginInputEvent {
                    control: InputEventControl::Space,
                    pressed: false,
                    captured_ms: 2,
                },
            ],
            overflow: 0,
        },
        HostMessage::Pause,
        HostMessage::InputEvents {
            events: vec![PluginInputEvent {
                control: InputEventControl::MouseLeft,
                pressed: true,
                captured_ms: 3,
            }],
            overflow: 1,
        },
        HostMessage::Resume,
        HostMessage::Stop,
    ] {
        assert!(process.exchange(message).validate());
    }
    assert!(process.child.wait().unwrap().success());
}
