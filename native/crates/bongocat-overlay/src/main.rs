use std::{env, path::PathBuf, process::ExitCode, time::Duration};

fn main() -> ExitCode {
    match run() {
        Ok(report) => {
            println!(
                "BongoCat Live2D preview: frames={} drawables={} masked_drawables={} textures={}",
                report.frames_presented,
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
    if arguments.next().is_some() {
        return Err("usage: bongocat-overlay [standard|keyboard|gamepad] [seconds]".to_owned());
    }
    let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .ok_or_else(|| "cannot locate repository root".to_owned())?
        .to_owned();
    bongocat_overlay::run_model_preview(
        &model_id,
        &repository_root
            .join("native/resources/models")
            .join(&model_id),
        Duration::from_secs(seconds),
    )
    .map_err(|error| error.to_string())
}
