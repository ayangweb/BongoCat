//! End-to-end: two clients, one relay, one room.
//!
//! The unit tests in each module use stub links; this one is the only place that
//! exercises a real socket between a real relay and two real clients. It exists
//! because the failure it guards against — "connected, handshook, and still
//! nothing arrives" — is invisible to every test that fakes either end.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use bongocat_input::{InputControl, InputEdge, MouseButton};
use bongocat_net::client::{ClientCommand, ClientConfig, ClientEvent};
use bongocat_net::protocol::{DeviceInfo, InputFrame};
use bongocat_net::relay::{BindMode, RelayConfig};

const PORT: u16 = 23487;
const PATIENCE: Duration = Duration::from_secs(5);

fn device(id: &str, publish: bool) -> DeviceInfo {
    DeviceInfo {
        id: id.to_owned(),
        name: id.to_owned(),
        publish,
    }
}

fn wait_for_joined(events: &Receiver<ClientEvent>) {
    let deadline = std::time::Instant::now() + PATIENCE;
    while std::time::Instant::now() < deadline {
        match events.recv_timeout(Duration::from_millis(100)) {
            Ok(ClientEvent::Joined { .. }) => return,
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => panic!("client thread died before joining"),
        }
    }
    panic!("client never joined the room");
}

fn wait_for_input(events: &Receiver<ClientEvent>) -> InputFrame {
    let deadline = std::time::Instant::now() + PATIENCE;
    while std::time::Instant::now() < deadline {
        match events.recv_timeout(Duration::from_millis(100)) {
            Ok(ClientEvent::Input(frame)) => return frame,
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => panic!("client thread died before a frame"),
        }
    }
    panic!("the frame never arrived");
}

#[test]
fn a_frame_from_one_client_reaches_the_other() {
    std::thread::spawn(move || {
        let _ = bongocat_net::relay::run(RelayConfig {
            bind: BindMode::LocalOnly,
            port: PORT,
            token: "room-under-test".to_owned(),
        });
    });
    // There is no ready signal to wait on: the relay binds inside `run`. A short
    // sleep is enough because a refused connection surfaces as `Failed`, which
    // the assertions below would report rather than hang on.
    std::thread::sleep(Duration::from_millis(300));

    let (sender_events, sender_commands) = bongocat_net::client::spawn(ClientConfig {
        url: format!("ws://127.0.0.1:{PORT}"),
        token: "room-under-test".to_owned(),
        device: device("pc", true),
    })
    .expect("connect the publishing client");

    let (watcher_events, _watcher_commands) = bongocat_net::client::spawn(ClientConfig {
        url: format!("ws://127.0.0.1:{PORT}"),
        token: "room-under-test".to_owned(),
        device: device("laptop", false),
    })
    .expect("connect the watching client");

    wait_for_joined(&sender_events);
    wait_for_joined(&watcher_events);

    sender_commands
        .send(ClientCommand::Publish(InputFrame {
            src: "pc".to_owned(),
            seq: 1,
            t: 1_762_000_000_000,
            control: InputControl::Mouse(MouseButton::Left),
            edge: InputEdge::Down,
            bounds: None,
        }))
        .expect("queue the frame");

    let frame = wait_for_input(&watcher_events);
    assert_eq!(
        frame.src, "pc",
        "the frame names the machine that produced it"
    );
    assert_eq!(
        frame.edge,
        InputEdge::Down,
        "the edge survives the round trip"
    );
    assert_eq!(
        frame.control,
        InputControl::Mouse(MouseButton::Left),
        "the control survives the round trip"
    );
}
