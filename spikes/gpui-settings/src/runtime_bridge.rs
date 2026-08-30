use async_channel::{Receiver, Sender};
use std::{fmt, future::Future, time::Duration};

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
    ReadSnapshot {
        reply: Sender<Result<RuntimeSnapshot, BridgeError>>,
    },
    Shutdown {
        acknowledged: Sender<()>,
    },
}

#[derive(Clone)]
pub struct RuntimeBridge {
    commands: Sender<RuntimeCommand>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeError {
    RuntimeStopped,
    ProbeFailure,
}

impl fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeStopped => formatter.write_str("runtime bridge is stopped"),
            Self::ProbeFailure => formatter.write_str("runtime probe failed"),
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
            .map_err(|_| BridgeError::RuntimeStopped)?
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RuntimeProbeMode {
    #[default]
    Normal,
    DelayedErrorRecovery,
}

impl RuntimeProbeMode {
    fn response_delay(self) -> Duration {
        match self {
            Self::Normal => Duration::ZERO,
            Self::DelayedErrorRecovery => Duration::from_millis(1_500),
        }
    }

    fn fails_read(self, read_number: u64) -> bool {
        self == Self::DelayedErrorRecovery && read_number == 2
    }
}

pub async fn run_runtime<Delay, DelayFuture>(
    receiver: RuntimeCommandReceiver,
    probe_mode: RuntimeProbeMode,
    mut delay: Delay,
) where
    Delay: FnMut(Duration) -> DelayFuture,
    DelayFuture: Future<Output = ()>,
{
    let mut revision = 0;
    let mut read_number = 0;
    while let Ok(command) = receiver.0.recv().await {
        match command {
            RuntimeCommand::ReadSnapshot { reply } => {
                read_number += 1;
                delay(probe_mode.response_delay()).await;
                let result = if probe_mode.fails_read(read_number) {
                    Err(BridgeError::ProbeFailure)
                } else {
                    revision += 1;
                    Ok(RuntimeSnapshot {
                        revision,
                        health: RuntimeHealth::Ready,
                    })
                };
                let _ = reply.send(result).await;
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
        let worker = std::thread::spawn(move || {
            futures_lite::future::block_on(run_runtime(commands, RuntimeProbeMode::Normal, |_| {
                std::future::ready(())
            }))
        });

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

    #[test]
    fn delayed_probe_fails_only_the_second_read() {
        assert_eq!(
            RuntimeProbeMode::DelayedErrorRecovery.response_delay(),
            Duration::from_millis(1_500)
        );
        assert!(!RuntimeProbeMode::DelayedErrorRecovery.fails_read(1));
        assert!(RuntimeProbeMode::DelayedErrorRecovery.fails_read(2));
        assert!(!RuntimeProbeMode::DelayedErrorRecovery.fails_read(3));
        assert!(!RuntimeProbeMode::Normal.fails_read(2));
    }
}
