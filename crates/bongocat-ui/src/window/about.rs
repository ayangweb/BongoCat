use super::*;
use gpui_kit::component::Sizable as _;

/// The project links are product-owned constants rather than values received
/// from a snapshot. They are passed through the platform adapter's HTTPS-only
/// URL validator before the operating system is asked to open them.
pub(super) const PROJECT_SOURCE_URL: &str = "https://github.com/ayangweb/BongoCat";
pub(super) const FEEDBACK_URL: &str = "https://github.com/ayangweb/BongoCat/issues/new/choose";

/// Every user-visible string on the About page belongs to this small contract.
/// Keeping it beside the page assembly makes the settings smoke check cover
/// the same rows that users can actually reach.
pub(super) const ABOUT_LOCALIZED_KEYS: [&str; 17] = [
    "about.product_information.version.title",
    "about.software_information.title",
    "about.software_information.description",
    "about.software_information.copy",
    "about.software_information.copy_success",
    "about.software_information.platform",
    "about.software_information.architecture",
    "about.project.title",
    "about.project.description",
    "about.project.action",
    "about.feedback.title",
    "about.feedback.description",
    "about.feedback.action",
    "about.logs.title",
    "about.logs.description",
    "about.logs.action",
    "update.about.label",
];

/// The product identity shown in the first row.
///
/// This deliberately contains only compiled identity and the platform
/// constants. It does not include a storage path, a model name, a log excerpt,
/// or a user file name, so copying it for a bug report cannot accidentally
/// disclose the user's data.
pub(super) fn software_info_text(
    language: SettingsLanguage,
    build_info: &SettingsBuildInfo,
) -> String {
    let locale = language.catalog_locale();
    format!(
        "BongoCat\n{}\n{}: {}\n{}: {}",
        build_info_detail(language, build_info),
        bongocat_i18n::text(locale, "about.software_information.platform"),
        std::env::consts::OS,
        bongocat_i18n::text(locale, "about.software_information.architecture"),
        std::env::consts::ARCH,
    )
}

fn product_info_description(snapshot: Option<&SettingsSnapshot>) -> String {
    let language = snapshot.map_or(SettingsLanguage::EnglishUnitedStates, |snapshot| {
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
        "BongoCat",
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
                    .secondary()
                    .disabled(!available)
                    .on_click(move |_, _, app| {
                        action_view.update(app, |view, cx| view.copy_software_info(cx));
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
    let project_description = format!(
        "{}\n{}",
        bongocat_i18n::text(locale, "about.project.description"),
        PROJECT_SOURCE_URL
    );
    let project = SettingItem::new(
        bongocat_i18n::text(locale, "about.project.title"),
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                let available = project_view.read(app).snapshot.is_some();
                let action_view = project_view.clone();
                Button::new("about-open-project-source-button")
                    .label(bongocat_i18n::text(locale, "about.project.action"))
                    .with_size(options.size())
                    .secondary()
                    .disabled(!available)
                    .on_click(move |_, _, app| {
                        action_view.update(app, |view, cx| view.open_project_source(cx));
                    })
            },
        ),
    )
    .description(project_description)
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
                    .secondary()
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
    let logs = SettingItem::new(
        bongocat_i18n::text(locale, "about.logs.title"),
        SettingField::element(
            move |options: &RenderOptions, _: &mut Window, app: &mut App| {
                let available = logs_view.read(app).snapshot.is_some();
                let action_view = logs_view.clone();
                Button::new("about-open-logs-button")
                    .label(bongocat_i18n::text(locale, "about.logs.action"))
                    .with_size(options.size())
                    .secondary()
                    .disabled(!available)
                    .on_click(move |_, _, app| {
                        action_view.update(app, |view, cx| view.open_logs_location(cx));
                    })
            },
        ),
    )
    .description(bongocat_i18n::text(locale, "about.logs.description"))
    .keywords(keywords);

    SettingGroup::new().items([product, software, project, feedback, logs])
}

impl SettingsView {
    /// Copy the small, privacy-safe build summary a bug reporter needs.
    ///
    /// Clipboard access is a main-thread platform capability. This method is
    /// called from the GPUI button callback, never from the settings worker, so
    /// the macOS AppKit invariant is preserved by construction.
    pub(super) fn copy_software_info(&mut self, cx: &mut Context<Self>) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let text = software_info_text(snapshot.resolved_language, &snapshot.build_info);
        match bongocat_platform::write_clipboard_text(&text) {
            Ok(()) => self.about_copy_success_pending = true,
            Err(_) => {
                self.pending_notification = Some(SettingsError::new(
                    SettingsErrorCode::SoftwareInfoCopyFailed,
                ));
            }
        }
        cx.notify();
    }

    fn open_external_link(&mut self, url: &str, cx: &mut Context<Self>) {
        if bongocat_platform::open_external_url(url).is_err() {
            self.pending_notification = Some(SettingsError::new(
                SettingsErrorCode::ExternalLinkOpenFailed,
            ));
        }
        cx.notify();
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
