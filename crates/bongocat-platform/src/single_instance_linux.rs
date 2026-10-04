use crate::{SingleInstanceAction, SingleInstanceEnvironment, SingleInstanceError};
use std::{
    env, fs, io,
    os::unix::{fs::PermissionsExt, net::UnixDatagram},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const WAKE_MESSAGE: &[u8] = b"open-settings";
const RECEIVE_INTERVAL: Duration = Duration::from_millis(100);

pub enum SingleInstanceStart {
    Primary(SingleInstance),
    SecondaryNotified,
}

pub struct SingleInstance {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    receiver: Receiver<SingleInstanceAction>,
    worker: Option<JoinHandle<io::Result<()>>>,
}

impl SingleInstanceEnvironment {
    fn endpoint_name(self) -> &'static str {
        match self {
            Self::Development => "bongocat-development.sock",
            Self::Production => "bongocat-production.sock",
        }
    }
}

impl SingleInstance {
    pub fn acquire(
        environment: SingleInstanceEnvironment,
    ) -> Result<SingleInstanceStart, SingleInstanceError> {
        let path = endpoint_path(environment)?;
        match bind_primary(&path) {
            Ok(socket) => Self::start_primary(path, socket).map(SingleInstanceStart::Primary),
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
                if notify_primary(&path).is_ok() {
                    return Ok(SingleInstanceStart::SecondaryNotified);
                }
                // Unix socket paths survive an unclean process exit. A live
                // primary accepted the datagram above; only an unreachable
                // endpoint is removed and claimed again.
                fs::remove_file(&path).map_err(|_| SingleInstanceError::PrimaryUnavailable)?;
                let socket =
                    bind_primary(&path).map_err(|_| SingleInstanceError::EndpointCreateFailed)?;
                Self::start_primary(path, socket).map(SingleInstanceStart::Primary)
            }
            Err(_) => Err(SingleInstanceError::EndpointCreateFailed),
        }
    }

    fn start_primary(path: PathBuf, socket: UnixDatagram) -> Result<Self, SingleInstanceError> {
        socket
            .set_read_timeout(Some(RECEIVE_INTERVAL))
            .map_err(|_| SingleInstanceError::EndpointCreateFailed)?;
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("bongocat-single-instance".into())
            .spawn(move || {
                let mut message = [0_u8; 32];
                while !worker_stop.load(Ordering::Acquire) {
                    match socket.recv(&mut message) {
                        Ok(length) if &message[..length] == WAKE_MESSAGE => {
                            let _ = sender.send(SingleInstanceAction::OpenSettings);
                        }
                        Ok(_) => {}
                        Err(error)
                            if matches!(
                                error.kind(),
                                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                            ) => {}
                        Err(error) => return Err(error),
                    }
                }
                Ok(())
            })
            .map_err(|_| SingleInstanceError::WorkerCreateFailed)?;
        Ok(Self {
            path,
            stop,
            receiver,
            worker: Some(worker),
        })
    }

    pub fn try_recv(&self) -> Option<SingleInstanceAction> {
        self.receiver.try_recv().ok()
    }

    pub fn shutdown(mut self) -> Result<(), SingleInstanceError> {
        self.cleanup()
    }

    fn cleanup(&mut self) -> Result<(), SingleInstanceError> {
        self.stop.store(true, Ordering::Release);
        let _ = notify_primary(&self.path);
        let worker_clean = self
            .worker
            .take()
            .is_none_or(|worker| worker.join().is_ok_and(|result| result.is_ok()));
        let endpoint_clean = match fs::remove_file(&self.path) {
            Ok(()) => true,
            Err(error) => error.kind() == io::ErrorKind::NotFound,
        };
        if worker_clean && endpoint_clean {
            Ok(())
        } else {
            Err(SingleInstanceError::ShutdownFailed)
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn endpoint_path(environment: SingleInstanceEnvironment) -> Result<PathBuf, SingleInstanceError> {
    let directory = env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or(SingleInstanceError::RuntimeDirectoryUnavailable)?;
    directory
        .is_dir()
        .then(|| directory.join(environment.endpoint_name()))
        .ok_or(SingleInstanceError::RuntimeDirectoryUnavailable)
}

fn bind_primary(path: &Path) -> io::Result<UnixDatagram> {
    let socket = UnixDatagram::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(socket)
}

fn notify_primary(path: &Path) -> io::Result<()> {
    let socket = UnixDatagram::unbound()?;
    socket.connect(path)?;
    socket.send(WAKE_MESSAGE).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn a_secondary_datagram_wakes_the_primary_and_shutdown_removes_the_endpoint() {
        let directory = tempfile::tempdir().expect("temporary runtime directory");
        let path = directory.path().join("single-instance.sock");
        let socket = bind_primary(&path).expect("bind primary endpoint");
        let primary = SingleInstance::start_primary(path.clone(), socket).expect("start primary");

        notify_primary(&path).expect("notify primary");
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if primary.try_recv() == Some(SingleInstanceAction::OpenSettings) {
                break;
            }
            assert!(Instant::now() < deadline, "primary did not receive wake");
            thread::sleep(Duration::from_millis(5));
        }

        primary.shutdown().expect("clean shutdown");
        assert!(!path.exists());
    }
}
