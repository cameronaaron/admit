//! A server that closes an HTTP body before its advertised length.
//! wiremock always produces complete bodies, so it cannot model this failure.
pub fn responses(bodies: Vec<&'static str>) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    std::thread::spawn(move || {
        for body in bodies {
            let (mut stream, _) = listener.accept().expect("connection");
            stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).expect("timeout");
            let mut bytes = [0; 8192];
            let count = stream.read(&mut bytes).expect("request");
            assert!(count > 0, "request must arrive before the response");
            if body.is_empty() {
                let _ = stream.shutdown(std::net::Shutdown::Both);
                continue;
            }
            stream.write_all(body.as_bytes()).expect("response");
        }
    });
    url
}
#[allow(dead_code)] // used from lifecycle.rs; edges.rs only drops the connection
pub const TRUNCATED: &str = "HTTP/1.1 200 OK\r\nContent-Length: 10000\r\nConnection: close\r\n\r\n{\"partial\":";
pub const EMPTY: &str = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
/// Accept the connection and close it before any response bytes.
#[allow(dead_code)] // used from edges.rs; lifecycle.rs only truncates a body
pub const DROPPED: &str = "";
