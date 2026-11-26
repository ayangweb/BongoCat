//! Non-blocking V4L2 output for the dedicated Alpha camera.
//!
//! This adapter owns the device handle and never exposes V4L2 types. Device
//! creation and removal belong to a separate privileged lifecycle operation.

use std::io::{self, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;
use v4l::video::Output;
use v4l::{Device, Format, FourCC};

pub const ALPHA_CAMERA_NAME: &str = "BongoCat Alpha";
pub const ALPHA_CAMERA_DEVELOPMENT_NAME: &str = "BongoCat Alpha (Development)";

pub const fn alpha_camera_name(environment: bongocat_config::BuildEnvironment) -> &'static str {
    match environment {
        bongocat_config::BuildEnvironment::Development => ALPHA_CAMERA_DEVELOPMENT_NAME,
        bongocat_config::BuildEnvironment::Production => ALPHA_CAMERA_NAME,
    }
}
pub const ALPHA_FRAME_WIDTH: u32 = 1280;
pub const ALPHA_FRAME_HEIGHT: u32 = 720;
pub const ALPHA_FRAME_BYTES: usize = ALPHA_FRAME_WIDTH as usize * ALPHA_FRAME_HEIGHT as usize * 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AlphaCameraError {
    #[error("invalid camera device")]
    InvalidDevice,
    #[error("camera device is not available")]
    Unavailable,
    #[error("camera device permission denied")]
    PermissionDenied,
    #[error("camera does not support the required Alpha format")]
    UnsupportedFormat,
    #[error("camera frame size is invalid")]
    InvalidFrame,
    #[error("camera output failed")]
    OutputFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlphaFrameWrite {
    Written,
    Dropped,
}

/// A single non-blocking output handle. Frames use straight-alpha BGRA8.
pub struct LinuxAlphaCamera {
    device: Device,
}

impl LinuxAlphaCamera {
    /// Find the device identified by the current root-owned installation record.
    pub fn discover(
        environment: bongocat_config::BuildEnvironment,
    ) -> Result<Self, AlphaCameraError> {
        let (node, inode) = super::alpha_admin::authorized_camera_node(environment)?;
        Self::open_named(&node, alpha_camera_name(environment), Some(inode))
    }

    /// Open only a video node with the exact BongoCat camera identity.
    /// The privileged manager must independently establish ownership before
    /// creating or deleting a device; the card label is not ownership proof.
    pub fn open(
        path: &Path,
        environment: bongocat_config::BuildEnvironment,
    ) -> Result<Self, AlphaCameraError> {
        let (authorized, inode) = super::alpha_admin::authorized_camera_node(environment)?;
        if path != authorized {
            return Err(AlphaCameraError::InvalidDevice);
        }
        Self::open_named(path, alpha_camera_name(environment), Some(inode))
    }

    fn open_named(
        path: &Path,
        expected_name: &str,
        expected_inode: Option<u64>,
    ) -> Result<Self, AlphaCameraError> {
        let name = video_node_name(path).ok_or(AlphaCameraError::InvalidDevice)?;
        let node = std::fs::symlink_metadata(path).map_err(map_io)?;
        if !node.file_type().is_char_device() {
            return Err(AlphaCameraError::InvalidDevice);
        }
        let sysfs = Path::new("/sys/class/video4linux").join(name).join("name");
        let card_name = std::fs::read_to_string(sysfs).map_err(map_io)?;
        if card_name.trim_end() != expected_name {
            return Err(AlphaCameraError::InvalidDevice);
        }
        let device = Device::with_path(path).map_err(map_io)?;
        // Verify the opened handle itself, so replacing the path between the
        // record check and open cannot redirect frames to another device.
        let descriptor = format!("/proc/self/fd/{}", device.handle().fd());
        let opened = std::fs::metadata(descriptor).map_err(map_io)?;
        if !opened.file_type().is_char_device()
            || opened.ino() != expected_inode.unwrap_or_else(|| node.ino())
        {
            return Err(AlphaCameraError::InvalidDevice);
        }
        // v4l2loopback card strings are kernel-provided and were checked via
        // sysfs above. No raw fd or third-party type leaves this adapter.
        let capabilities = device.query_caps().map_err(map_io)?;
        if capabilities.driver != "v4l2 loopback"
            || capabilities.card != expected_name
            || !capabilities
                .capabilities
                .contains(v4l::capability::Flags::VIDEO_OUTPUT)
            || !capabilities
                .capabilities
                .contains(v4l::capability::Flags::READ_WRITE)
        {
            return Err(AlphaCameraError::InvalidDevice);
        }
        let requested = Format::new(ALPHA_FRAME_WIDTH, ALPHA_FRAME_HEIGHT, FourCC::new(b"AR24"));
        let actual = Output::set_format(&device, &requested).map_err(map_io)?;
        if actual.width != ALPHA_FRAME_WIDTH
            || actual.height != ALPHA_FRAME_HEIGHT
            || actual.fourcc != requested.fourcc
            || actual.stride != ALPHA_FRAME_WIDTH * 4
            || actual.size as usize != ALPHA_FRAME_BYTES
            || actual.flags.contains(v4l::format::Flags::PREMUL_ALPHA)
        {
            return Err(AlphaCameraError::UnsupportedFormat);
        }
        Ok(Self { device })
    }

    /// Write at most one whole frame. A busy camera drops it rather than blocking.
    pub fn write_frame(&mut self, frame: &[u8]) -> Result<AlphaFrameWrite, AlphaCameraError> {
        write_frame(&mut self.device, frame)
    }
}

pub(super) fn has_named_camera(name: &str) -> Result<bool, super::alpha_admin::AlphaAdminError> {
    matching_nodes(Path::new("/sys/class/video4linux"), name)
        .map(|nodes| !nodes.is_empty())
        .map_err(|_| super::alpha_admin::AlphaAdminError::OperationFailed)
}

fn matching_nodes(sysfs: &Path, expected_name: &str) -> Result<Vec<String>, AlphaCameraError> {
    let entries = std::fs::read_dir(sysfs).map_err(map_io)?;
    let mut matches = Vec::new();
    for entry in entries {
        let entry = entry.map_err(map_io)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if video_node_name(&Path::new("/dev").join(name)).is_none() {
            continue;
        }
        let Ok(card) = std::fs::read_to_string(entry.path().join("name")) else {
            continue;
        };
        if card.trim_end() == expected_name {
            matches.push(name.to_owned());
        }
    }
    Ok(matches)
}

fn video_node_name(path: &Path) -> Option<&str> {
    let name = path.strip_prefix("/dev").ok()?.to_str()?;
    let number = name.strip_prefix("video")?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(name)
}

fn map_io(error: io::Error) -> AlphaCameraError {
    match error.kind() {
        io::ErrorKind::PermissionDenied => AlphaCameraError::PermissionDenied,
        io::ErrorKind::NotFound => AlphaCameraError::Unavailable,
        _ => AlphaCameraError::OutputFailed,
    }
}

fn write_frame(writer: &mut impl Write, frame: &[u8]) -> Result<AlphaFrameWrite, AlphaCameraError> {
    if frame.len() != ALPHA_FRAME_BYTES {
        return Err(AlphaCameraError::InvalidFrame);
    }
    loop {
        match writer.write(frame) {
            Ok(written) if written == frame.len() => return Ok(AlphaFrameWrite::Written),
            Ok(_) => return Err(AlphaCameraError::OutputFailed),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Ok(AlphaFrameWrite::Dropped);
            }
            Err(error) => return Err(map_io(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TestWriter<F>(F);
    impl<F: FnMut(&[u8]) -> io::Result<usize>> Write for TestWriter<F> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            (self.0)(bytes)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn camera_names_are_isolated_by_build_environment() {
        use bongocat_config::BuildEnvironment;
        assert_ne!(
            alpha_camera_name(BuildEnvironment::Development),
            alpha_camera_name(BuildEnvironment::Production)
        );
    }

    #[test]
    fn discovery_matches_only_the_dedicated_card() {
        let root = tempfile::tempdir().expect("temporary sysfs");
        for (node, card) in [
            ("video1", ALPHA_CAMERA_NAME),
            ("video2", "webcam"),
            ("not-video", ALPHA_CAMERA_NAME),
        ] {
            let directory = root.path().join(node);
            std::fs::create_dir(&directory).expect("sysfs node");
            std::fs::write(directory.join("name"), format!("{card}\n")).expect("card name");
        }
        assert_eq!(
            matching_nodes(root.path(), ALPHA_CAMERA_NAME),
            Ok(vec!["video1".to_owned()])
        );
    }

    #[test]
    fn accepts_only_direct_numbered_video_nodes() {
        for good in ["/dev/video0", "/dev/video123"] {
            assert!(video_node_name(Path::new(good)).is_some());
        }
        for bad in [
            "video0",
            "/dev/video",
            "/dev/video0/../video1",
            "/tmp/video0",
            "/dev/video-1",
        ] {
            assert!(video_node_name(Path::new(bad)).is_none());
        }
    }

    #[test]
    fn rejects_wrong_frame_size_and_drops_when_busy() {
        let mut sink = io::sink();
        assert_eq!(
            write_frame(&mut sink, &[]),
            Err(AlphaCameraError::InvalidFrame)
        );
        let frame = vec![0; ALPHA_FRAME_BYTES];
        assert_eq!(write_frame(&mut sink, &frame), Ok(AlphaFrameWrite::Written));
        let mut busy = TestWriter(|_bytes: &[u8]| Err(io::Error::from(io::ErrorKind::WouldBlock)));
        assert_eq!(write_frame(&mut busy, &frame), Ok(AlphaFrameWrite::Dropped));
    }

    #[test]
    fn short_write_does_not_submit_a_second_partial_frame() {
        let frame = vec![0; ALPHA_FRAME_BYTES];
        let mut writes = 0;
        let mut short = TestWriter(|_bytes: &[u8]| {
            writes += 1;
            Ok(4)
        });
        assert_eq!(
            write_frame(&mut short, &frame),
            Err(AlphaCameraError::OutputFailed)
        );
        assert_eq!(writes, 1);
    }

    #[test]
    #[ignore = "requires an isolated managed production loopback device"]
    fn opened_handle_must_match_the_recorded_inode() {
        let path = PathBuf::from(std::env::var("BONGOCAT_ALPHA_TEST_DEVICE").unwrap());
        let inode = std::fs::symlink_metadata(&path).unwrap().ino();
        assert!(matches!(
            LinuxAlphaCamera::open_named(&path, ALPHA_CAMERA_NAME, Some(inode + 1)),
            Err(AlphaCameraError::InvalidDevice)
        ));
        let camera = LinuxAlphaCamera::open_named(&path, ALPHA_CAMERA_NAME, Some(inode))
            .expect("the actual opened device inode is accepted");
        drop(camera);
    }

    #[test]
    #[ignore = "requires a disposable v4l2loopback device named BongoCat-Alpha-Test"]
    fn writes_to_a_real_loopback_device() {
        let path = std::env::var_os("BONGOCAT_ALPHA_TEST_DEVICE").expect("test device path");
        let mut camera =
            LinuxAlphaCamera::open_named(&PathBuf::from(path), "BongoCat-Alpha-Test", None)
                .expect("open temporary camera");
        let frame = vec![0; ALPHA_FRAME_BYTES];
        assert_eq!(camera.write_frame(&frame), Ok(AlphaFrameWrite::Written));
    }
}
