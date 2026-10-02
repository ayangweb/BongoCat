use super::lifecycle::rounded_u32;
use super::*;
use bongocat_platform::ExternalUrlOpenError;
use gpui_kit::component::Sizable as _;

/// The project links are product-owned constants rather than values received
/// from a snapshot. They are passed through the platform adapter's HTTPS-only
/// URL validator before the operating system is asked to open them.
pub(super) const PROJECT_SOURCE_URL: &str = "https://github.com/ayangweb/BongoCat";
pub(super) const FEEDBACK_URL: &str = "https://github.com/ayangweb/BongoCat/issues/new/choose";

/// Every user-visible string on the About page belongs to this small contract.
/// Keeping it beside the page assembly makes the settings smoke check cover
/// the same rows that users can actually reach.
pub(super) const ABOUT_LOCALIZED_KEYS: [&str; 13] = [
    "about.product_information.version.title",
    "about.software_information.title",
    "about.software_information.description",
    "about.software_information.copy",
    "about.software_information.copy_success",
    "about.project.title",
    "about.project.action",
    "about.feedback.title",
    "about.feedback.description",
    "about.feedback.action",
    "about.logs.title",
    "about.logs.action",
    "update.about.label",
];

/// The product identity the report names, and the About page's own row title.
const PRODUCT_NAME: &str = "BongoCat";

/// The bug-report document `copy_software_info` puts on the clipboard.
///
/// A JSON object rather than prose, because the reader of a report is a person
/// triaging many of them: named, stable keys can be looked up, diffed between
/// two reports and quoted in an answer, where a line of translated prose has to
/// be read and translated back first. Field order is the order a human scans
/// them in — what was built, what it runs on, then what state it is in — and
/// `serde` preserves it.
///
/// Every value here is a fact about the build or the running process. There is
/// no storage path, no model title, no log line, no shortcut, and no configured
/// value anywhere in this document, so copying it for a report cannot disclose
/// anything about the machine it was copied from. The platform fields are the
/// closest the document comes to machine-specific data: three of them are
/// compile-time constants of the process itself, and the fourth is what the
/// kernel reports about the system it is running on.
///
/// `platform_version` and `platform_build` are `None` on a system that cannot
/// name its own version. The field is left out of the document rather than
/// filled with a placeholder, because a value a maintainer has to learn to
/// distrust is worse than a field they can see is missing.
#[derive(serde::Serialize)]
pub(super) struct SoftwareInformation {
    app_name: &'static str,
    app_version: String,
    build_environment: &'static str,
    /// Which Cubism Core this binary is linked against, because a model's
    /// compatibility is a property of this and not of the app version.
    cubism_core_version: String,
    platform: &'static str,
    platform_arch: &'static str,
    platform_version: Option<String>,
    platform_build: Option<String>,
    /// The language the window is actually showing, as a locale code. A report
    /// about a label or a layout has to be read in the language it was seen in.
    locale: &'static str,
    runtime_health: &'static str,
    /// The last render error, when the runtime has one. The single most useful
    /// field in the document for a model that renders black or not at all.
    runtime_error_code: Option<&'static str>,
    input_service_status: &'static str,
    /// What this platform gates global input behind, by the platform's own name:
    /// `input_monitoring` on macOS and `administrator` on Windows.
    ///
    /// Naming it rather than assuming one shape is what lets a Windows report
    /// answer the question that matters there — is the process elevated — and a
    /// macOS one answer whether the TCC grant was given. A single
    /// granted/denied/unsupported field could only ever describe the second
    /// (ADR-0032).
    input_capability: &'static str,
    /// Whether this process has that capability right now. On Windows this is
    /// the elevation state, which is what decides whether raw input keeps
    /// arriving while a higher-integrity window has focus.
    input_capability_available: bool,
    connected_gamepad_count: usize,
    /// How many times the input service has had to clear a pressed key or button
    /// without a matching release edge from the platform: a state reconcile, a
    /// release attributed to a reset, and a release with no press before it.
    ///
    /// Zero is the healthy answer for all three, and a non-zero one is the
    /// evidence behind "a key stays stuck" (issue #47) that a report can be
    /// acted on. They are summed rather than listed because the question a
    /// triage asks is whether any of them ever happened, and one number answers
    /// it; the full split is in the diagnostics export.
    input_release_reconciliations: u64,
    /// Where the active model came from: `preset` or `installed`, and absent
    /// when no model is active. The id is deliberately not here — an imported
    /// model's id is derived from the user's own file.
    active_model_origin: Option<&'static str>,
    /// How many imported models are ready, and how many the store rejected.
    ///
    /// A count is the whole answer to "my model is not showing up": zero ready
    /// means it never loaded, a non-zero invalid count means the store refused
    /// it and the log carries which diagnostic said why. The diagnostics export
    /// has the same counts with the per-code breakdown.
    ready_installed_model_count: usize,
    invalid_model_count: usize,
    /// The display scale the window is rendering at: 1.0, 1.25, 1.5, 2.0.
    ///
    /// Layout reports are the ones a maintainer cannot reproduce from a version
    /// and an OS build, because the same window is correct at one scale and
    /// clipped at another. The 800x600 layout floor in the acceptance criteria
    /// makes the size below the other half of that pair.
    ui_scale_factor: f32,
    /// The settings window's size in logical pixels, as the user left it.
    ///
    /// Logical, not physical: multiplied by [`Self::ui_scale_factor`] it is the
    /// pixel count the compositor works with, and reporting it that way keeps
    /// the two fields consistent on a Retina or scaled display.
    ui_window_size: (u32, u32),
}

