//! Root-owned lifecycle for the dedicated v4l2loopback device.
//!
//! Only the fixed operations below are accepted by the application helper.
//! The card label is not an ownership proof: a root-owned, environment-specific
//! record and the current boot identifier are required before deleting a node.

use bongocat_config::BuildEnvironment;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use v4l::Device;

use super::alpha_output::alpha_camera_name;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AlphaAdminError {
    #[error("administrator authorization was denied")]
    AuthorizationDenied,
    #[error("the v4l2loopback driver is not installed")]
    DriverMissing,
    #[error("a required system tool or service is unavailable")]
    UnsupportedSystem,
    #[error("the camera or its management files are in use")]
    Busy,
    #[error("camera ownership cannot be verified")]
    OwnershipConflict,
    #[error("camera node permission was not applied")]
    PermissionDenied,
    #[error("camera management failed")]
    OperationFailed,
    #[error("installation failed and rollback requires attention")]
    RollbackFailed,
}

impl AlphaAdminError {
    pub const fn exit_code(self) -> i32 {
        match self {
            Self::AuthorizationDenied => 21,
            Self::DriverMissing => 22,
            Self::UnsupportedSystem => 23,
            Self::Busy => 24,
            Self::OwnershipConflict => 25,
            Self::OperationFailed => 26,
            Self::PermissionDenied => 27,
            Self::RollbackFailed => 28,
        }
    }
}

/// Observed system state. This is a read-only check and does not authorize
/// administrative changes or establish ownership for deletion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AlphaSystemStatus {
    pub driver_installed: Option<bool>,
    pub module_loaded: bool,
    pub management_available: bool,
    pub management_unavailable: Option<AlphaManagementUnavailable>,
    pub camera: AlphaInstallationStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlphaManagementUnavailable {
    Authorization,
    LoopbackControl,
    Udev,
    ModuleTools,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlphaInstallationStatus {
    NotInstalled,
    Installed,
    RecoveryRequired,
    Conflict,
    Unknown,
}

/// Probe system facilities on a worker thread; it invokes fixed system tools.
pub fn inspect_alpha_system(environment: BuildEnvironment) -> AlphaSystemStatus {
    let module_loaded = Path::new("/sys/module/v4l2loopback").is_dir();
    let driver_installed = if module_loaded {
        Some(true)
    } else {
        Command::new("/usr/bin/modinfo")
            .args(["-F", "filename", "v4l2loopback"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()
            .map(|status| status.success())
    };
    let management_unavailable = if !Path::new("/usr/bin/pkexec").is_file() {
        Some(AlphaManagementUnavailable::Authorization)
    } else if !Path::new("/usr/bin/v4l2loopback-ctl").is_file() {
        Some(AlphaManagementUnavailable::LoopbackControl)
    } else if !Path::new("/usr/bin/udevadm").is_file() {
        Some(AlphaManagementUnavailable::Udev)
    } else if !Path::new("/usr/bin/modprobe").is_file() || !Path::new("/usr/bin/modinfo").is_file()
    {
        Some(AlphaManagementUnavailable::ModuleTools)
    } else {
        None
    };
    let management_available = management_unavailable.is_none();
    let paths = Paths::system(environment);
    let uid = fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("Uid:"))
                .and_then(|line| line.split_whitespace().next())
                .and_then(|value| value.parse::<u32>().ok())
                .filter(|uid| *uid != 0)
        });
    let camera = match uid {
        Some(uid) => inspect_installation(&paths, uid),
        None => AlphaInstallationStatus::Unknown,
    };
    AlphaSystemStatus {
        driver_installed,
        module_loaded,
        management_available,
        management_unavailable,
        camera,
    }
}

fn inspect_installation(paths: &Paths, uid: u32) -> AlphaInstallationStatus {
    use AlphaInstallationStatus::*;
    let directory = match fs::symlink_metadata(&paths.state_dir) {
        Ok(metadata)
            if metadata.is_dir() && metadata.uid() == 0 && metadata.mode() & 0o066 == 0 =>
        {
            true
        }
        Ok(_) => return Conflict,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(_) => return Unknown,
    };
    let record = match trusted_file(&paths.state()) {
        Ok(value) => value,
        Err(AlphaAdminError::OwnershipConflict) => return Conflict,
        Err(_) if !directory => false,
        Err(_) => return Unknown,
    };
    let rule = match trusted_file(&paths.rule) {
        Ok(value) => value,
        Err(AlphaAdminError::OwnershipConflict) => return Conflict,
        Err(_) => return Unknown,
    };
    if !record && !rule {
        return match super::alpha_output::has_named_camera(alpha_camera_name(paths.environment)) {
            Ok(false) => NotInstalled,
            Ok(true) => Conflict,
            Err(_) => Unknown,
        };
    }
    if !directory || !record || !rule {
        return RecoveryRequired;
    }
    let state = match authorized_state(paths, uid) {
        Ok(state) => state,
        Err(AlphaAdminError::OwnershipConflict) => return Conflict,
        Err(_) => return RecoveryRequired,
    };
    match node_matches(paths, state.device) {
        Ok(true) => Installed,
        Ok(false) => RecoveryRequired,
        Err(AlphaAdminError::OwnershipConflict) => Conflict,
        Err(_) => RecoveryRequired,
    }
}

