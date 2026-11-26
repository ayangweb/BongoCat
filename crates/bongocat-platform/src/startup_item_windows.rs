use crate::{StartupItemEnvironment, StartupItemError};
use windows::{
    Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
            TaskScheduler::{
                ITaskFolder, ITaskService, TASK_CREATE_OR_UPDATE, TASK_LOGON_INTERACTIVE_TOKEN,
                TaskScheduler,
            },
            Variant::VARIANT,
        },
    },
    core::{BSTR, HRESULT},
};

struct Apartment(bool);

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances this thread's successful CoInitializeEx call after all COM objects drop.
            unsafe { CoUninitialize() };
        }
    }
}

struct Backend {
    folder: ITaskFolder,
    name: BSTR,
    user: String,
    _apartment: Apartment,
}

impl Backend {
    fn new(environment: StartupItemEnvironment) -> windows::core::Result<Self> {
        // SAFETY: COM stays on the calling thread; the guard balances only our own successful
        // initialization. An existing STA is usable without changing or uninitializing it.
        unsafe {
            let initialized = CoInitializeEx(None, COINIT_MULTITHREADED);
            if initialized != RPC_E_CHANGED_MODE {
                initialized.ok()?;
            }
            let apartment = Apartment(initialized.is_ok());
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
            let empty = VARIANT::default();
            service.Connect(&empty, &empty, &empty, &empty)?;
            let user = format!(
                "{}\\{}",
                service.ConnectedDomain()?,
                service.ConnectedUser()?
            );
            let product = match environment {
                StartupItemEnvironment::Development => "BongoCat Development",
                StartupItemEnvironment::Production => "BongoCat Production",
            };
            // Account-specific names prevent one user's registration replacing another's task.
            let name = BSTR::from(format!("{product} {}", user.replace('\\', "@")));
            let folder = service.GetFolder(&BSTR::from("\\"))?;
            drop(service);
            Ok(Self {
                folder,
                name,
                user,
                _apartment: apartment,
            })
        }
    }
}

fn missing(error: &windows::core::Error) -> bool {
    error.code() == HRESULT::from_win32(2)
}

pub(super) fn state(environment: StartupItemEnvironment) -> Result<Option<bool>, StartupItemError> {
    let backend = Backend::new(environment).map_err(|_| StartupItemError::BackendUnavailable)?;
    // SAFETY: the live folder interface is used and released on this initialized COM thread.
    unsafe {
        match backend.folder.GetTask(&backend.name) {
            Ok(task) => task
                .Enabled()
                .map(|value| Some(value.as_bool()))
                .map_err(|_| StartupItemError::StateReadFailed),
            Err(error) if missing(&error) => Ok(None),
            Err(_) => Err(StartupItemError::StateReadFailed),
        }
    }
}

pub(super) fn set_enabled(
    environment: StartupItemEnvironment,
    enabled: bool,
) -> Result<(), StartupItemError> {
    let backend = Backend::new(environment).map_err(|_| StartupItemError::BackendUnavailable)?;
    // SAFETY: all BSTR/VARIANT inputs live through their synchronous COM calls. Interfaces never
    // leave this thread. InteractiveToken uses the current user, never a password or SYSTEM.
    unsafe {
        if !enabled {
            return match backend.folder.DeleteTask(&backend.name, 0) {
                Ok(()) => Ok(()),
                Err(error) if missing(&error) => Ok(()),
                Err(_) => Err(StartupItemError::DisableFailed),
            };
        }
        let executable =
            std::env::current_exe().map_err(|_| StartupItemError::CurrentExecutableUnavailable)?;
        let executable = executable
            .to_str()
            .filter(|_| executable.is_absolute())
            .ok_or(StartupItemError::InvalidExecutablePath)?;
        let xml = super::startup_task::definition(
            &backend.user,
            executable,
            crate::startup_permission_available(),
        );
        let empty = VARIANT::default();
        backend
            .folder
            .RegisterTask(
                &backend.name,
                &BSTR::from(xml),
                TASK_CREATE_OR_UPDATE.0,
                &empty,
                &empty,
                TASK_LOGON_INTERACTIVE_TOKEN,
                &empty,
            )
            .map_err(|_| StartupItemError::EnableFailed)?;
        Ok(())
    }
}