/// The window's own presentation facts, for a layout report.
///
/// The scale factor and the bounds exist only on a `Window`, so they are read
/// where the button hands one over rather than being carried through the view.
#[derive(Clone, Copy, Debug)]
pub(super) struct WindowFacts {
    scale_factor: f32,
    width: u32,
    height: u32,
}

impl WindowFacts {
    /// A window at a known scale and size.
    ///
    /// A unit test cannot have a real `Window`, and stating one through this
    /// constructor keeps its fixture in exactly the shape [`Self::read`]
    /// produces.
    pub(super) const fn new(scale_factor: f32, width: u32, height: u32) -> Self {
        Self {
            scale_factor,
            width,
            height,
        }
    }

    fn read(window: &Window) -> Self {
        // A fullscreen window has no size the user chose, so the viewport it is
        // filling is what a layout report is about. A window that has not been
        // laid out yet measures as zero, which is the same "no size to report"
        // answer the window placement code gives.
        let size = window.viewport_size();
        Self::new(
            window.scale_factor(),
            rounded_u32(size.width).unwrap_or_default(),
            rounded_u32(size.height).unwrap_or_default(),
        )
    }
}

/// The imported models the store can use, and the ones it refused.
///
/// A preset is never counted here: it ships with the product, so a user who
/// reports a missing model is always talking about one they added, and the
/// preset's presence is not a variable.
fn model_counts(snapshot: &SettingsSnapshot) -> (usize, usize) {
    snapshot
        .model_catalog
        .entries
        .iter()
        .filter(|entry| entry.origin == SettingsModelOrigin::Imported)
        .fold((0, 0), |(ready, invalid), entry| {
            match &entry.availability {
                SettingsModelAvailability::Ready { .. } => (ready + 1, invalid),
                SettingsModelAvailability::Invalid { .. } => (ready, invalid + 1),
            }
        })
}