/// Check the root-owned record and rule before exposing a device to the
/// unprivileged output path. Legacy private records remain readable by the
/// privileged installer; reinstalling publishes the same non-secret identity.
fn authorized_state(paths: &Paths, uid: u32) -> Result<State, AlphaAdminError> {
    for directory in [
        Path::new("/var"),
        Path::new("/var/lib"),
        &paths.state_dir,
        Path::new("/etc"),
        Path::new("/etc/udev"),
        Path::new("/etc/udev/rules.d"),
    ] {
        let metadata =
            fs::symlink_metadata(directory).map_err(|_| AlphaAdminError::OperationFailed)?;
        if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(AlphaAdminError::OwnershipConflict);
        }
    }
    let state = read_state(paths)?.ok_or(AlphaAdminError::OperationFailed)?;
    if state.owner_uid != uid {
        return Err(AlphaAdminError::OwnershipConflict);
    }
    if !state.matches_session(uid, &boot_id()?) {
        return Err(AlphaAdminError::OperationFailed);
    }
    if Some(device_inode(state.device)?) != state.node_inode {
        return Err(AlphaAdminError::OwnershipConflict);
    }
    if !trusted_file(&paths.rule)?
        || fs::read(&paths.rule).map_err(|_| AlphaAdminError::OperationFailed)?
            != paths.rule_contents(uid, state.device).as_bytes()
    {
        return Err(AlphaAdminError::OwnershipConflict);
    }
    Ok(state)
}

pub(super) fn authorized_camera_node(
    environment: BuildEnvironment,
) -> Result<(PathBuf, u64), super::alpha_output::AlphaCameraError> {
    use super::alpha_output::AlphaCameraError;
    let uid = real_uid(Path::new("/proc/self/status"))
        .filter(|uid| *uid != 0)
        .ok_or(AlphaCameraError::PermissionDenied)?;
    let state =
        authorized_state(&Paths::system(environment), uid).map_err(|error| match error {
            AlphaAdminError::OwnershipConflict => AlphaCameraError::InvalidDevice,
            AlphaAdminError::PermissionDenied => AlphaCameraError::PermissionDenied,
            _ => AlphaCameraError::Unavailable,
        })?;
    let inode = state.node_inode.ok_or(AlphaCameraError::Unavailable)?;
    Ok((node_path(state.device), inode))
}

/// Only these fixed operations can be requested from the unprivileged process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlphaAdminAction {
    Install,
    Uninstall,
}

impl AlphaAdminAction {
    const fn argument(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Uninstall => "uninstall",
        }
    }
}

