#![forbid(unsafe_code)]

use std::fmt;

/// Platform overlay lifecycle states. Platform handles are owned outside this
/// contract and must not outlive `Closed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayState {
    New,
    Visible,
    Hidden,
    Closing,
    Closed,
}

/// Required shutdown order for the application runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownStage {
    InputStopped,
    RuntimeStopped,
    ConfigFlushed,
    FrameSourceStopped,
    RendererReleased,
    OverlayDestroyed,
    GpuiClosed,
}

impl ShutdownStage {
    const ORDER: [Self; 7] = [
        Self::InputStopped,
        Self::RuntimeStopped,
        Self::ConfigFlushed,
        Self::FrameSourceStopped,
        Self::RendererReleased,
        Self::OverlayDestroyed,
        Self::GpuiClosed,
    ];

    fn next_after(completed: &[Self]) -> Option<Self> {
        Self::ORDER
            .iter()
            .copied()
            .find(|stage| !completed.contains(stage))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecycleError {
    InvalidTransition {
        state: OverlayState,
        operation: &'static str,
    },
    ShutdownAlreadyStarted,
    ShutdownStageOutOfOrder {
        expected: ShutdownStage,
        received: ShutdownStage,
    },
    ShutdownNotStarted,
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition { state, operation } => {
                write!(f, "cannot {operation} while overlay is {state:?}")
            }
            Self::ShutdownAlreadyStarted => f.write_str("shutdown has already started"),
            Self::ShutdownStageOutOfOrder { expected, received } => {
                write!(f, "shutdown expected {expected:?}, received {received:?}")
            }
            Self::ShutdownNotStarted => f.write_str("shutdown has not started"),
        }
    }
}

impl std::error::Error for LifecycleError {}

#[derive(Debug, Default)]
pub struct OverlayLifecycle {
    state: Option<OverlayState>,
    completed_shutdown: Vec<ShutdownStage>,
}

impl OverlayLifecycle {
    pub fn new() -> Self {
        Self {
            state: Some(OverlayState::New),
            completed_shutdown: Vec::new(),
        }
    }

    pub fn state(&self) -> OverlayState {
        self.state.unwrap_or(OverlayState::Closed)
    }

    pub fn completed_shutdown(&self) -> &[ShutdownStage] {
        &self.completed_shutdown
    }

    pub fn show(&mut self) -> Result<(), LifecycleError> {
        match self.state() {
            OverlayState::New | OverlayState::Hidden => {
                self.state = Some(OverlayState::Visible);
                Ok(())
            }
            state => Err(LifecycleError::InvalidTransition {
                state,
                operation: "show",
            }),
        }
    }

    pub fn hide(&mut self) -> Result<(), LifecycleError> {
        match self.state() {
            OverlayState::Visible => {
                self.state = Some(OverlayState::Hidden);
                Ok(())
            }
            state => Err(LifecycleError::InvalidTransition {
                state,
                operation: "hide",
            }),
        }
    }

    pub fn begin_shutdown(&mut self) -> Result<(), LifecycleError> {
        match self.state() {
            OverlayState::Visible | OverlayState::Hidden => {
                self.state = Some(OverlayState::Closing);
                Ok(())
            }
            OverlayState::Closing | OverlayState::Closed => {
                Err(LifecycleError::ShutdownAlreadyStarted)
            }
            OverlayState::New => Err(LifecycleError::InvalidTransition {
                state: OverlayState::New,
                operation: "begin shutdown",
            }),
        }
    }

    pub fn complete_shutdown(&mut self, stage: ShutdownStage) -> Result<(), LifecycleError> {
        if self.state() == OverlayState::New {
            return Err(LifecycleError::InvalidTransition {
                state: OverlayState::New,
                operation: "complete shutdown",
            });
        }
        let Some(expected) = ShutdownStage::next_after(&self.completed_shutdown) else {
            return Err(LifecycleError::ShutdownAlreadyStarted);
        };
        if !self.completed_shutdown.is_empty() || self.state() == OverlayState::Closing {
            if stage != expected {
                return Err(LifecycleError::ShutdownStageOutOfOrder {
                    expected,
                    received: stage,
                });
            }
        } else {
            return Err(LifecycleError::ShutdownNotStarted);
        }
        self.completed_shutdown.push(stage);
        if stage == ShutdownStage::OverlayDestroyed {
            self.state = Some(OverlayState::Closed);
        }
        Ok(())
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.completed_shutdown.len() == ShutdownStage::ORDER.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shutdown(lifecycle: &mut OverlayLifecycle) {
        lifecycle.begin_shutdown().unwrap();
        for stage in ShutdownStage::ORDER {
            lifecycle.complete_shutdown(stage).unwrap();
        }
    }

    #[test]
    fn allows_show_hide_and_reopen_before_shutdown() {
        let mut lifecycle = OverlayLifecycle::new();
        lifecycle.show().unwrap();
        lifecycle.hide().unwrap();
        lifecycle.show().unwrap();
        assert_eq!(lifecycle.state(), OverlayState::Visible);
    }

    #[test]
    fn enforces_shutdown_order_and_closes_after_overlay_destroy() {
        let mut lifecycle = OverlayLifecycle::new();
        lifecycle.show().unwrap();
        lifecycle.begin_shutdown().unwrap();
        let error = lifecycle
            .complete_shutdown(ShutdownStage::RendererReleased)
            .unwrap_err();
        assert_eq!(
            error,
            LifecycleError::ShutdownStageOutOfOrder {
                expected: ShutdownStage::InputStopped,
                received: ShutdownStage::RendererReleased,
            }
        );
        for stage in ShutdownStage::ORDER {
            lifecycle.complete_shutdown(stage).unwrap();
            if stage == ShutdownStage::OverlayDestroyed {
                assert_eq!(lifecycle.state(), OverlayState::Closed);
            }
        }
        assert!(lifecycle.is_shutdown_complete());
    }

    #[test]
    fn repeated_create_destroy_cycles_do_not_retain_state() {
        for _ in 0..100 {
            let mut lifecycle = OverlayLifecycle::new();
            lifecycle.show().unwrap();
            shutdown(&mut lifecycle);
            assert_eq!(lifecycle.state(), OverlayState::Closed);
            assert!(lifecycle.is_shutdown_complete());
        }
    }

    #[test]
    fn cannot_reopen_after_shutdown_begins() {
        let mut lifecycle = OverlayLifecycle::new();
        lifecycle.show().unwrap();
        lifecycle.begin_shutdown().unwrap();
        assert!(matches!(
            lifecycle.show(),
            Err(LifecycleError::InvalidTransition {
                state: OverlayState::Closing,
                operation: "show"
            })
        ));
    }
}
