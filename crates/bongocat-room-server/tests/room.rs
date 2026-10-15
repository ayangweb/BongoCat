use bongocat_online_client::{OnlineOwner, OnlinePhase};
use bongocat_online_protocol::*;
use bongocat_room_server::RoomServer;
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::{
    net::TcpStream,
    sync::oneshot,
    time::{sleep, timeout},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn start() -> (
    String,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<std::io::Result<()>>,
) {
    let server = RoomServer::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let address = format!("ws://{}/ws", server.address().unwrap());
    let (stop, stopped) = oneshot::channel();
    let run = tokio::spawn(server.run(async {
        let _ = stopped.await;
    }));
    (address, stop, run)
}
async fn send(socket: &mut Socket, message: ClientMessage) {
    socket
        .send(Message::Text(
            serde_json::to_string(&message).unwrap().into(),
        ))
        .await
        .unwrap();
}
async fn receive(socket: &mut Socket) -> ServerMessage {
    timeout(Duration::from_secs(3), async {
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Text(text) => return serde_json::from_str(&text).unwrap(),
                Message::Ping(payload) => socket.send(Message::Pong(payload)).await.unwrap(),
                _ => {}
            }
        }
    })
    .await
    .expect("room message deadline")
}
async fn join(address: &str) -> (Socket, MemberId, Vec<MemberId>) {
    let (mut socket, _) = connect_async(address).await.unwrap();
    send(
        &mut socket,
        ClientMessage::Join {
            protocol_version: 1,
        },
    )
    .await;
    match receive(&mut socket).await {
        ServerMessage::Welcome { you, members, .. } => (socket, you, members),
        other => panic!("unexpected welcome: {other:?}"),
    }
}

#[tokio::test]
async fn eight_members_ninth_rejected_actions_and_leave_ordered() {
    let (address, stop, run) = start().await;
    let mut sockets = Vec::new();
    let mut ids = Vec::new();
    for count in 1..=8 {
        let (socket, id, members) = join(&address).await;
        assert_eq!(members.len(), count);
        ids.push(id);
        sockets.push(socket);
    }
    for socket in &mut sockets {
        // Drain all Joined events preceding the action.
        while let Ok(message) = timeout(Duration::from_millis(20), receive(socket)).await {
            assert!(matches!(message, ServerMessage::Joined { .. }));
        }
    }
    let (mut ninth, _) = connect_async(&address).await.unwrap();
    send(
        &mut ninth,
        ClientMessage::Join {
            protocol_version: 1,
        },
    )
    .await;
    assert_eq!(
        receive(&mut ninth).await,
        ServerMessage::Error {
            code: RoomError::RoomFull
        }
    );
    send(
        &mut sockets[0],
        ClientMessage::Action {
            action: PawAction::Both,
        },
    )
    .await;
    for socket in &mut sockets {
        assert!(
            matches!(receive(socket).await, ServerMessage::Action { member, action: PawAction::Both, .. } if member == ids[0])
        );
    }
    sockets[0].close(None).await.unwrap();
    for socket in sockets.iter_mut().skip(1) {
        assert!(
            matches!(receive(socket).await, ServerMessage::Left { member, .. } if member == ids[0])
        );
    }
    let (replacement, id, members) = join(&address).await;
    assert_eq!(members.len(), 8);
    assert!(!ids.contains(&id));
    drop(replacement);
    drop(ninth);
    drop(sockets);
    let _ = stop.send(());
    run.await.unwrap().unwrap();
}

#[tokio::test]
async fn version_raw_input_and_sender_injection_refused() {
    let (address, stop, run) = start().await;
    let (mut socket, _) = connect_async(&address).await.unwrap();
    send(
        &mut socket,
        ClientMessage::Join {
            protocol_version: 99,
        },
    )
    .await;
    assert_eq!(
        receive(&mut socket).await,
        ServerMessage::Error {
            code: RoomError::ProtocolMismatch
        }
    );
    let (mut socket, _, _) = join(&address).await;
    socket
        .send(Message::Text(
            r#"{"type":"action","action":"left","key":65,"member":123}"#.into(),
        ))
        .await
        .unwrap();
    assert_eq!(
        receive(&mut socket).await,
        ServerMessage::Error {
            code: RoomError::InvalidMessage
        }
    );
    drop(socket);
    let _ = stop.send(());
    run.await.unwrap().unwrap();
}

