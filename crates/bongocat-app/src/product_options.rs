//! The command line the product accepts.
//!
//! One parser owns every run mode: an ordinary interactive run, the bounded
//! diagnostic runs the CI smoke steps use, and the storage-injection flag a
//! release build refuses. Adding a mode here is the only way to add one at all,
//! so the set stays visible in one place instead of spread across the flags that
//! read them.

use super::*;

/// The name `clap` puts in its own diagnostics and usage line.
pub(crate) const PROGRAM_NAME: &str = "bongocat-app";

/// How this process was asked to run.
///
/// Every flag is declared exactly once, here. The parser, the `--help` text and the
/// `#[cfg]`-gated availability of a harness all read this one declaration, so a flag
/// cannot be accepted but undocumented, documented but rejected, or advertised in help
/// for a build that does not accept it.
///
/// Most flags select a smoke or diagnostic harness that scripts and CI launch. Only
/// `--run-seconds` and the help flag are ever passed to a packaged build, the second one
/// by the login item.
#[derive(Clone, Debug, Eq, PartialEq, clap::Parser)]
#[command(
    name = "bongocat-app",
    about = "BongoCat",
    long_about = "The application runs until it is explicitly quit by default. A positive \
                  --run-seconds value enables a bounded diagnostic run."
)]
pub(crate) struct RunOptions {
    /// Run for this many seconds instead of until the product is quit.
    ///
    /// Zero is the default and means the same unbounded lifetime as passing nothing.
    #[arg(long, value_name = "SECONDS", default_value_t = 0)]
    pub(crate) run_seconds: u64,

    /// Paint the settings window and exit once the run guard fires.
    #[arg(long)]
    pub(crate) settings_window_smoke: bool,

    /// Open the settings window without instrumenting the run.
    #[arg(long)]
    pub(crate) settings_window_open_smoke: bool,

    /// Open the model library page.
    ///
    /// Implies `--settings-window-smoke`: there is no way to paint a page without the
    /// window smoke that owns the run guard.
    #[arg(long)]
    pub(crate) models_page_smoke: bool,

    /// Switch models without the overlay or the status icon being visible.
    #[arg(long)]
    pub(crate) hidden_model_switch_smoke: bool,

    /// Rewrite the stored window layout and report what it wrote.
    #[cfg(feature = "storage-test-injection")]
    #[arg(long)]
    pub(crate) settings_window_state_smoke: bool,

    /// Crash on purpose and report the diagnostics the panic produced.
    #[cfg(feature = "storage-test-injection")]
    #[arg(long)]
    pub(crate) panic_diagnostics_smoke: bool,

    /// The re-executed child of `--panic-diagnostics-smoke`.
    ///
    /// Hidden because it is spawned by its parent harness and never typed by a person;
    /// showing it would only invite someone to run half a diagnostic by hand.
    #[cfg(feature = "storage-test-injection")]
    #[arg(long, hide = true)]
    pub(crate) panic_diagnostics_smoke_child: bool,

    /// Write a diagnostics preview bundle and report where it landed.
    #[cfg(feature = "storage-test-injection")]
    #[arg(long)]
    pub(crate) diagnostics_export_smoke: bool,

    /// Fail the diagnostics export partway and report how that surfaces.
    #[cfg(feature = "storage-test-injection")]
    #[arg(long)]
    pub(crate) diagnostics_export_failure_smoke: bool,

    /// Paint the system menu and report the actions it offers.
    #[arg(long)]
    pub(crate) system_menu_smoke: bool,

    /// Report what the startup permission check would decide.
    #[arg(long)]
    pub(crate) startup_permission_smoke: bool,

    /// Report how the application answers a second launch.
    #[cfg(target_os = "macos")]
    #[arg(long)]
    pub(crate) application_reopen_smoke: bool,

    /// Report the login item state and leave it as it was found.
    #[cfg(target_os = "macos")]
    #[arg(long)]
    pub(crate) startup_item_smoke: bool,

    /// Report how a second launch is turned away.
    #[cfg(target_os = "windows")]
    #[arg(long)]
    pub(crate) single_instance_smoke: bool,

    /// The file the primary instance writes once it is ready to be notified.
    ///
    /// Hidden for the same reason as `--panic-diagnostics-smoke-child`: CI plumbing
    /// that only means anything next to `--single-instance-smoke`.
    #[cfg(target_os = "windows")]
    #[arg(
        long,
        value_name = "PATH",
        hide = true,
        requires = "single_instance_smoke",
        value_parser = non_empty_path
    )]
    pub(crate) single_instance_ready_file: Option<PathBuf>,

    /// The file a secondary instance writes to report what the primary did.
    #[cfg(target_os = "windows")]
    #[arg(
        long,
        value_name = "PATH",
        hide = true,
        requires = "single_instance_smoke",
        value_parser = non_empty_path
    )]
    pub(crate) single_instance_result_file: Option<PathBuf>,
}

