//! A service-neutral UI seam. Network libraries stay outside this crate.
pub use bongocat_online_protocol::{
    OnlineError, OnlinePhase, OnlineSnapshot, RoomError, valid_server_address,
};
use std::sync::Arc;

pub trait OnlineSettingsCapability: Send + Sync + 'static {
    fn snapshot(&self) -> OnlineSnapshot;
    fn join(&self, address: String) -> Result<(), OnlineError>;
    fn leave(&self);
    fn retry_rendering(&self);
}
#[derive(Clone)]
pub struct OnlineSettingsHandle(Arc<dyn OnlineSettingsCapability>);
impl OnlineSettingsHandle {
    pub fn new(service: impl OnlineSettingsCapability) -> Self {
        Self(Arc::new(service))
    }
    pub fn snapshot(&self) -> OnlineSnapshot {
        self.0.snapshot()
    }
    pub fn join(&self, address: String) -> Result<(), OnlineError> {
        self.0.join(address)
    }
    pub fn leave(&self) {
        self.0.leave();
    }
    pub fn retry_rendering(&self) {
        self.0.retry_rendering();
    }
}
