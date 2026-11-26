//! End-to-end: the real binary, over real pipes, with the real framing.
use bongocat_plugin_sdk::{
    ActionIcon, HostMessage, PROTOCOL_VERSION, Request, Response, UiNode, read_frame, write_frame,
};
use std::io::{BufReader, BufWriter};
use std::process::{Command, Stdio};

fn exchange(
    input: &mut impl std::io::Write,
    output: &mut impl std::io::Read,
    sequence: u64,
    message: HostMessage,
) -> Response {
    write_frame(input, &Request { sequence, message }).unwrap();
    read_frame::<Response>(output).unwrap()
}

#[test]
fn pomodoro_binary_completes_a_full_session() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bongocat-pomodoro"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the pomodoro binary builds with the test");
    let mut input = BufWriter::new(child.stdin.take().unwrap());
    let mut output = BufReader::new(child.stdout.take().unwrap());

    let handshake = exchange(
        &mut input,
        &mut output,
        1,
        HostMessage::Handshake {
            protocol_version: PROTOCOL_VERSION,
            id: "com.bongocat.pomodoro".into(),
        },
    );
    assert!(handshake.result.is_ok());

    let started = exchange(
        &mut input,
        &mut output,
        2,
        HostMessage::Start {
            config: [(
                "minutes".into(),
                bongocat_plugin_sdk::ConfigValue::Integer(1),
            )]
            .into(),
            state: Vec::new(),
        },
    )
    .result
    .unwrap();
    assert!(
        started.model.validate(),
        "the plugin publishes a bounded view"
    );
    assert_button(&started, "toggle", "Start", ActionIcon::Play);
    assert_button(&started, "restart", "Restart", ActionIcon::Restart);

    let toggled = exchange(
        &mut input,
        &mut output,
        3,
        HostMessage::Action {
            id: "toggle".into(),
        },
    )
    .result
    .unwrap();
    assert!(toggled.model.nodes.iter().any(|node| matches!(
        node,
        bongocat_plugin_sdk::UiNode::Button { id, .. } if id == "toggle"
    )));
    assert_button(&toggled, "toggle", "Pause", ActionIcon::Pause);

    let stopped = exchange(
        &mut input,
        &mut output,
        4,
        HostMessage::Tick { elapsed_ms: 0 },
    );
    assert!(stopped.result.unwrap().validate());

    let stopped = exchange(&mut input, &mut output, 5, HostMessage::Stop);
    assert!(stopped.result.is_ok());
    assert!(child.wait().unwrap().success());
}

fn assert_button(
    output: &bongocat_plugin_sdk::PluginOutput,
    action: &str,
    expected_label: &str,
    expected_icon: ActionIcon,
) {
    let (label, icon) = output
        .model
        .nodes
        .iter()
        .find_map(|node| match node {
            UiNode::Button {
                id, label, icon, ..
            } if id == action => Some((label, icon)),
            _ => None,
        })
        .expect("the card action must cross the process boundary");
    assert_eq!(label, expected_label);
    assert_eq!(*icon, expected_icon);
}

#[test]
fn card_tooltips_and_icons_follow_locale_and_timer_state_over_real_pipes() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bongocat-pomodoro"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = BufWriter::new(child.stdin.take().unwrap());
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut sequence = 0;
    let mut send = |message| {
        sequence += 1;
        exchange(&mut input, &mut output, sequence, message)
            .result
            .unwrap()
    };
    send(HostMessage::Handshake {
        protocol_version: PROTOCOL_VERSION,
        id: "com.bongocat.pomodoro".into(),
    });
    send(HostMessage::Start {
        config: Default::default(),
        state: Vec::new(),
    });
    for (locale, start, pause, restart) in [
        ("zh-CN", "开始", "暂停", "重新计时"),
        ("en-US", "Start", "Pause", "Restart"),
        ("zh-TW", "Start", "Pause", "Restart"),
        ("fr-FR", "Start", "Pause", "Restart"),
    ] {
        let idle = send(HostMessage::Locale {
            locale: locale.into(),
        });
        assert_button(&idle, "toggle", start, ActionIcon::Play);
        assert_button(&idle, "restart", restart, ActionIcon::Restart);
        let running = send(HostMessage::Action {
            id: "toggle".into(),
        });
        assert_button(&running, "toggle", pause, ActionIcon::Pause);
        let restarted = send(HostMessage::Action {
            id: "restart".into(),
        });
        assert_button(&restarted, "toggle", pause, ActionIcon::Pause);
        assert_button(&restarted, "restart", restart, ActionIcon::Restart);
        let paused = send(HostMessage::Action {
            id: "toggle".into(),
        });
        assert_button(&paused, "toggle", start, ActionIcon::Play);
    }
    send(HostMessage::Stop);
    assert!(child.wait().unwrap().success());
}