impl SoftwareInformation {
    /// Read the current state of this process.
    ///
    /// `snapshot` carries everything the services know; the operating system
    /// version is read through the platform adapter and the window facts come
    /// from the window itself, both at the moment the report is built, so
    /// neither can go stale the way a value in a revisioned snapshot would.
    pub(super) fn read(snapshot: &SettingsSnapshot, window: WindowFacts) -> Self {
        let system = bongocat_platform::operating_system_version();
        let input = &snapshot.input_diagnostics;
        let (ready_installed, invalid) = model_counts(snapshot);
        Self {
            app_name: PRODUCT_NAME,
            app_version: snapshot.build_info.product_version.clone(),
            build_environment: snapshot.build_info.environment.code(),
            cubism_core_version: snapshot.build_info.cubism_core_version.clone(),
            platform: std::env::consts::OS,
            platform_arch: std::env::consts::ARCH,
            platform_version: system.as_ref().map(|system| system.version.clone()),
            platform_build: system.as_ref().map(|system| system.build.clone()),
            locale: snapshot.resolved_language.code(),
            runtime_health: snapshot.runtime_health.as_str(),
            runtime_error_code: snapshot
                .runtime_diagnostics
                .render_error
                .map(SettingsRuntimeErrorCode::as_str),
            input_service_status: input.service_status.as_str(),
            input_capability: input.input_capability.name,
            input_capability_available: input.input_capability.available,
            connected_gamepad_count: input.connected_gamepad_count,
            input_release_reconciliations: input
                .reconciled_release
                .saturating_add(input.released_by_reset)
                .saturating_add(input.unmatched_release),
            active_model_origin: snapshot
                .active_model
                .as_ref()
                .map(|model| match model.origin {
                    SettingsModelOrigin::BuiltIn => "preset",
                    SettingsModelOrigin::Imported => "installed",
                }),
            ready_installed_model_count: ready_installed,
            invalid_model_count: invalid,
            ui_scale_factor: window.scale_factor,
            ui_window_size: (window.width, window.height),
        }
    }

    /// Render the document for the clipboard.
    ///
    /// Pretty-printed, because the reader is a person opening an issue and this
    /// text is pasted into a code block. `None` when serialization fails, which
    /// for a document of strings and integers means a formatting fault rather
    /// than a missing value, and is reported as the same copy failure as a
    /// clipboard that would not accept the text.
    pub(super) fn to_json(&self) -> Option<String> {
        serde_json::to_string_pretty(self).ok()
    }
}

fn product_info_description(snapshot: Option<&SettingsSnapshot>) -> String {
    let language = snapshot.map_or(SettingsLanguage::English, |snapshot| {
        snapshot.resolved_language
    });
    snapshot.map_or_else(
        || {
            bongocat_i18n::text(
                language.catalog_locale(),
                "about.product_information.version.title",
            )
            .to_owned()
        },
        |snapshot| build_info_detail(language, &snapshot.build_info),
    )
}

