use std::{env, path::PathBuf, process::ExitCode, sync::Arc, time::Duration};

/// Subcommand that renders one model into a cover image instead of previewing it.
const CAPTURE_COVER: &str = "capture-cover";

fn main() -> ExitCode {
    match run() {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("BongoCat Live2D preview failed: {error}");
            ExitCode::FAILURE
        }
    }
}

/// What a preview run was asked to do.
#[derive(Debug, PartialEq, Eq)]
struct PreviewOptions {
    model_id: String,
    seconds: u64,
    interactive: bool,
    switch_cycles: Option<u32>,
}

const PREVIEW_USAGE: &str = "usage: bongocat-overlay [standard|keyboard|gamepad] [seconds] [--interactive|--switch-cycles cycles]";

/// Reads the preview arguments, with the model first and the duration second.
///
/// `first` is the argument `run` already took to tell the subcommand from a
/// preview, handed back rather than re-read. Testing it against the subcommand
/// name spent it: the model was never seen, so `bongocat-overlay standard 30`
/// read "30" as the model and refused to run, and every preview invocation the
/// justfile and CI use was broken with it.
fn parse_preview_arguments(
    first: Option<String>,
    rest: impl Iterator<Item = String>,
) -> Result<PreviewOptions, String> {
    let mut arguments = rest;
    let model_id = first.unwrap_or_else(|| "standard".to_owned());
    if !matches!(model_id.as_str(), "standard" | "keyboard" | "gamepad") {
        return Err("model must be standard, keyboard, or gamepad".to_owned());
    }
    let seconds = arguments
        .next()
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| "duration must be whole seconds".to_owned())
        })
        .transpose()?
        .unwrap_or(15);
    let (interactive, switch_cycles) = match arguments.next().as_deref() {
        None => (false, None),
        Some("--interactive") => (true, None),
        Some("--switch-cycles") => {
            let cycles = arguments
                .next()
                .ok_or_else(|| "--switch-cycles requires a cycle count".to_owned())?
                .parse::<u32>()
                .map_err(|_| "switch cycle count must be a whole number".to_owned())?;
            (false, Some(cycles))
        }
        Some(_) => return Err(PREVIEW_USAGE.to_owned()),
    };
    if arguments.next().is_some() {
        return Err(PREVIEW_USAGE.to_owned());
    }
    Ok(PreviewOptions {
        model_id,
        seconds,
        interactive,
        switch_cycles,
    })
}

fn run() -> Result<String, String> {
    let mut arguments = env::args().skip(1);
    let first = arguments.next();
    if first.as_deref() == Some(CAPTURE_COVER) {
        return capture_cover(arguments);
    }
    let PreviewOptions {
        model_id,
        seconds,
        interactive,
        switch_cycles,
    } = parse_preview_arguments(first, arguments)?;
    let model_root = repository_root()?.join("resources/models").join(&model_id);
    let report = if let Some(cycles) = switch_cycles {
        bongocat_overlay::run_model_switch_preview(&model_id, &model_root, cycles)
    } else if interactive {
        bongocat_overlay::run_interactive_model_preview(
            &model_id,
            &model_root,
            Duration::from_secs(seconds),
        )
    } else {
        bongocat_overlay::run_model_preview(&model_id, &model_root, Duration::from_secs(seconds))
    };
    report.map(|report| format!(
        "BongoCat Live2D preview: frames={} dynamic_snapshots={} runtime_input_events={} platform_input_edges={} runtime_cursor_published={} runtime_cursor_coalesced={} runtime_cursor_consumed={} platform_cursor_samples={} render_frames_published={} render_frames_coalesced={} render_frames_consumed={} model_switches={} failed_gpu_prepare_preserved={} gpu_bytes_before={} gpu_bytes_after={} drawables={} masked_drawables={} textures={} warmup_thread_high_water={:?} threads_after={:?} frame_timing={:?}",
        report.frames_presented,
        report.dynamic_snapshots,
        report.runtime_input_events,
        report.platform_input_edges,
        report.runtime_cursor_published,
        report.runtime_cursor_coalesced,
        report.runtime_cursor_consumed,
        report.platform_cursor_samples,
        report.render_frames_published,
        report.render_frames_coalesced,
        report.render_frames_consumed,
        report.model_switches,
        report.failed_gpu_prepare_preserved,
        report.gpu_bytes_before,
        report.gpu_bytes_after,
        report.drawable_count,
        report.masked_drawable_count,
        report.texture_count,
        report.warmup_thread_high_water,
        report.threads_after,
        report.frame_timing
    ))
    .map_err(|error| error.to_string())
}

