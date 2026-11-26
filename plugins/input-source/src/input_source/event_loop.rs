//! The main thread waits for either protocol input or a distributed notification.
use crate::InputSourceIndicator;
use bongocat_plugin_sdk::{
    HostMessage, PROTOCOL_VERSION, Plugin, PluginOutput, Request, Response, read_frame, write_frame,
};
use objc2_core_foundation::{
    CFRunLoop, CFRunLoopSource, CFRunLoopSourceContext, kCFRunLoopDefaultMode,
};
use std::{ffi::c_void, io, ptr, sync::mpsc, thread};

unsafe extern "C-unwind" fn perform(_: *mut c_void) {}

pub fn run(plugin: &mut InputSourceIndicator) -> io::Result<()> {
    let run_loop = CFRunLoop::current().ok_or_else(|| io::Error::other("missing main run loop"))?;
    let mut context = CFRunLoopSourceContext {
        version: 0,
        info: ptr::null_mut(),
        retain: None,
        release: None,
        copyDescription: None,
        equal: None,
        hash: None,
        schedule: None,
        cancel: None,
        perform: Some(perform),
    };
    // SAFETY: context has no borrowed callback state and CF copies its fields.
    let source = unsafe { CFRunLoopSource::new(None, 0, &mut context) }
        .ok_or_else(|| io::Error::other("missing protocol run loop source"))?;
    // SAFETY: Apple's immortal default-mode constant is valid for registration.
    unsafe {
        run_loop.add_source(Some(&source), kCFRunLoopDefaultMode);
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    let wake_loop = (&*run_loop as *const CFRunLoop) as usize;
    // CFRunLoopSourceSignal is explicitly thread-safe. The main owner retains
    // the source until the reader ends; only this documented call uses its address.
    let wake_source = (&*source as *const CFRunLoopSource) as usize;
    let reader = thread::spawn(move || {
        let mut input = io::stdin().lock();
        loop {
            let request = read_frame::<Request>(&mut input);
            let failed = request.is_err()
                || request
                    .as_ref()
                    .is_ok_and(|request| matches!(request.message, HostMessage::Stop));
            if sender.send(request).is_err() {
                break;
            }
            // SAFETY: main retains the source for the reader lifetime. Signal
            // is thread-safe and does not access thread-confined callback state.
            unsafe {
                (&*(wake_source as *const CFRunLoopSource)).signal();
            }
            // SAFETY: WakeUp is thread-safe; the main owner retains this loop.
            unsafe {
                (&*(wake_loop as *const CFRunLoop)).wake_up();
            }
            if failed {
                break;
            }
        }
    });
    let mut ready = false;
    let mut output = io::stdout().lock();
    let result = (|| {
        loop {
            while let Ok(request) = receiver.try_recv() {
                let request = request?;
                let stop = matches!(request.message, HostMessage::Stop);
                let result = match request.message {
                    HostMessage::Handshake {
                        protocol_version,
                        ref id,
                    } => {
                        if ready || protocol_version != PROTOCOL_VERSION || id != plugin.id() {
                            Err("incompatible plugin handshake".into())
                        } else {
                            ready = true;
                            Ok(PluginOutput::default())
                        }
                    }
                    message if ready => plugin.handle(message),
                    _ => Err("handshake required".into()),
                };
                write_frame(
                    &mut output,
                    &Response {
                        sequence: request.sequence,
                        result,
                    },
                )?;
                if stop {
                    return Ok(());
                }
            }
            if ready
                && !plugin.paused
                && plugin
                    .monitor
                    .as_ref()
                    .is_some_and(crate::input_source::InputSourceMonitor::changed)
            {
                plugin.read_source();
                write_frame(
                    &mut output,
                    &Response {
                        sequence: 0,
                        result: Ok(plugin.output()),
                    },
                )?;
            }
            // SAFETY: the registered protocol source keeps this wait alive even
            // while observation is paused. No periodic source query is scheduled.
            unsafe {
                CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, f64::MAX, true);
            }
        }
    })();
    // SAFETY: remove the live source on its owning main thread before returning.
    unsafe {
        run_loop.remove_source(Some(&source), kCFRunLoopDefaultMode);
    }
    // Stop and EOF both end the reader without leaving a blocked worker.
    if result.is_ok() {
        let _ = reader.join();
    } else {
        // The process exits on protocol failure; retain the source for any
        // final signal from the blocked stdin reader.
        std::mem::forget(source);
        std::mem::forget(run_loop);
    }
    result
}