/// Ask the system authorization agent to run a fixed management operation.
/// Call this on a worker, and stop/join the output before requesting uninstall.
pub fn manage_alpha_camera(
    _environment: BuildEnvironment,
    action: AlphaAdminAction,
    cancel: &AtomicBool,
) -> Result<(), AlphaAdminError> {
    if cancel.load(Ordering::Acquire) {
        return Err(AlphaAdminError::OperationFailed);
    }
    let executable = std::env::current_exe().map_err(|_| AlphaAdminError::UnsupportedSystem)?;
    #[cfg(test)]
    let executable = std::env::var_os("BONGOCAT_TEST_ALPHA_EXECUTABLE")
        .map(PathBuf::from)
        .unwrap_or(executable);
    let mut child = Command::new("/usr/bin/pkexec")
        .arg(executable)
        .args(["--alpha-helper", action.argument()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| AlphaAdminError::UnsupportedSystem)?;
    let status = wait_for_authorization(&mut child, cancel)?;
    alpha_admin_exit_status(status.code())
}

/// A caller can terminate an unanswered authorization while pkexec still has
/// the caller's real uid. Once pkexec has changed all uids to root, do not
/// interrupt the helper: it must complete its own transaction or rollback.
/// Linux checks the target uid when sending the signal; once all uids have
/// become root the unprivileged caller cannot signal the helper.
/// See https://github.com/polkit-org/polkit/blob/main/src/programs/pkexec.c
fn wait_for_authorization(
    child: &mut Child,
    cancel: &AtomicBool,
) -> Result<ExitStatus, AlphaAdminError> {
    let caller_uid = real_uid(Path::new("/proc/self/status"));
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|_| AlphaAdminError::OperationFailed)?
        {
            return Ok(status);
        }
        if cancel.load(Ordering::Acquire) {
            let status_path = Path::new("/proc")
                .join(child.id().to_string())
                .join("status");
            let target_uid = real_uid(&status_path);
            if caller_uid.is_some_and(|uid| uid != 0 && Some(uid) == target_uid)
                && child.kill().is_ok()
            {
                let _ = child.wait();
                return Err(AlphaAdminError::OperationFailed);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn real_uid(path: &Path) -> Option<u32> {
    fs::read_to_string(path)
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn alpha_admin_exit_status(code: Option<i32>) -> Result<(), AlphaAdminError> {
    match code {
        Some(0) => Ok(()),
        Some(21 | 126 | 127) => Err(AlphaAdminError::AuthorizationDenied),
        Some(22) => Err(AlphaAdminError::DriverMissing),
        Some(23) => Err(AlphaAdminError::UnsupportedSystem),
        Some(24) => Err(AlphaAdminError::Busy),
        Some(25) => Err(AlphaAdminError::OwnershipConflict),
        Some(27) => Err(AlphaAdminError::PermissionDenied),
        Some(28) => Err(AlphaAdminError::RollbackFailed),
        _ => Err(AlphaAdminError::OperationFailed),
    }
}

struct Paths {
    environment: BuildEnvironment,
    state_dir: PathBuf,
    rule: PathBuf,
}

impl Paths {
    fn system(environment: BuildEnvironment) -> Self {
        let suffix = environment.directory_name();
        Self {
            environment,
            state_dir: Path::new("/var/lib").join(format!("bongocat-alpha-{suffix}")),
            rule: Path::new("/etc/udev/rules.d").join(format!("90-bongocat-alpha-{suffix}.rules")),
        }
    }
    fn state(&self) -> PathBuf {
        self.state_dir.join("state")
    }
    fn rule_contents(&self, uid: u32, device: u32) -> String {
        format!(
            "SUBSYSTEM==\"video4linux\", KERNEL==\"video{}\", ATTR{{name}}==\"{}\", OWNER=\"{}\", MODE=\"0600\"\n",
            device,
            alpha_camera_name(self.environment),
            uid
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct State {
    owner_uid: u32,
    device: u32,
    boot_id: String,
    ready: bool,
    node_inode: Option<u64>,
}

impl State {
    fn matches_session(&self, uid: u32, current_boot: &str) -> bool {
        self.owner_uid == uid
            && self.ready
            && self.boot_id == current_boot
            && self.node_inode.is_some()
    }

    fn encode(&self) -> String {
        let mut value = format!(
            "1\n{}\n{}\n{}\n{}\n",
            self.owner_uid,
            self.device,
            self.boot_id,
            if self.ready { "ready" } else { "creating" }
        );
        if let Some(inode) = self.node_inode {
            value.push_str(&format!("{inode}\n"));
        }
        value
    }
    fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        if lines.next()? != "1" {
            return None;
        }
        let owner_uid: u32 = lines.next()?.parse().ok()?;
        let device: u32 = lines.next()?.parse().ok()?;
        let boot_id = lines.next()?.to_owned();
        let ready = match lines.next()? {
            "ready" => true,
            "creating" => false,
            _ => return None,
        };
        let node_inode = match lines.next() {
            Some(value) => Some(value.parse::<u64>().ok().filter(|inode| *inode != 0)?),
            None => None,
        };
        if owner_uid == 0 || device > 4095 || !valid_boot_id(&boot_id) || lines.next().is_some() {
            return None;
        }
        Some(Self {
            owner_uid,
            device,
            boot_id,
            ready,
            node_inode,
        })
    }
}

fn valid_boot_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn boot_id() -> Result<String, AlphaAdminError> {
    let value = fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map_err(|_| AlphaAdminError::UnsupportedSystem)?;
    let value = value.trim().to_owned();
    if !valid_boot_id(&value) {
        return Err(AlphaAdminError::UnsupportedSystem);
    }
    Ok(value)
}

fn trusted_file(path: &Path) -> Result<bool, AlphaAdminError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
                return Err(AlphaAdminError::OwnershipConflict);
            }
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(AlphaAdminError::OperationFailed),
    }
}

fn ensure_state_dir(path: &Path) -> Result<(), AlphaAdminError> {
    match fs::create_dir(path) {
        Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(0o711))
            .map_err(|_| AlphaAdminError::OperationFailed)?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(AlphaAdminError::OperationFailed),
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| AlphaAdminError::OperationFailed)?;
    if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o066 != 0 {
        return Err(AlphaAdminError::OwnershipConflict);
    }
    Ok(())
}

fn managed_file(path: &Path, contents: &str) -> Result<bool, AlphaAdminError> {
    if trusted_file(path)? {
        return if fs::read(path).map_err(|_| AlphaAdminError::OperationFailed)?
            == contents.as_bytes()
        {
            Ok(false)
        } else {
            Err(AlphaAdminError::OwnershipConflict)
        };
    }
    let parent = path.parent().ok_or(AlphaAdminError::OperationFailed)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(AlphaAdminError::OperationFailed)?;
    let temporary = parent.join(format!(".{name}.bongocat-{}-tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(&temporary)
        .map_err(|_| AlphaAdminError::OperationFailed)?;
    let result = file
        .write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::hard_link(&temporary, path))
        .map_err(|_| AlphaAdminError::OperationFailed);
    let cleanup = fs::remove_file(&temporary).map_err(|_| AlphaAdminError::OperationFailed);
    result?;
    cleanup?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| AlphaAdminError::OperationFailed)?;
    Ok(true)
}

fn read_state(paths: &Paths) -> Result<Option<State>, AlphaAdminError> {
    if !trusted_file(&paths.state())? {
        return Ok(None);
    }
    let text = fs::read_to_string(paths.state()).map_err(|_| AlphaAdminError::OperationFailed)?;
    State::parse(&text)
        .map(Some)
        .ok_or(AlphaAdminError::OwnershipConflict)
}

fn write_state(paths: &Paths, state: &State) -> Result<(), AlphaAdminError> {
    let temporary = paths.state_dir.join("state.tmp");
    if trusted_file(&temporary)? {
        fs::remove_file(&temporary).map_err(|_| AlphaAdminError::OperationFailed)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|_| AlphaAdminError::OperationFailed)?;
    file.write_all(state.encode().as_bytes())
        .and_then(|()| file.set_permissions(fs::Permissions::from_mode(0o644)))
        .and_then(|()| file.sync_all())
        .map_err(|_| AlphaAdminError::OperationFailed)?;
    fs::rename(&temporary, paths.state()).map_err(|_| AlphaAdminError::OperationFailed)?;
    File::open(&paths.state_dir)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| AlphaAdminError::OperationFailed)
}

fn run(program: &str, args: &[&str]) -> Result<(), AlphaAdminError> {
    run_with_timeout(program, args, Duration::from_secs(10))
}

fn run_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<(), AlphaAdminError> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| AlphaAdminError::UnsupportedSystem)?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|_| AlphaAdminError::OperationFailed)?
        {
            return if status.success() {
                Ok(())
            } else {
                Err(AlphaAdminError::OperationFailed)
            };
        }
        if Instant::now() >= deadline {
            child.kill().map_err(|_| AlphaAdminError::OperationFailed)?;
            child.wait().map_err(|_| AlphaAdminError::OperationFailed)?;
            return Err(AlphaAdminError::OperationFailed);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn ensure_module() -> Result<(), AlphaAdminError> {
    if Path::new("/sys/module/v4l2loopback").is_dir() {
        return Ok(());
    }
    if run("/usr/bin/modinfo", &["-F", "filename", "v4l2loopback"]).is_err() {
        return Err(AlphaAdminError::DriverMissing);
    }
    run("/usr/bin/modprobe", &["v4l2loopback", "devices=0"])
}

fn node_path(number: u32) -> PathBuf {
    Path::new("/dev").join(format!("video{number}"))
}
fn sysfs_name(number: u32) -> PathBuf {
    Path::new("/sys/class/video4linux")
        .join(format!("video{number}"))
        .join("name")
}

fn device_inode(number: u32) -> Result<u64, AlphaAdminError> {
    use std::os::unix::fs::FileTypeExt;
    let metadata = fs::symlink_metadata(node_path(number)).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => AlphaAdminError::OperationFailed,
        _ => AlphaAdminError::OwnershipConflict,
    })?;
    if !metadata.file_type().is_char_device() || metadata.ino() == 0 {
        return Err(AlphaAdminError::OwnershipConflict);
    }
    Ok(metadata.ino())
}

