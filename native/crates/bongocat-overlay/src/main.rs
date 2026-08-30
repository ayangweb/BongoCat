use std::{env, path::PathBuf, process::ExitCode, time::Duration};

fn main() -> ExitCode {
    match run() {
        Ok(report) => {
            println!(
                "BongoCat Live2D preview: frames={} dynamic_snapshots={} runtime_input_events={} platform_input_edges={} runtime_cursor_published={} runtime_cursor_coalesced={} runtime_cursor_consumed={} platform_cursor_samples={} render_frames_published={} render_frames_coalesced={} render_frames_consumed={} model_switches={} failed_gpu_prepare_preserved={} metal_bytes_before={} metal_bytes_after={} drawables={} masked_drawables={} textures={}",
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
                report.metal_bytes_before,
                report.metal_bytes_after,
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
    let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .ok_or_else(|| "cannot locate repository root".to_owned())?
        .to_owned();
    let model_root = repository_root
        .join("native/resources/models")
        .join(&model_id);
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
    report.map_err(|error| error.to_string())
}
