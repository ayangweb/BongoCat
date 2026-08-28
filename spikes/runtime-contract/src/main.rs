use bongocat_runtime_contract_spike::RuntimeContract;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = RuntimeContract::new();
    runtime.mark_ready()?;
    runtime.tick(0)?;
    runtime.begin_shutdown()?;
    runtime.complete_shutdown()?;
    println!("runtime-contract-spike: state={:?}", runtime.state());
    Ok(())
}
