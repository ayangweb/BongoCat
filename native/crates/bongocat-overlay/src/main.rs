use std::{env, path::PathBuf, process::ExitCode, time::Duration};

fn main() -> ExitCode {
    match run() {
        Ok(report) => {
            println!(
                "BongoCat Live2D preview: frames={} dynamic_snapshots={} runtime_input_events={} platform_input_edges={} runtime_cursor_published={} runtime_cursor_coalesced={} runtime_cursor_consumed={} platform_cursor_samples={} render_frames_published={} render_frames_coalesced={} render_frames_consumed={} drawables={} masked_drawables={} textures={}",
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
                report.drawable_count,
                report.masked_drawable_count,
                report.texture_count
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("BongoCat Live2D preview failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bongocat_overlay::PreviewReport, String> {
    let mut arguments = env::args().skip(1);
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
    let interactive = match arguments.next().as_deref() {
        None => false,
        Some("--interactive") => true,
        Some(_) => {
            return Err(
                "usage: bongocat-overlay [standard|keyboard|gamepad] [seconds] [--interactive]"
                    .to_owned(),
            );
        }
    };
    if arguments.next().is_some() {
        return Err(
            "usage: bongocat-overlay [standard|keyboard|gamepad] [seconds] [--interactive]"
                .to_owned(),
        );
    }
    let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .ok_or_else(|| "cannot locate repository root".to_owned())?
        .to_owned();
    let model_root = repository_root
        .join("native/resources/models")
        .join(&model_id);
    let run = if interactive {
        bongocat_overlay::run_interactive_model_preview
    } else {
        bongocat_overlay::run_model_preview
    };
    run(&model_id, &model_root, Duration::from_secs(seconds)).map_err(|error| error.to_string())
}
