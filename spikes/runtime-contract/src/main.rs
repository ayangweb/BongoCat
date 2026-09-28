use bongocat_runtime_contract_spike::{RuntimeCommand, RuntimeWorker};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let worker = RuntimeWorker::spawn(8);
    worker.send(RuntimeCommand::MarkReady)?;
    worker.send(RuntimeCommand::Tick { at_ms: 0 })?;
    let report = worker.shutdown();
    println!(
        "runtime-contract-spike: state={:?} revision={} worker_status={:?} exit={:?}",
        report.snapshot.state, report.snapshot.revision, report.snapshot.worker_status, report.exit
    );
    Ok(())
}
