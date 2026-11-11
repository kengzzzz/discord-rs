use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    process::ExitCode,
    time::Duration,
};

fn main() -> ExitCode {
    // Falls back to HTTP_ADDR, so changing only that keeps the container healthy.
    let parsed = |var| {
        std::env::var(var)
            .ok()?
            .parse::<SocketAddr>()
            .ok()
    };
    let addr: SocketAddr = parsed("HEALTHCHECK_ADDR")
        .or_else(|| parsed("HTTP_ADDR"))
        .map(|mut addr| {
            if addr.ip().is_unspecified() {
                addr.set_ip([127, 0, 0, 1].into());
            }
            addr
        })
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 8080)));
    match probe(addr) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("healthcheck failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn probe(addr: SocketAddr) -> std::io::Result<bool> {
    let timeout = Duration::from_secs(3);
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response.starts_with("HTTP/1.1 200"))
}