fn node_matches(paths: &Paths, number: u32) -> Result<bool, AlphaAdminError> {
    let node = node_path(number);
    let sysfs = sysfs_name(number);
    let node_exists = fs::symlink_metadata(&node).is_ok();
    let name = match fs::read_to_string(sysfs) {
        Ok(name) => name,
        Err(error) if error.kind() == io::ErrorKind::NotFound && !node_exists => return Ok(false),
        Err(_) => return Err(AlphaAdminError::OwnershipConflict),
    };
    if name.trim_end() != alpha_camera_name(paths.environment) || !node_exists {
        return Err(AlphaAdminError::OwnershipConflict);
    }
    let metadata = fs::symlink_metadata(&node).map_err(|_| AlphaAdminError::OwnershipConflict)?;
    use std::os::unix::fs::FileTypeExt;
    if !metadata.file_type().is_char_device() {
        return Err(AlphaAdminError::OwnershipConflict);
    }
    let device = Device::with_path(node).map_err(|_| AlphaAdminError::Busy)?;
    let caps = device
        .query_caps()
        .map_err(|_| AlphaAdminError::OwnershipConflict)?;
    if caps.driver != "v4l2 loopback" || caps.card != alpha_camera_name(paths.environment) {
        return Err(AlphaAdminError::OwnershipConflict);
    }
    Ok(true)
}

fn path_missing(path: &Path) -> Result<bool, AlphaAdminError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(_) => Err(AlphaAdminError::OperationFailed),
    }
}

fn free_node() -> Result<u32, AlphaAdminError> {
    for number in 0..=4095 {
        let sysfs = sysfs_name(number);
        let parent = sysfs.parent().ok_or(AlphaAdminError::OperationFailed)?;
        if path_missing(&node_path(number))? && path_missing(parent)? {
            return Ok(number);
        }
    }
    Err(AlphaAdminError::Busy)
}

fn configure_udev(number: u32, uid: u32) -> Result<(), AlphaAdminError> {
    run("/usr/bin/udevadm", &["control", "--reload"])?;
    let name = format!("--sysname-match=video{number}");
    run(
        "/usr/bin/udevadm",
        &[
            "trigger",
            "--action=change",
            "--subsystem-match=video4linux",
            &name,
        ],
    )?;
    run("/usr/bin/udevadm", &["settle", "--timeout=3"])?;
    if fs::metadata(node_path(number))
        .map_err(|_| AlphaAdminError::OperationFailed)?
        .uid()
        != uid
    {
        return Err(AlphaAdminError::PermissionDenied);
    }
    Ok(())
}

fn ensure_device(paths: &Paths, mut state: State) -> Result<State, AlphaAdminError> {
    ensure_module()?;
    let current_boot = boot_id()?;
    if state.boot_id == current_boot && node_matches(paths, state.device)? {
        let inode = device_inode(state.device)?;
        if state.node_inode != Some(inode) && !(state.ready && state.node_inode.is_none()) {
            return Err(AlphaAdminError::OwnershipConflict);
        }
        // Only a completed legacy installation can enroll an existing node
        // without a recorded inode.
        state.node_inode = Some(inode);
        configure_udev(state.device, state.owner_uid)?;
        state.ready = true;
        write_state(paths, &state)?;
        return Ok(state);
    }
    if state.boot_id != current_boot
        && (fs::symlink_metadata(node_path(state.device)).is_ok()
            || fs::symlink_metadata(sysfs_name(state.device).parent().expect("sysfs parent"))
                .is_ok())
    {
        return Err(AlphaAdminError::OwnershipConflict);
    }
    state.boot_id = current_boot;
    state.ready = false;
    state.node_inode = None;
    write_state(paths, &state)?;
    let node = node_path(state.device);
    let node = node.to_str().ok_or(AlphaAdminError::OperationFailed)?;
    run(
        "/usr/bin/v4l2loopback-ctl",
        &["add", "-n", alpha_camera_name(paths.environment), node],
    )?;
    if !node_matches(paths, state.device)? {
        return Err(AlphaAdminError::OperationFailed);
    }
    state.node_inode = Some(device_inode(state.device)?);
    // Publish the device identity before any later operation can fail, so a
    // rollback never deletes an unverified replacement.
    write_state(paths, &state)?;
    configure_udev(state.device, state.owner_uid)?;
    state.ready = true;
    write_state(paths, &state)?;
    Ok(state)
}

fn install(paths: &Paths, uid: u32) -> Result<(), AlphaAdminError> {
    ensure_module()?;
    let existing_state = read_state(paths)?;
    let is_new = existing_state.is_none();
    let state = match existing_state {
        Some(state) if state.owner_uid == uid => state,
        Some(_) => return Err(AlphaAdminError::AuthorizationDenied),
        None => {
            if super::alpha_output::has_named_camera(alpha_camera_name(paths.environment))? {
                return Err(AlphaAdminError::OwnershipConflict);
            }
            let state = State {
                owner_uid: uid,
                device: free_node()?,
                boot_id: boot_id()?,
                ready: false,
                node_inode: None,
            };
            write_state(paths, &state)?;
            state
        }
    };
    let result = (|| {
        managed_file(&paths.rule, &paths.rule_contents(uid, state.device))?;
        run("/usr/bin/udevadm", &["control", "--reload"])?;
        ensure_device(paths, state).map(|_| ())
    })();
    if result.is_err() && is_new && uninstall(paths, uid).is_err() {
        // Retain the ownership record if rollback cannot safely finish.
        return Err(AlphaAdminError::RollbackFailed);
    }
    result
}