/// A path argument that rejects an empty value.
///
/// `clap` accepts `--flag ""` as a present value, and an empty marker file path would
/// name the process's working directory rather than nothing at all.
#[cfg(target_os = "windows")]
pub(crate) fn non_empty_path(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() {
        return Err("a non-empty file path is required".to_owned());
    }
    Ok(PathBuf::from(value))
}

impl RunOptions {
    pub(crate) fn parse(
        arguments: impl IntoIterator<Item = String>,
    ) -> Result<Self, RunOptionsError> {
        // `clap` reads the first element as the binary name, so the caller's arguments —
        // which already skip `argv[0]` — are prefixed with a fixed one rather than with
        // whatever the process was launched as. The name only appears in diagnostics.
        let arguments = std::iter::once(PROGRAM_NAME.to_owned()).chain(arguments);
        let mut options = <Self as clap::Parser>::try_parse_from(arguments)?;
        // `--models-page-smoke` names a page, and painting a page needs the window smoke
        // that owns the run guard. Deriving it keeps the two flags from being able to
        // disagree at runtime.
        if options.models_page_smoke {
            options.settings_window_smoke = true;
        }
        Ok(options)
    }

    /// How long the run is bounded for, or [`Duration::ZERO`] for an unbounded one.
    pub(crate) fn run_duration(&self) -> Duration {
        Duration::from_secs(self.run_seconds)
    }

    /// Whether this run is a harness rather than a product start.
    ///
    /// Every accepted argument except `--run-seconds` selects a harness, so this is true
    /// exactly when one of the harness flags is present. The startup permission prompt
    /// hangs on it: a harness is launched by a script on a machine where nobody can
    /// answer a native dialog.
    pub(crate) fn automated_verification(&self) -> bool {
        self.settings_window_smoke
            || self.settings_window_open_smoke
            || self.models_page_smoke
            || self.hidden_model_switch_smoke
            || self.system_menu_smoke
            || self.startup_permission_smoke
            || self.single_instance_arguments_present()
            || self.storage_test_injection_arguments_present()
    }

    /// Whether a single-instance flag or marker file was named.
    #[cfg(target_os = "windows")]
    pub(crate) fn single_instance_arguments_present(&self) -> bool {
        self.single_instance_smoke
            || self.single_instance_ready_file.is_some()
            || self.single_instance_result_file.is_some()
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn single_instance_arguments_present(&self) -> bool {
        false
    }

    /// Whether a storage-test-injection harness was named.
    #[cfg(feature = "storage-test-injection")]
    pub(crate) fn storage_test_injection_arguments_present(&self) -> bool {
        self.settings_window_state_smoke
            || self.panic_diagnostics_smoke
            || self.panic_diagnostics_smoke_child
            || self.diagnostics_export_smoke
            || self.diagnostics_export_failure_smoke
    }

    #[cfg(not(feature = "storage-test-injection"))]
    pub(crate) fn storage_test_injection_arguments_present(&self) -> bool {
        false
    }

    pub(crate) fn opens_settings_window_on_start(&self) -> bool {
        let mut opens_settings_window =
            self.settings_window_smoke || self.settings_window_open_smoke;
        #[cfg(target_os = "macos")]
        {
            opens_settings_window |= self.application_reopen_smoke;
        }
        #[cfg(target_os = "windows")]
        {
            opens_settings_window |= self.single_instance_smoke;
        }
        opens_settings_window
    }
}

#[derive(Eq, PartialEq, thiserror::Error)]
#[error("{message}")]
pub(crate) struct RunOptionsError {
    /// The rendered `clap` diagnostic: the full help for `--help`, and the offending
    /// argument plus the usage line for anything else.
    pub(crate) message: String,
    /// Whether this was a request for help rather than a bad command line.
    pub(crate) help: bool,
}

/// `Debug` is the rendered message rather than the derived struct form.
///
/// `main` hands this back as a boxed error, and `Result`'s `Termination` prints the
/// `Debug` form — so the derived one would wrap a multi-line diagnostic in the struct's
/// braces and escape its newlines. These flags exist to be read by a person or a CI log
/// fixing a command line, so what gets printed is what a reader can act on.
impl std::fmt::Debug for RunOptionsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl From<clap::Error> for RunOptionsError {
    fn from(error: clap::Error) -> Self {
        Self {
            help: matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ),
            message: error.render().to_string(),
        }
    }
}

/// The command line as `--help` prints it.
///
/// The flags come from the one declaration above, so this is also what a test reads to
/// confirm that a harness this build does not contain is absent from the help text too.
#[cfg(test)]
pub(crate) fn usage() -> String {
    <RunOptions as clap::CommandFactory>::command()
        .render_help()
        .to_string()
}
