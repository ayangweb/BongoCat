use async_channel::{Receiver, Sender};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeHealth {
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub revision: u64,
    pub health: RuntimeHealth,
}

enum RuntimeCommand {
    ReadSnapshot { reply: Sender<RuntimeSnapshot> },
    Shutdown { acknowledged: Sender<()> },
}

#[derive(Clone)]
pub struct RuntimeBridge {
    commands: Sender<RuntimeCommand>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeError {
    RuntimeStopped,
}

impl fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeStopped => formatter.write_str("runtime bridge is stopped"),
        }
    }
}

impl std::error::Error for BridgeError {}

impl RuntimeBridge {
    pub fn new() -> (Self, RuntimeCommandReceiver) {
        let (commands, receiver) = async_channel::bounded(8);
        (Self { commands }, RuntimeCommandReceiver(receiver))
    }

    pub async fn read_snapshot(&self) -> Result<RuntimeSnapshot, BridgeError> {
        let (reply, response) = async_channel::bounded(1);
        self.commands
            .send(RuntimeCommand::ReadSnapshot { reply })
            .await
            .map_err(|_| BridgeError::RuntimeStopped)?;
        response
            .recv()
            .await
            .map_err(|_| BridgeError::RuntimeStopped)
    }

    pub async fn shutdown(&self) -> Result<(), BridgeError> {
        let (acknowledged, response) = async_channel::bounded(1);
        self.commands
            .send(RuntimeCommand::Shutdown { acknowledged })
            .await
            .map_err(|_| BridgeError::RuntimeStopped)?;
        response
            .recv()
            .await
            .map_err(|_| BridgeError::RuntimeStopped)
    }
}

pub struct RuntimeCommandReceiver(Receiver<RuntimeCommand>);

pub async fn run_runtime(receiver: RuntimeCommandReceiver) {
    let mut revision = 0;
    while let Ok(command) = receiver.0.recv().await {
        match command {
            RuntimeCommand::ReadSnapshot { reply } => {
                revision += 1;
                let _ = reply
                    .send(RuntimeSnapshot {
                        revision,
                        health: RuntimeHealth::Ready,
                    })
                    .await;
            }
            RuntimeCommand::Shutdown { acknowledged } => {
                receiver.0.close();
                let _ = acknowledged.send(()).await;
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_delivers_snapshots_and_closes_the_bridge() {
        let (bridge, commands) = RuntimeBridge::new();
        let worker = std::thread::spawn(|| futures_lite::future::block_on(run_runtime(commands)));

        futures_lite::future::block_on(async {
            let first = bridge.read_snapshot().await.unwrap();
            let second = bridge.read_snapshot().await.unwrap();

            assert_eq!(first.revision, 1);
            assert_eq!(second.revision, 2);
            assert_eq!(first.health, RuntimeHealth::Ready);
            assert_eq!(second.health, RuntimeHealth::Ready);

            bridge.shutdown().await.unwrap();
            assert_eq!(
                bridge.read_snapshot().await,
                Err(BridgeError::RuntimeStopped)
            );
        });

        worker.join().unwrap();
    }
}