fn remove_managed(path: &Path, expected: &str) -> Result<(), AlphaAdminError> {
    if !trusted_file(path)? {
        return Ok(());
    }
    if fs::read(path).map_err(|_| AlphaAdminError::OperationFailed)? != expected.as_bytes() {
        return Err(AlphaAdminError::OwnershipConflict);
    }
    fs::remove_file(path).map_err(|_| AlphaAdminError::OperationFailed)
}

fn uninstall(paths: &Paths, uid: u32) -> Result<(), AlphaAdminError> {
    let Some(state) = read_state(paths)? else {
        return Ok(());
    };
    if state.owner_uid != uid {
        return Err(AlphaAdminError::AuthorizationDenied);
    }
    if trusted_file(&paths.rule)?
        && fs::read(&paths.rule).map_err(|_| AlphaAdminError::OperationFailed)?
            != paths.rule_contents(uid, state.device).as_bytes()
    {
        return Err(AlphaAdminError::OwnershipConflict);
    }
    if state.boot_id == boot_id()? && node_matches(paths, state.device)? {
        if state.node_inode != Some(device_inode(state.device)?) {
            return Err(AlphaAdminError::OwnershipConflict);
        }
        let node = node_path(state.device);
        run(
            "/usr/bin/v4l2loopback-ctl",
            &[
                "delete",
                node.to_str().ok_or(AlphaAdminError::OperationFailed)?,
            ],
        )
        .map_err(|_| AlphaAdminError::Busy)?;
    }
    remove_managed(&paths.rule, &paths.rule_contents(uid, state.device))?;
    run("/usr/bin/udevadm", &["control", "--reload"])?;
    fs::remove_file(paths.state()).map_err(|_| AlphaAdminError::OperationFailed)?;
    Ok(())
}

