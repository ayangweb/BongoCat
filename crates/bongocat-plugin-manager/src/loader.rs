//! One owned process with bounded request/response exchanges.
use bongocat_plugin_sdk::{
    HostMessage, PROTOCOL_VERSION, PluginMetadata, PluginOutput, Request, Response, read_frame,
    write_frame,
};
use std::{
    io,
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(crate) struct Process {
    child: Child,
    input: Option<ChildStdin>,
    responses: Receiver<io::Result<Response>>,
    reader: Option<JoinHandle<()>>,
    sequence: u64,
}
impl Process {
    pub(crate) fn start(root: &Path, metadata: &PluginMetadata) -> io::Result<Self> {
        let mut command = Command::new(entry_path(root, &metadata.entry));
        command
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        /// The executable for this platform. A manifest names the base name; Windows
        /// packages append `.exe`, so the same manifest installs on both platforms.
        fn entry_path(root: &Path, entry: &str) -> std::path::PathBuf {
            let path = root.join(entry);
            #[cfg(target_os = "windows")]
            if !path.is_file() {
                let with_extension = root.join(format!("{entry}.exe"));
                if with_extension.is_file() {
                    return with_extension;
                }
            }
            path
        }

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn()?;
        let input = child.stdin.take();
        let mut output = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("plugin stdout unavailable"))?;
        let (sender, responses) = mpsc::sync_channel(2);
        let reader = thread::spawn(move || {
            loop {
                let response = read_frame(&mut output);
                let failed = response.is_err();
                // Unsolicited output cannot block process cleanup or grow memory.
                if sender.try_send(response).is_err() || failed {
                    break;
                }
            }
        });
        let mut process = Self {
            child,
            input,
            responses,
            reader: Some(reader),
            sequence: 0,
        };
        process.exchange(HostMessage::Handshake {
            protocol_version: PROTOCOL_VERSION,
            id: metadata.id.clone(),
        })?;
        Ok(process)
    }
    pub(crate) fn exchange(&mut self, message: HostMessage) -> io::Result<PluginOutput> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| io::Error::other("sequence exhausted"))?;
        // Writing is also bounded: a watchdog kills a non-reading child, unblocking
        // the pipe. The watchdog is joined before returning from this request.
        let input = self
            .input
            .take()
            .ok_or_else(|| io::Error::other("plugin stopped"))?;
        let request = Request {
            sequence: self.sequence,
            message,
        };
        let writer = thread::spawn(move || {
            let mut input = input;
            let result = write_frame(&mut input, &request);
            (input, result)
        });
        let deadline = Instant::now() + Duration::from_millis(750);
        while !writer.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        if !writer.is_finished() {
            let _ = self.child.kill();
        }
        let (input, result) = writer
            .join()
            .map_err(|_| io::Error::other("plugin writer failed"))?;
        self.input = Some(input);
        result?;
        let response = self
            .responses
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "plugin response deadline"))??;
        if response.sequence != self.sequence {
            return Err(io::Error::other("plugin response sequence mismatch"));
        }
        let output = response.result.map_err(io::Error::other)?;
        if !output.validate() {
            return Err(io::Error::other("plugin output exceeds limits"));
        }
        Ok(output)
    }
    pub(crate) fn stop(&mut self) -> io::Result<PluginOutput> {
        let output = self.exchange(HostMessage::Stop);
        self.input.take();
        let deadline = Instant::now() + Duration::from_millis(750);
        while self.child.try_wait()?.is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        if self.child.try_wait()?.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "plugin shutdown deadline",
            ));
        }
        output
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
