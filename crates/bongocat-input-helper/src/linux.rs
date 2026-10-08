//! Privileged input owner. No GUI, model files, shell commands or user paths.
use crate::protocol::{ButtonState as PacketButtonState, HEADER, InputMessage};
use input::{
    Event, Libinput, LibinputInterface,
    event::{
        keyboard::{KeyState, KeyboardEvent, KeyboardEventTrait},
        pointer::{ButtonState, PointerEvent},
    },
};
use std::{
    fs::OpenOptions,
    io::{self, Write},
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::fs::OpenOptionsExt,
    },
    path::Path,
};
use zbus::{blocking::Proxy, zvariant::OwnedObjectPath};

struct Devices;
impl LibinputInterface for Devices {
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        OpenOptions::new()
            .read(true)
            .write(flags & libc::O_ACCMODE != libc::O_RDONLY)
            .custom_flags(flags & !libc::O_ACCMODE)
            .open(path)
            .map(Into::into)
            .map_err(|error| error.raw_os_error().unwrap_or(libc::EIO))
    }
    fn close_restricted(&mut self, fd: OwnedFd) {
        drop(fd);
    }
}

fn packet(output: &mut impl Write, message: InputMessage) -> io::Result<()> {
    output.write_all(&message.encode())?;
    // Stdout is line-buffered; binary packets must never wait for a newline.
    output.flush()
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let uid: u32 = std::env::var("PKEXEC_UID")?.parse()?;
    // SAFETY: geteuid has no pointer arguments or side effects.
    if uid == 0 || unsafe { libc::geteuid() } != 0 {
        return Err("use pkexec from a local user session".into());
    }
    if std::env::args_os().skip(1).collect::<Vec<_>>() != ["--input-helper"] {
        return Err("input helper accepts only --input-helper".into());
    }
    let bus = zbus::blocking::connection::Builder::system()?
        .method_timeout(std::time::Duration::from_secs(1))
        .build()?;
    let manager = Proxy::new(
        &bus,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )?;
    let user_path: OwnedObjectPath = manager.call("GetUser", &(uid,))?;
    let user = Proxy::new(
        &bus,
        "org.freedesktop.login1",
        user_path,
        "org.freedesktop.login1.User",
    )?;
    let (_, session_path): (String, OwnedObjectPath) = user.get_property("Display")?;
    let session = Proxy::new(
        &bus,
        "org.freedesktop.login1",
        session_path,
        "org.freedesktop.login1.Session",
    )?;
    let (session_uid, _): (u32, OwnedObjectPath) = session.get_property("User")?;
    let (seat, _): (String, OwnedObjectPath) = session.get_property("Seat")?;
    if session_uid != uid || seat.is_empty() || session.get_property::<bool>("Remote")? {
        return Err("a local graphical session is required".into());
    }
    let mut input = Libinput::new_with_udev(Devices);
    input
        .udev_assign_seat(&seat)
        .map_err(|_| "cannot assign input seat")?;
    let mut output = io::stdout();
    // stdout is a private pipe owned by the unprivileged parent. A reader that
    // stops draining must never leave a privileged process blocked indefinitely.
    // SAFETY: fcntl only reads/sets flags of this process's valid stdout fd.
    unsafe {
        let flags = libc::fcntl(output.as_raw_fd(), libc::F_GETFL);
        if flags == -1
            || libc::fcntl(output.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) == -1
        {
            return Err(io::Error::last_os_error().into());
        }
    }
    output.write_all(HEADER)?;
    output.flush()?;
    let mut enabled = false;
    input.suspend();
    loop {
        let mut fds = [
            libc::pollfd {
                fd: 0,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: input.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: fds is an initialized array, valid for its full length.
        let result = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, 50) };
        if result < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e.into());
        }
        // Closing the parent's stdin pipe (including a crash) ends this owner.
        if fds[0].revents != 0 {
            break;
        }
        let active = session.get_property::<bool>("Active")?
            && !session.get_property::<bool>("LockedHint")?;
        if active != enabled {
            packet(&mut output, InputMessage::Reset { enabled: active })?;
            if active {
                input.resume().map_err(|_| "cannot resume input")?;
            } else {
                input.suspend();
            }
            enabled = active;
        }
        packet(&mut output, InputMessage::Heartbeat { enabled })?;
        input.dispatch()?;
        for event in &mut input {
            if !enabled {
                continue;
            }
            match event {
                Event::Keyboard(KeyboardEvent::Key(event)) => {
                    let down = event.key_state() == KeyState::Pressed;
                    if (down && event.seat_key_count() == 1)
                        || (!down && event.seat_key_count() == 0)
                    {
                        packet(
                            &mut output,
                            InputMessage::Key {
                                code: event.key(),
                                state: if down {
                                    PacketButtonState::Pressed
                                } else {
                                    PacketButtonState::Released
                                },
                            },
                        )?;
                    }
                }
                Event::Pointer(PointerEvent::Button(event)) => {
                    let down = event.button_state() == ButtonState::Pressed;
                    if (down && event.seat_button_count() == 1)
                        || (!down && event.seat_button_count() == 0)
                    {
                        packet(
                            &mut output,
                            InputMessage::Button {
                                code: event.button(),
                                state: if down {
                                    PacketButtonState::Pressed
                                } else {
                                    PacketButtonState::Released
                                },
                            },
                        )?;
                    }
                }
                Event::Pointer(PointerEvent::Motion(event)) => packet(
                    &mut output,
                    InputMessage::Motion {
                        dx: event.dx(),
                        dy: event.dy(),
                    },
                )?,
                Event::Device(input::event::DeviceEvent::Removed(_)) => {
                    packet(&mut output, InputMessage::Reset { enabled: true })?
                }
                _ => {}
            }
        }
    }
    input.suspend();
    Ok(())
}
