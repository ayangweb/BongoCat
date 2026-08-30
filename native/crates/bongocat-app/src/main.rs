#![forbid(unsafe_code)]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let application = bongocat_app::Application::start()?;
    application.shutdown()?;
    Ok(())
}
