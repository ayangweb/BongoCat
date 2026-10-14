//! End-to-end: the real binary, over real pipes, with the real framing.
use bongocat_plugin_sdk::{
    HostMessage, PROTOCOL_VERSION, Request, Response, read_frame, write_frame,
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