/// Render one preset model into the cover PNG written at `output`.
///
/// This is the capture the product runs after importing a model, driven from the
/// command line so the whole path — hidden window, GPU readback, crop, encode —
/// can be exercised and inspected without going through an import.
fn capture_cover(arguments: impl Iterator<Item = String>) -> Result<String, String> {
    const USAGE: &str =
        "usage: bongocat-overlay capture-cover <standard|keyboard|gamepad> <output.png>";
    let mut arguments = arguments;
    let model_id = arguments.next().unwrap_or_else(|| "standard".to_owned());
    let output = arguments.next().ok_or_else(|| USAGE.to_owned())?;
    if arguments.next().is_some() {
        return Err(USAGE.to_owned());
    }
    let model_root = repository_root()?.join("resources/models");
    let catalog = bongocat_model::PresetModelCatalog::open(
        &model_root,
        bongocat_model::ModelPackageLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let id = bongocat_model::ModelId::parse(&model_id).map_err(|error| error.to_string())?;
    let model = catalog.load(&id).map_err(|error| error.to_string())?;
    let cover = bongocat_overlay::capture_model_cover(Arc::new(model))
        .map_err(|error| error.to_string())?;
    std::fs::write(&output, cover.png()).map_err(|error| format!("write {output}: {error}"))?;
    Ok(format!(
        "BongoCat model cover capture: model={model_id} bytes={} width={} height={} path={output}",
        cover.png().len(),
        cover.width(),
        cover.height()
    ))
}

fn repository_root() -> Result<PathBuf, String> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .ok_or_else(|| "cannot locate repository root".to_owned())?
        .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(first: Option<&str>, rest: &[&str]) -> Result<PreviewOptions, String> {
        parse_preview_arguments(
            first.map(str::to_owned),
            rest.iter().map(|value| (*value).to_owned()),
        )
    }

    /// The model is the first argument, and the subcommand check must not spend it.
    ///
    /// It did, and the symptom was that every documented invocation was rejected:
    /// `bongocat-overlay standard 30` read the duration as the model and reported
    /// "model must be standard, keyboard, or gamepad", so both the justfile's
    /// preview recipe and the CI model-switching smoke were broken by it.
    #[test]
    fn the_first_argument_is_the_model_and_is_not_consumed_by_the_subcommand_check() {
        let options = parse(Some("keyboard"), &["30"]).expect("preview options");
        assert_eq!(options.model_id, "keyboard");
        assert_eq!(options.seconds, 30);
        assert!(!options.interactive);
        assert_eq!(options.switch_cycles, None);

        let options =
            parse(Some("standard"), &["0", "--switch-cycles", "3"]).expect("switch cycle options");
        assert_eq!(options.model_id, "standard");
        assert_eq!(options.seconds, 0);
        assert_eq!(options.switch_cycles, Some(3));
    }

    #[test]
    fn an_empty_command_line_previews_the_standard_model() {
        let options = parse(None, &[]).expect("default options");
        assert_eq!(options.model_id, "standard");
        assert_eq!(options.seconds, 15);
        assert!(!options.interactive);
        assert_eq!(options.switch_cycles, None);
    }

    #[test]
    fn an_unknown_model_and_a_trailing_argument_are_refused() {
        assert!(
            parse(Some("30"), &[])
                .expect_err("a duration in the model slot")
                .contains("model must be")
        );
        assert_eq!(
            parse(Some("standard"), &["1", "2"]),
            Err(PREVIEW_USAGE.to_owned())
        );
        assert!(parse(Some("standard"), &["--switch-cycles", "many"]).is_err());
    }
}
