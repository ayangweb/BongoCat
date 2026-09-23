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

fn run() -> Result<String, String> {
    let mut arguments = env::args().skip(1);
    if arguments.next().as_deref() == Some(CAPTURE_COVER) {
        return capture_cover(arguments);
    }
    let model_id = arguments.next().unwrap_or_else(|| "standard".to_owned());
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
        Some(_) => {
            return Err(
                "usage: bongocat-overlay [standard|keyboard|gamepad] [seconds] [--interactive|--switch-cycles cycles]".to_owned(),
            );
        }
    };
    if arguments.next().is_some() {
        return Err(
            "usage: bongocat-overlay [standard|keyboard|gamepad] [seconds] [--interactive|--switch-cycles cycles]".to_owned(),
        );
    }
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
