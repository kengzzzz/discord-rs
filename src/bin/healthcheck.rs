use std::io::{Read, Write};
use std::net::TcpStream;

fn main() {
    let address = "127.0.0.1:8080";
    match TcpStream::connect(address) {
        Ok(mut stream) => {
            let request = "GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
            if stream.write_all(request.as_bytes()).is_err() {
                std::process::exit(1);
            }

            let mut buffer = String::new();
            if stream.read_to_string(&mut buffer).is_err() {
                std::process::exit(1);
            }

            if buffer.contains("HTTP/1.1 200 OK") {
                std::process::exit(0);
            } else {
                eprintln!("Healthcheck failed: Not 200 OK");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Healthcheck connection failed: {}", e);
            std::process::exit(1);
        }
    }
}