/// The operational rows of the About page.
///
/// About is rendered through the same `SettingPage`/`SettingGroup` contract as
/// every other settings destination. The buttons are deliberately one-shot
/// actions: this page does not invent a second persisted preference for
/// information that is already owned by the application and its services.
///
/// The page shows one button per row, so five same-coloured buttons in a
/// column are told apart by their labels alone. The page is a quiet one, so it
/// uses three tiers instead of a colour per row: checking for updates is the
/// single filled `primary` action, reporting an issue is the one row that wants
/// an answer and carries `danger`, and the three rows that only read or open
/// something stay on the library's plain default button. A colour that means
/// nothing on its row is noise, and every colour that is used comes from the
/// active theme rather than a hardcoded value, which needs no branch for a
/// light/dark switch:
///
/// | row | variant | reads as |
/// | --- | --- | --- |
/// | checking for updates | `primary` | the one action that changes the app |
/// | copy software information | `Default` | a read-only action |
/// | open the project home | `Default` | a read-only action |
/// | report an issue | `danger` + outline | the row that wants an answer |
/// | open the log folder | `Default` | a read-only action |
///
/// `danger` is borrowed here as "this one wants an answer", not as a claim that
/// the action is destructive; the window's own destructive surfaces keep their
/// own `danger` controls.
///
/// A row carries a description only when it has something the title and the
/// button cannot say: the build identity under the product row, what gets
/// copied under "Software information", and what to copy first under "Problem
/// feedback". The project row shows the URL and the log row shows nothing,
/// because a sentence beside an open-the-link button repeats it.
///
/// The two rows that open a location label their button with the bare verb
/// ("Open"), because the row title beside it already names what is opened and a
/// longer label only widens the button.
pub(super) fn operational_group(
    view: Entity<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    language: SettingsLanguage,
    keywords: Vec<SharedString>,
    request_update: SettingsWindowRequest,
) -> SettingGroup {
    let locale = language.catalog_locale();
    let product_description = product_info_description(snapshot);

    let update_view = view.clone();
    let update_request = request_update;
    let product = SettingItem::new(
        PRODUCT_NAME,
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                let available = update_view.read(app).snapshot.is_some();
                let request = update_request.clone();
                Button::new("about-check-for-updates-button")
                    .label(bongocat_i18n::text(locale, "update.about.label"))
                    .with_size(options.size())
                    .primary()
                    .disabled(!available)
                    .on_click(move |_, _, app| (request)(app))
            },
        ),
    )
    .description(product_description)
    .keywords(keywords.clone());

    let software_view = view.clone();
    let software = SettingItem::new(
        bongocat_i18n::text(locale, "about.software_information.title"),
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                let available = software_view.read(app).snapshot.is_some();
                let action_view = software_view.clone();
                Button::new("about-copy-software-info-button")
                    .label(bongocat_i18n::text(
                        locale,
                        "about.software_information.copy",
                    ))
                    .with_size(options.size())
                    .with_variant(ButtonVariant::Default)
                    .disabled(!available)
                    .on_click(move |_, window, app| {
                        // The layout facts only exist on the window, so they are
                        // taken here rather than threaded through the view.
                        let window = WindowFacts::read(window);
                        action_view.update(app, |view, cx| view.copy_software_info(window, cx));
                    })
            },
        ),
    )
    .description(bongocat_i18n::text(
        locale,
        "about.software_information.description",
    ))
    .keywords(keywords.clone());

    let project_view = view.clone();
    // The row's description is the link itself: a second sentence about the
    // source, releases or progress repeats what the button and the URL already
    // say, and the URL is the part a reader may want to select or type.
    let project = SettingItem::new(
        bongocat_i18n::text(locale, "about.project.title"),
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                let available = project_view.read(app).snapshot.is_some();
                let action_view = project_view.clone();
                Button::new("about-open-project-source-button")
                    .label(bongocat_i18n::text(locale, "about.project.action"))
                    .with_size(options.size())
                    .with_variant(ButtonVariant::Default)
                    .disabled(!available)
                    .on_click(move |_, _, app| {
                        action_view.update(app, |view, cx| view.open_project_source(cx));
                    })
            },
        ),
    )
    .description(PROJECT_SOURCE_URL.to_owned())
    .keywords(keywords.clone());

    let feedback_view = view.clone();
    let feedback = SettingItem::new(
        bongocat_i18n::text(locale, "about.feedback.title"),
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                let available = feedback_view.read(app).snapshot.is_some();
                let action_view = feedback_view.clone();
                Button::new("about-open-feedback-button")
                    .label(bongocat_i18n::text(locale, "about.feedback.action"))
                    .with_size(options.size())
                    .danger()
                    .outline()
                    .disabled(!available)
                    .on_click(move |_, _, app| {
                        action_view.update(app, |view, cx| view.open_feedback(cx));
                    })
            },
        ),
    )
    .description(bongocat_i18n::text(locale, "about.feedback.description"))
    .keywords(keywords.clone());

    let logs_view = view.clone();
    // No description: this row opens the log folder rather than explaining it,
    // and the size, level and retention limits are already settings of their own
    // under "App & system > Logging".
    let logs = SettingItem::new(
        bongocat_i18n::text(locale, "about.logs.title"),
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                let available = logs_view.read(app).snapshot.is_some();
                let action_view = logs_view.clone();
                Button::new("about-open-logs-button")
                    .label(bongocat_i18n::text(locale, "about.logs.action"))
                    .with_size(options.size())
                    .with_variant(ButtonVariant::Default)
                    .disabled(!available)
                    .on_click(move |_, _, app| {
                        action_view.update(app, |view, cx| view.open_logs_location(cx));
                    })
            },
        ),
    )
    .keywords(keywords);

    SettingGroup::new().items([product, software, project, feedback, logs])
}