/// Execute a single fixed administrative operation after authentication.
pub fn run_alpha_helper(
    environment: BuildEnvironment,
    operation: &str,
) -> Result<(), AlphaAdminError> {
    if !matches!(operation, "install" | "uninstall") {
        return Err(AlphaAdminError::OperationFailed);
    }
    let paths = Paths::system(environment);
    for directory in [
        Path::new("/usr"),
        Path::new("/usr/bin"),
        Path::new("/var"),
        Path::new("/var/lib"),
        Path::new("/etc"),
        Path::new("/etc/udev"),
        Path::new("/etc/udev/rules.d"),
    ] {
        let metadata =
            fs::symlink_metadata(directory).map_err(|_| AlphaAdminError::OwnershipConflict)?;
        if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(AlphaAdminError::OwnershipConflict);
        }
    }
    let status = fs::read_to_string("/proc/self/status")
        .map_err(|_| AlphaAdminError::AuthorizationDenied)?;
    let effective_uid = status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u32>().ok());
    if effective_uid != Some(0) {
        return Err(AlphaAdminError::AuthorizationDenied);
    }
    let uid = || {
        std::env::var("PKEXEC_UID")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|uid| *uid != 0)
            .ok_or(AlphaAdminError::AuthorizationDenied)
    };
    ensure_state_dir(&paths.state_dir)?;
    let lock_path = paths.state_dir.join("manager.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(lock_path)
        .map_err(|_| AlphaAdminError::OperationFailed)?;
    lock.try_lock().map_err(|_| AlphaAdminError::Busy)?;
    let operation_uid = uid()?;
    match operation {
        "install" => install(&paths, operation_uid),
        "uninstall" => uninstall(&paths, operation_uid),
        _ => Err(AlphaAdminError::OperationFailed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_system_command_timeout_reaps_the_child() {
        let started = Instant::now();
        assert_eq!(
            run_with_timeout("/usr/bin/sleep", &["10"], Duration::from_millis(50)),
            Err(AlphaAdminError::OperationFailed)
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            run_with_timeout("/usr/bin/true", &[], Duration::from_secs(1)),
            Ok(())
        );
    }

    #[test]
    fn cancellation_reaps_an_unprivileged_authorization_process() {
        if real_uid(Path::new("/proc/self/status")) == Some(0) {
            return;
        }
        let mut child = Command::new("/usr/bin/sleep").arg("10").spawn().unwrap();
        let cancel = AtomicBool::new(true);
        let started = Instant::now();
        assert_eq!(
            wait_for_authorization(&mut child, &cancel).unwrap_err(),
            AlphaAdminError::OperationFailed
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn authorization_and_helper_failures_remain_distinct() {
        assert_eq!(alpha_admin_exit_status(Some(0)), Ok(()));
        assert_eq!(
            alpha_admin_exit_status(Some(126)),
            Err(AlphaAdminError::AuthorizationDenied)
        );
        assert_eq!(
            alpha_admin_exit_status(Some(127)),
            Err(AlphaAdminError::AuthorizationDenied)
        );
        assert_eq!(
            alpha_admin_exit_status(Some(22)),
            Err(AlphaAdminError::DriverMissing)
        );
        assert_eq!(
            alpha_admin_exit_status(Some(24)),
            Err(AlphaAdminError::Busy)
        );
        assert_eq!(
            alpha_admin_exit_status(Some(28)),
            Err(AlphaAdminError::RollbackFailed)
        );
        assert_eq!(
            alpha_admin_exit_status(None),
            Err(AlphaAdminError::OperationFailed)
        );
    }

    #[test]
    fn free_node_probe_rejects_errors_other_than_missing() {
        let temporary = tempfile::tempdir().unwrap();
        let regular_file = temporary.path().join("file");
        fs::write(&regular_file, b"x").unwrap();
        assert_eq!(path_missing(&temporary.path().join("missing")), Ok(true));
        assert_eq!(path_missing(&regular_file), Ok(false));
        assert_eq!(
            path_missing(&regular_file.join("child")),
            Err(AlphaAdminError::OperationFailed)
        );
    }

    #[test]
    fn camera_identity_requires_owner_current_boot_and_ready_state() {
        let mut state = State {
            owner_uid: 1000,
            device: 6,
            boot_id: "01234567-89ab-cdef-0123-456789abcdef".into(),
            ready: true,
            node_inode: Some(42),
        };
        let current = state.boot_id.clone();
        assert!(state.matches_session(1000, &current));
        assert!(!state.matches_session(1001, &current));
        assert!(!state.matches_session(1000, "00000000-0000-0000-0000-000000000000"));
        state.ready = false;
        assert!(!state.matches_session(1000, &current));
    }

    #[test]
    fn state_round_trips_and_rejects_invalid_or_future_values() {
        let state = State {
            owner_uid: 1000,
            device: 6,
            boot_id: "01234567-89ab-cdef-0123-456789abcdef".into(),
            ready: true,
            node_inode: Some(42),
        };
        assert_eq!(State::parse(&state.encode()), Some(state));
        let legacy = State::parse("1\n1000\n6\n01234567-89ab-cdef-0123-456789abcdef\nready\n")
            .expect("existing private records remain readable by the installer");
        assert_eq!(legacy.node_inode, None);
        for value in [
            "2\n1000\n6\n01234567-89ab-cdef-0123-456789abcdef\nready\n",
            "1\n0\n6\n01234567-89ab-cdef-0123-456789abcdef\nready\n",
            "1\n1000\n4096\n01234567-89ab-cdef-0123-456789abcdef\nready\n",
            "1\n1000\n6\nnot-a-boot-id\nready\n",
            "1\n1000\n6\n01234567-89ab-cdef-0123-456789abcdef\nready\nextra\n",
        ] {
            assert!(State::parse(value).is_none());
        }
    }

    #[test]
    #[ignore = "requires a dedicated external polkit agent and test application executable"]
    fn real_pending_pkexec_authorization_is_cancelled() {
        let ready = std::env::var("BONGOCAT_TEST_AGENT_READY").unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        while !Path::new(&ready).exists() {
            assert!(Instant::now() < deadline, "polkit test agent not ready");
            std::thread::sleep(Duration::from_millis(20));
        }
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let setter = std::sync::Arc::clone(&cancel);
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            setter.store(true, Ordering::Release);
        });
        let started = Instant::now();
        assert_eq!(
            manage_alpha_camera(
                BuildEnvironment::Development,
                AlphaAdminAction::Install,
                &cancel
            ),
            Err(AlphaAdminError::OperationFailed)
        );
        thread.join().unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "pending authorization did not exit"
        );
        assert!(
            !super::super::alpha_output::has_named_camera(alpha_camera_name(
                BuildEnvironment::Development
            ))
            .unwrap()
        );
    }

    #[test]
    #[ignore = "requires a dedicated external polkit agent and test application executable"]
    fn real_pkexec_management_with_external_agent() {
        let ready = std::env::var("BONGOCAT_TEST_AGENT_READY").unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        while !Path::new(&ready).exists() {
            assert!(Instant::now() < deadline, "polkit test agent not ready");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !super::super::alpha_output::has_named_camera(alpha_camera_name(
                BuildEnvironment::Development
            ))
            .unwrap()
        );
        let cancel = AtomicBool::new(false);
        manage_alpha_camera(
            BuildEnvironment::Development,
            AlphaAdminAction::Install,
            &cancel,
        )
        .unwrap();
        assert!(
            super::super::alpha_output::has_named_camera(alpha_camera_name(
                BuildEnvironment::Development
            ))
            .unwrap()
        );
        manage_alpha_camera(
            BuildEnvironment::Development,
            AlphaAdminAction::Uninstall,
            &cancel,
        )
        .unwrap();
        assert!(
            !super::super::alpha_output::has_named_camera(alpha_camera_name(
                BuildEnvironment::Development
            ))
            .unwrap()
        );
    }

    #[test]
    #[ignore = "reads real development camera state"]
    fn real_development_camera_status() {
        let expected = std::env::var("BONGOCAT_TEST_ALPHA_EXPECT").unwrap();
        let status = inspect_alpha_system(BuildEnvironment::Development);
        let expected = match expected.as_str() {
            "installed" => AlphaInstallationStatus::Installed,
            "absent" => AlphaInstallationStatus::NotInstalled,
            "recovery" => AlphaInstallationStatus::RecoveryRequired,
            other => panic!("unexpected test state {other}"),
        };
        assert_eq!(status.camera, expected);
        assert_eq!(status.driver_installed, Some(true));
        assert!(status.module_loaded);
        assert!(status.management_available);
    }

    #[test]
    #[ignore = "manages a disposable production camera for isolated integration tests"]
    fn real_production_manager_operation() {
        let operation = std::env::var("BONGOCAT_TEST_OPERATION").unwrap();
        assert!(matches!(operation.as_str(), "install" | "uninstall"));
        run_alpha_helper(BuildEnvironment::Production, &operation).unwrap();
    }

    #[test]
    #[ignore = "reads the isolated production camera identity"]
    fn real_production_identity_status() {
        use super::super::alpha_output::{AlphaCameraError, LinuxAlphaCamera};
        let expected = std::env::var("BONGOCAT_TEST_ALPHA_EXPECT").unwrap();
        let status = inspect_alpha_system(BuildEnvironment::Production);
        match expected.as_str() {
            "installed" => {
                assert_eq!(status.camera, AlphaInstallationStatus::Installed);
                let camera = LinuxAlphaCamera::discover(BuildEnvironment::Production)
                    .expect("verified camera opens");
                drop(camera);
            }
            "recovery" => {
                assert_eq!(status.camera, AlphaInstallationStatus::RecoveryRequired);
                assert!(matches!(
                    LinuxAlphaCamera::discover(BuildEnvironment::Production),
                    Err(AlphaCameraError::Unavailable)
                ));
            }
            "conflict" => {
                assert_eq!(status.camera, AlphaInstallationStatus::Conflict);
                assert!(matches!(
                    LinuxAlphaCamera::discover(BuildEnvironment::Production),
                    Err(AlphaCameraError::InvalidDevice)
                ));
            }
            other => panic!("unexpected status {other}"),
        }
    }

    #[test]
    #[ignore = "creates a disposable production camera and permission rule"]
    fn real_camera_identity_rejects_old_boot_and_private_legacy_record() {
        let paths = Paths::system(BuildEnvironment::Production);
        assert!(!paths.state().exists());
        assert!(!paths.rule.exists());
        assert!(
            !super::super::alpha_output::has_named_camera(alpha_camera_name(paths.environment))
                .unwrap()
        );
        let uid: u32 = std::env::var("PKEXEC_UID").unwrap().parse().unwrap();
        let user = std::env::var("BONGOCAT_TEST_USER").unwrap();
        ensure_state_dir(&paths.state_dir).unwrap();
        struct Cleanup<'a> {
            paths: &'a Paths,
            uid: u32,
        }
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                if let Err(error) = uninstall(self.paths, self.uid) {
                    eprintln!("identity test cleanup failed: {error}");
                }
            }
        }
        let _cleanup = Cleanup { paths: &paths, uid };
        let check = |expected: &str| {
            let status = Command::new("/usr/bin/runuser")
                .args(["-u", &user, "--", "/usr/bin/env"])
                .arg(format!("BONGOCAT_TEST_ALPHA_EXPECT={expected}"))
                .arg(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "linux::alpha_admin::tests::real_production_identity_status",
                ])
                .status()
                .unwrap();
            assert!(status.success(), "desktop identity check failed");
        };

        install(&paths, uid).unwrap();
        let state = read_state(&paths).unwrap().unwrap();
        assert_eq!(fs::metadata(paths.state()).unwrap().mode() & 0o777, 0o644);
        check("installed");

        let mut previous_boot = state.clone();
        previous_boot.boot_id = "00000000-0000-0000-0000-000000000000".into();
        write_state(&paths, &previous_boot).unwrap();
        check("recovery");
        // Return the test record to this boot before teardown can delete it.
        write_state(&paths, &state).unwrap();

        let mut legacy = state.clone();
        legacy.node_inode = None;
        legacy.ready = false;
        write_state(&paths, &legacy).unwrap();
        let result = install(&paths, uid);
        let after_attempt = read_state(&paths).unwrap().unwrap();
        let after_inode = device_inode(state.device).unwrap();
        // Restore the identity for cleanup even if the assertion fails.
        write_state(&paths, &state).unwrap();
        assert_eq!(result, Err(AlphaAdminError::OwnershipConflict));
        assert!(!after_attempt.ready);
        assert_eq!(after_attempt.node_inode, None);
        assert_eq!(Some(after_inode), state.node_inode);

        legacy.ready = true;
        write_state(&paths, &legacy).unwrap();
        fs::set_permissions(paths.state(), fs::Permissions::from_mode(0o600)).unwrap();
        check("recovery");
        // Reinstall upgrades the existing private record without replacing the
        // device, even while a capture client holds it open.
        let existing_inode = device_inode(state.device).unwrap();
        let capture_client = Device::with_path(node_path(state.device)).unwrap();
        install(&paths, uid).unwrap();
        assert_eq!(read_state(&paths).unwrap().unwrap().device, state.device);
        assert_eq!(device_inode(state.device).unwrap(), existing_inode);
        drop(capture_client);
        check("installed");

        // Even an identical label and device number cannot authenticate a
        // replacement created later in the same boot.
        let node = node_path(state.device);
        run(
            "/usr/bin/v4l2loopback-ctl",
            &["delete", node.to_str().unwrap()],
        )
        .unwrap();
        run(
            "/usr/bin/v4l2loopback-ctl",
            &[
                "add",
                "-n",
                alpha_camera_name(paths.environment),
                node.to_str().unwrap(),
            ],
        )
        .unwrap();
        struct Replacement {
            device: u32,
            inode: u64,
        }
        impl Drop for Replacement {
            fn drop(&mut self) {
                if device_inode(self.device).ok() == Some(self.inode)
                    && fs::read_to_string(sysfs_name(self.device))
                        .ok()
                        .as_deref()
                        .map(str::trim_end)
                        == Some(alpha_camera_name(BuildEnvironment::Production))
                {
                    let path = node_path(self.device);
                    let _ = run(
                        "/usr/bin/v4l2loopback-ctl",
                        &["delete", path.to_str().unwrap()],
                    );
                }
            }
        }
        let replacement = Replacement {
            device: state.device,
            inode: device_inode(state.device).unwrap(),
        };
        assert_ne!(Some(replacement.inode), state.node_inode);
        check("conflict");
        assert_eq!(
            uninstall(&paths, uid),
            Err(AlphaAdminError::OwnershipConflict)
        );
        assert_eq!(device_inode(state.device).unwrap(), replacement.inode);
        drop(replacement);
        assert!(!node.exists());
    }

    #[test]
    #[ignore = "requires an installed camera with deliberately restricted permissions"]
    fn real_development_camera_permission_denied() {
        assert!(matches!(
            super::super::alpha_output::LinuxAlphaCamera::discover(BuildEnvironment::Development),
            Err(super::super::alpha_output::AlphaCameraError::PermissionDenied)
        ));
    }

    fn check_permission_as_desktop_user() {
        let user = std::env::var("BONGOCAT_TEST_USER").unwrap();
        assert!(
            Command::new("/usr/bin/runuser")
                .args(["-u", &user, "--"])
                .arg(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "linux::alpha_admin::tests::real_development_camera_permission_denied"
                ])
                .status()
                .unwrap()
                .success()
        );
    }

    fn check_status_as_desktop_user(expected: &str) {
        let user = std::env::var("BONGOCAT_TEST_USER").unwrap();
        let success = Command::new("/usr/bin/runuser")
            .args(["-u", &user, "--", "/usr/bin/env"])
            .arg(format!("BONGOCAT_TEST_ALPHA_EXPECT={expected}"))
            .arg(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "linux::alpha_admin::tests::real_development_camera_status",
                "--nocapture",
            ])
            .status()
            .unwrap()
            .success();
        assert!(success);
    }

    #[test]
    #[ignore = "creates a real development camera and udev rule"]
    fn real_development_camera_lifecycle() {
        let paths = Paths::system(BuildEnvironment::Development);
        assert!(!paths.state().exists());
        assert!(!paths.rule.exists());
        assert!(
            !super::super::alpha_output::has_named_camera(alpha_camera_name(paths.environment))
                .unwrap()
        );
        let uid: u32 = std::env::var("PKEXEC_UID").unwrap().parse().unwrap();
        struct Cleanup<'a> {
            paths: &'a Paths,
            uid: u32,
        }
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                if let Err(error) = uninstall(self.paths, self.uid) {
                    eprintln!("development camera cleanup failed: {error}");
                }
            }
        }
        let _cleanup = Cleanup { paths: &paths, uid };
        check_status_as_desktop_user("absent");
        run_alpha_helper(paths.environment, "install").unwrap();
        run_alpha_helper(paths.environment, "install").unwrap();
        let state = read_state(&paths).unwrap().unwrap();
        assert!(state.ready);
        assert!(node_matches(&paths, state.device).unwrap());
        check_status_as_desktop_user("installed");
        let camera_node = node_path(state.device);
        fs::set_permissions(&camera_node, fs::Permissions::from_mode(0o000)).unwrap();
        check_permission_as_desktop_user();
        check_status_as_desktop_user("recovery");
        fs::set_permissions(&camera_node, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(fs::metadata(node_path(state.device)).unwrap().uid(), uid);
        let active_user = Device::with_path(node_path(state.device)).unwrap();
        assert_eq!(
            run_alpha_helper(paths.environment, "uninstall"),
            Err(AlphaAdminError::Busy),
            "an open camera must not be removed"
        );
        assert!(node_matches(&paths, state.device).unwrap());
        drop(active_user);
        let node = node_path(state.device);
        run(
            "/usr/bin/v4l2loopback-ctl",
            &["delete", node.to_str().unwrap()],
        )
        .unwrap();
        let mut before_reboot = state;
        before_reboot.boot_id = "00000000-0000-0000-0000-000000000000".into();
        write_state(&paths, &before_reboot).unwrap();

        // Simulate a different device claiming the old video number after reboot.
        // Reinstallation must fail closed and leave that foreign device untouched.
        struct Sentinel {
            number: u32,
            name: String,
        }
        impl Drop for Sentinel {
            fn drop(&mut self) {
                let name = fs::read_to_string(sysfs_name(self.number)).ok();
                if name.as_deref().map(str::trim_end) == Some(self.name.as_str()) {
                    let node = node_path(self.number);
                    if let Err(error) = run(
                        "/usr/bin/v4l2loopback-ctl",
                        &["delete", node.to_str().unwrap()],
                    ) {
                        eprintln!("test sentinel cleanup failed: {error}");
                    }
                }
            }
        }
        let sentinel = Sentinel {
            number: before_reboot.device,
            name: format!("BC-test-{}", std::process::id()),
        };
        run(
            "/usr/bin/v4l2loopback-ctl",
            &["add", "-n", &sentinel.name, node.to_str().unwrap()],
        )
        .unwrap();
        assert_eq!(
            run_alpha_helper(paths.environment, "install"),
            Err(AlphaAdminError::OwnershipConflict)
        );
        assert_eq!(
            fs::read_to_string(sysfs_name(sentinel.number))
                .unwrap()
                .trim_end(),
            sentinel.name
        );
        drop(sentinel);
        assert!(!node.exists());
        run_alpha_helper(paths.environment, "install").unwrap();
        let restored = read_state(&paths).unwrap().unwrap();
        assert!(restored.ready);
        assert_eq!(restored.boot_id, boot_id().unwrap());
        assert!(node_matches(&paths, restored.device).unwrap());
        run_alpha_helper(paths.environment, "uninstall").unwrap();
        run_alpha_helper(paths.environment, "uninstall").unwrap();
        assert!(!paths.state().exists());
        assert!(!paths.rule.exists());
        assert!(!node_path(restored.device).exists());
        check_status_as_desktop_user("absent");

        // A foreign rule must never be overwritten or removed by rollback.
        fs::write(&paths.rule, b"foreign rule\n").unwrap();
        assert_eq!(
            run_alpha_helper(paths.environment, "install"),
            Err(AlphaAdminError::RollbackFailed)
        );
        assert_eq!(fs::read(&paths.rule).unwrap(), b"foreign rule\n");
        fs::remove_file(&paths.rule).unwrap();
        run_alpha_helper(paths.environment, "uninstall").unwrap();
        assert!(!paths.state().exists());
    }

    #[test]
    fn environments_have_disjoint_system_files_and_device_labels() {
        let dev = Paths::system(BuildEnvironment::Development);
        let prod = Paths::system(BuildEnvironment::Production);
        assert_ne!(dev.state_dir, prod.state_dir);
        assert_ne!(dev.rule, prod.rule);
        assert_ne!(
            alpha_camera_name(dev.environment),
            alpha_camera_name(prod.environment)
        );
    }
}
