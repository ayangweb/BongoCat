#![forbid(unsafe_code)]
use std::{
    io::{self, Write},
    net::SocketAddr,
};

#[tokio::main]
async fn main() -> io::Result<()> {
    let address: SocketAddr = std::env::var("BONGOCAT_ROOM_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid BONGOCAT_ROOM_LISTEN"))?;
    let server = bongocat_room_server::RoomServer::bind(address).await?;
    writeln!(
        io::stdout().lock(),
        "room server listening on {}",
        server.address()?
    )?;
    server.run(async {
        #[cfg(unix)]
        {
            if let Ok(mut termination) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = termination.recv() => {} }
            } else { let _ = tokio::signal::ctrl_c().await; }
        }
        #[cfg(windows)]
        { let _ = tokio::signal::ctrl_c().await; }
    }).await
}