impl SettingsView {
    /// Copy the privacy-safe [`SoftwareInformation`] report a bug reporter needs.
    ///
    /// The document is built here, at the moment the user asks for it, rather
    /// than held in the snapshot: the operating system version is a question to
    /// the machine this runs on, and a revisioned snapshot is the wrong place to
    /// keep an answer to it.
    ///
    /// Clipboard access is a main-thread platform capability. This method is
    /// called from the GPUI button callback, never from the settings worker, so
    /// the macOS AppKit invariant is preserved by construction. A report that
    /// cannot be built and a clipboard that will not accept it are the same
    /// failure to the user — nothing was copied — so they report one code.
    pub(super) fn copy_software_info(&mut self, window: WindowFacts, cx: &mut Context<Self>) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let copied = SoftwareInformation::read(snapshot, window)
            .to_json()
            .is_some_and(|json| bongocat_platform::write_clipboard_text(&json).is_ok());
        if copied {
            self.about_copy_success_pending = true;
        } else {
            self.pending_notification = Some(SettingsError::new(
                SettingsErrorCode::SoftwareInfoCopyFailed,
            ));
        }
        cx.notify();
    }

    /// Hand an external link to the operating system, and tell the user when it
    /// could not be opened.
    ///
    /// The launch is scheduled, not made here, and that is the whole fix. This
    /// callback is running inside GPUI's mutable borrow of its `App`, and
    /// `ShellExecuteW` pumps this process's message queue on the way out. The
    /// pump re-enters GPUI's foreground tasks, which ask the same `RefCell` for
    /// another mutable borrow; `RefCell` grants one, so the second one panicked
    /// and killed the process. Doing it on a background thread means no pump
    /// happens under the borrow, and the window also keeps painting while the
    /// browser starts. `external_link` documents the mechanism.
    fn open_external_link(&mut self, url: &'static str, cx: &mut Context<Self>) {
        self.open_external_link_with(url, bongocat_platform::open_external_url, cx);
    }

    /// The one implementation both rows go through, with the launch injected so
    /// the scheduling can be asserted without a test run opening a browser.
    pub(super) fn open_external_link_with(
        &mut self,
        url: &'static str,
        launch: impl FnOnce(&str) -> Result<(), ExternalUrlOpenError> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let opened =
                crate::external_link::open_with(url.to_owned(), cx.background_executor(), launch)
                    .await
                    .is_ok();
            let _ = this.update(cx, |view, cx| {
                if !opened {
                    view.pending_notification = Some(SettingsError::new(
                        SettingsErrorCode::ExternalLinkOpenFailed,
                    ));
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn open_project_source(&mut self, cx: &mut Context<Self>) {
        self.open_external_link(PROJECT_SOURCE_URL, cx);
    }

    pub(super) fn open_feedback(&mut self, cx: &mut Context<Self>) {
        self.open_external_link(FEEDBACK_URL, cx);
    }

    /// Ask the settings worker to open the application-owned log directory.
    ///
    /// The path is deliberately not put in `SettingsSnapshot`: it is a local
    /// implementation detail, and the service can validate and open it without
    /// making the UI carry a filesystem capability.
    pub(super) fn open_logs_location(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.snapshot.is_none() {
            return;
        }
        self.pending = Some(PendingOperation::OpenLogsLocation);
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.open_logs_location().await;
            let _ = this.update(cx, |view, cx| {
                view.pending = None;
                match result {
                    Ok(snapshot)
                        if accepts_snapshot_revision(
                            view.snapshot.as_ref().map(|current| current.revision),
                            snapshot.revision,
                        ) =>
                    {
                        view.snapshot = Some(snapshot);
                    }
                    Ok(_) => {}
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
