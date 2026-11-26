//! Exercise the actual plugin main thread, native observer lifetime and framing.
use bongocat_plugin_sdk::{
    ConfigValue, HostMessage, Request, Response, UiNode, read_frame, write_frame,
};
use std::{
    io::{BufReader, BufWriter},
    process::{Command, Stdio},
};

#[test]
fn real_process_supports_modes_locale_errors_reset_and_clean_shutdown() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bongocat-input-source"))
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
        write_frame(&mut input, &Request { sequence, message }).unwrap();
        // A native change may publish an unsolicited update before the reply, in
        // exactly the shape the host accepts: reserved sequence zero and a valid
        // output. Every other frame keeps its request's sequence.
        let response: Response = loop {
            let frame: Response = read_frame(&mut output).unwrap();
            if frame.sequence == 0 {
                let update = frame.result.expect("an update is never a refusal");
                assert!(update.validate(), "an update is bounded");
                continue;
            }
            break frame;
        };
        assert_eq!(response.sequence, sequence);
        if let Ok(value) = &response.result {
            assert!(
                value
                    .settings
                    .nodes
                    .iter()
                    .all(|node| !matches!(node, UiNode::IconText { .. })),
                "the source preview belongs only in the model window"
            );
        }
        response.result
    };
    send(HostMessage::Handshake {
        protocol_version: 1,
        id: "com.bongocat.input-source".into(),
    })
    .unwrap();
    let started = send(HostMessage::Start {
        config: Default::default(),
        state: vec![],
    })
    .unwrap();
    assert!(started.validate());
    #[cfg(target_os = "macos")]
    assert_agent(child.id());
    assert_eq!(started.config["mode"], ConfigValue::Text("always".into()));
    assert!(started.settings.nodes.iter().any(
        |node| matches!(node, UiNode::Select { key, value, .. } if key == "mode" && value == "always")
    ));
    let started = send(HostMessage::Configure {
        config: [("mode".into(), ConfigValue::Text("switch".into()))].into(),
    })
    .unwrap();
    assert!(started.model.nodes.is_empty(), "a baseline is not a switch");
    for (locale, label) in [("zh-CN", "显示方式"), ("fr-FR", "Display mode")] {
        let value = send(HostMessage::Locale {
            locale: locale.into(),
        })
        .unwrap();
        assert!(
            value.settings.nodes.iter().any(
                |node| matches!(node, UiNode::Select { label: actual, .. } if actual == label)
            )
        );
    }
    assert!(
        send(HostMessage::Configure {
            config: [("seconds".into(), ConfigValue::Integer(100))].into()
        })
        .is_err()
    );
    let value = send(HostMessage::Tick { elapsed_ms: 2500 }).unwrap();
    assert!(value.validate());
    assert_eq!(value.config["mode"], ConfigValue::Text("switch".into()));
    let value = send(HostMessage::Reset).unwrap();
    assert_eq!(value.config["mode"], ConfigValue::Text("always".into()));
    // A headless machine may report unavailable, but a live source must carry
    // its own name and artwork rather than a host-side input-source guess.
    if let Some(node) = value.model.nodes.first() {
        assert!(
            matches!(node, UiNode::IconText { icon, text } if icon.validate() && !text.is_empty())
        );
    }
    assert!(send(HostMessage::Pause).unwrap().model.nodes.is_empty());
    assert!(send(HostMessage::Resume).unwrap().validate());
    #[cfg(target_os = "macos")]
    assert_agent(child.id());
    let stopped = send(HostMessage::Stop).unwrap();
    assert!(stopped.model.nodes.is_empty());
    assert!(
        stopped.errors.is_empty(),
        "disabling the plugin must not publish a runtime error"
    );
    assert!(child.wait().unwrap().success());
}

#[cfg(target_os = "macos")]
fn assert_agent(pid: u32) {
    use objc2::{msg_send, rc::Retained, runtime::AnyClass, runtime::AnyObject};
    // Keep AppKit linked in the test driver as well as in the child process.
    std::hint::black_box(<objc2_app_kit::NSImage as objc2::ClassType>::class());
    let class = AnyClass::get(c"NSRunningApplication").unwrap();
    // SAFETY: NSRunningApplication's documented class method accepts a pid_t
    // and returns an autoreleased object. Retained owns it during the query.
    let application: Option<Retained<AnyObject>> =
        unsafe { msg_send![class, runningApplicationWithProcessIdentifier: pid as i32] };
    let application = application.expect("plugin registered with Launch Services");
    // SAFETY: the live NSRunningApplication reports an NSInteger policy.
    let policy: isize = unsafe { msg_send![&*application, activationPolicy] };
    assert_eq!(
        policy, 1,
        "plugin must be an accessory, without a Dock icon"
    );
}