#[tokio::test]
async fn actual_client_activity_and_bounded_cancel() {
    let (address, stop, run) = start().await;
    let activity = ActivitySignal::default();
    let mut owner = OnlineOwner::start(activity.clone()).unwrap();
    let client = owner.client();
    client.join(address.clone()).unwrap();
    timeout(Duration::from_secs(5), async {
        while client.snapshot().phase != OnlinePhase::Connected {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let (mut observer, _, _) = join(&address).await;
    for _ in 0..1000 {
        activity.mark();
    }
    assert!(matches!(
        receive(&mut observer).await,
        ServerMessage::Action { .. }
    ));
    client.leave();
    timeout(Duration::from_secs(3), async {
        loop {
            if matches!(receive(&mut observer).await, ServerMessage::Left { .. }) {
                break;
            }
        }
    })
    .await
    .unwrap();
    activity.mark();
    assert!(!activity.take());
    assert!(owner.shutdown().is_ok());
    drop(observer);
    let _ = stop.send(());
    run.await.unwrap().unwrap();
}

#[tokio::test]
async fn server_restart_clears_members_and_client_rejoins() {
    let server = RoomServer::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let address = server.address().unwrap();
    let endpoint = format!("ws://{address}/ws");
    let (stop, stopped) = oneshot::channel();
    let run = tokio::spawn(server.run(async {
        let _ = stopped.await;
    }));
    let mut owner = OnlineOwner::start(ActivitySignal::default()).unwrap();
    let client = owner.client();
    client.join(endpoint).unwrap();
    timeout(Duration::from_secs(5), async {
        while client.snapshot().phase != OnlinePhase::Connected {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let previous = client.snapshot();
    let _ = stop.send(());
    run.await.unwrap().unwrap();
    let next = RoomServer::bind(address).await.unwrap();
    let (stop, stopped) = oneshot::channel();
    let run = tokio::spawn(next.run(async {
        let _ = stopped.await;
    }));
    timeout(Duration::from_secs(8), async {
        loop {
            let snapshot = client.snapshot();
            if snapshot.phase == OnlinePhase::Connected && snapshot.generation > previous.generation
            {
                assert_eq!(snapshot.members.len(), 1);
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    owner.shutdown().unwrap();
    let _ = stop.send(());
    run.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn heartbeat_timeout_removes_an_unresponsive_member() {
    let (address, stop, run) = start().await;
    let (mut dead, _, _) = join(&address).await;
    tokio::time::advance(HEARTBEAT_TIMEOUT + Duration::from_secs(1)).await;
    // Deliberately do not answer any further heartbeat. Closure happens only
    // after removal, so the next snapshot must contain just the new member.
    timeout(Duration::from_secs(10), async {
        while let Some(message) = dead.next().await {
            if matches!(message, Ok(Message::Close(_)) | Err(_)) {
                break;
            }
        }
    })
    .await
    .unwrap();
    let (next, _, members) = join(&address).await;
    assert_eq!(members.len(), 1);
    drop(dead);
    drop(next);
    let _ = stop.send(());
    run.await.unwrap().unwrap();
}

#[tokio::test]
async fn room_full_is_terminal_until_user_retries() {
    let (address, stop, run) = start().await;
    let mut sockets = Vec::new();
    for _ in 0..8 {
        sockets.push(join(&address).await.0);
    }
    let mut owner = OnlineOwner::start(ActivitySignal::default()).unwrap();
    let client = owner.client();
    client.join(address.clone()).unwrap();
    timeout(Duration::from_secs(5), async {
        while client.snapshot().phase != OnlinePhase::Failed {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        client.snapshot().error,
        Some(bongocat_online_client::OnlineError::Room(
            RoomError::RoomFull
        ))
    );
    let generation = client.snapshot().generation;
    sockets.pop().unwrap().close(None).await.unwrap();
    sleep(Duration::from_millis(1100)).await;
    assert_eq!(client.snapshot().phase, OnlinePhase::Failed);
    assert_eq!(client.snapshot().generation, generation);
    client.join(address).unwrap();
    timeout(Duration::from_secs(5), async {
        while client.snapshot().phase != OnlinePhase::Connected {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    owner.shutdown().unwrap();
    drop(sockets);
    let _ = stop.send(());
    run.await.unwrap().unwrap();
}
