//! Deciding when the Windows taskbar icon has settled.
//!
//! The `show_taskbar_icon` preference shows up in three places at once: the
//! configuration the runtime publishes as a settings snapshot, the model
//! window's own taskbar button, and the settings window's. A settings write
//! reaches the product in three steps — the native surface first, then the
//! configuration, then the republished snapshot — so between a write and the
//! snapshot that carries it, the published preference and what the windows show
//! are *expected* to differ.
//!
//! That is why the system menu smoke polls for the settled state instead of
//! sampling it once. Sampling is what made the startup assertion fail on a
//! Windows runner, where the shell had not yet caught up with the window styles
//! the product had already applied. The budget below is what keeps that a
//! settling delay rather than an open-ended wait, and a surface that never
//! settles inside it is still a failure — a genuinely hidden button stays
//! hidden, it just takes the shell a moment to say so.

use super::*;

/// How many times a taskbar check looks before it gives up.
pub(crate) const TASKBAR_ICON_SETTLE_ATTEMPTS: u32 = 100;

/// The gap between those looks, so the budget above is a little over a second.
pub(crate) const TASKBAR_ICON_SETTLE_INTERVAL: Duration = Duration::from_millis(10);

/// One observation of everywhere the `show_taskbar_icon` preference shows up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TaskbarIconSample {
    /// What the runtime's published settings snapshot carries.
    pub(crate) published: bool,
    /// What the shell currently shows for the model window's taskbar button.
    pub(crate) model_window: bool,
    /// What the shell currently shows for the settings window's taskbar button.
    pub(crate) settings_window: bool,
    /// Whether the model window itself is on screen at all.
    pub(crate) model_window_visible: bool,
}

impl TaskbarIconSample {
    /// Records one observation, in the order the three surfaces are read.
    pub(crate) fn new(
        published: bool,
        model_window: bool,
        settings_window: bool,
        model_window_visible: bool,
    ) -> Self {
        Self {
            published,
            model_window,
            settings_window,
            model_window_visible,
        }
    }
}

/// Which surface has not caught up yet, or that is genuinely wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskbarIconSettleGap {
    /// The settings window no longer has the taskbar button GPUI gave it.
    ///
    /// Hiding that button with `WS_EX_TOOLWINDOW` also replaces the title bar
    /// with the tool window's short caption, so a run where it never comes back
    /// is the regression this read exists to catch.
    SettingsWindowButtonLost,
    /// The model window is hidden, so its button cannot be judged at all.
    ModelWindowHidden,
    /// The runtime still publishes the preference this check is waiting on.
    PublishedPreferenceStale,
    /// The model window still shows the button for the previous preference.
    ModelWindowButtonStale,
}

impl TaskbarIconSettleGap {
    /// The smoke's failure text for this gap.
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::SettingsWindowButtonLost => {
                "the settings window lost the taskbar button GPUI created it with"
            }
            Self::ModelWindowHidden => "the model window is hidden",
            Self::PublishedPreferenceStale => {
                "the runtime still publishes the previous taskbar icon preference"
            }
            Self::ModelWindowButtonStale => {
                "the model window still shows the previous taskbar button"
            }
        }
    }
}

/// What has not caught up in `sample`, or `None` once it has.
///
/// The two window reads are live shell reads rather than product records, so a
/// gap in either can be the shell lagging behind a style the product has already
/// applied. That is why no gap here is treated as final: the caller gives the
/// surface the settle budget and reports the gap that outlasted it.
///
/// The window shape is still judged before the preference, so a run whose
/// settings button is missing is never reported as a stale snapshot.
pub(crate) fn taskbar_icon_settle_gap(
    sample: &TaskbarIconSample,
    expected: bool,
) -> Option<TaskbarIconSettleGap> {
    if !sample.settings_window {
        return Some(TaskbarIconSettleGap::SettingsWindowButtonLost);
    }
    if !sample.model_window_visible {
        return Some(TaskbarIconSettleGap::ModelWindowHidden);
    }
    if sample.published != expected {
        return Some(TaskbarIconSettleGap::PublishedPreferenceStale);
    }
    if sample.model_window != expected {
        return Some(TaskbarIconSettleGap::ModelWindowButtonStale);
    }
    None
}
