use std::{
    env,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::atomic::{AtomicU64, Ordering},
    thread,
};

const SERVICE_NAME: &str = "notify";
static REQUESTS_TOTAL: AtomicU64 = AtomicU64::new(0);

fn main() -> std::io::Result<()> {
    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_owned());
    let listener = TcpListener::bind(format!("0.0.0.0:{port}"))?;

    println!("{SERVICE_NAME} listening on 0.0.0.0:{port}");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(|| {
                    if let Err(error) = handle(stream) {
                        eprintln!("request failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("accept failed: {error}"),
        }
    }

    Ok(())
}

fn handle(mut stream: TcpStream) -> std::io::Result<()> {
    let mut request = [0_u8; 2048];
    let bytes_read = stream.read(&mut request)?;
    let request = String::from_utf8_lossy(&request[..bytes_read]);
    let path = request.split_whitespace().nth(1).unwrap_or("/");
    REQUESTS_TOTAL.fetch_add(1, Ordering::Relaxed);

    let (status, content_type, body) = response(path);
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );

    stream.write_all(response.as_bytes())
}

fn response(path: &str) -> (&'static str, &'static str, String) {
    match path {
        "/" => (
            "200 OK",
            "text/plain; charset=utf-8",
            format!("hello from gavel {SERVICE_NAME}\n"),
        ),
        "/healthz" | "/readyz" => (
            "200 OK",
            "application/json",
            format!("{{\"status\":\"ok\",\"service\":\"{SERVICE_NAME}\"}}\n"),
        ),
        "/metrics" => (
            "200 OK",
            "text/plain; version=0.0.4; charset=utf-8",
            format!(
                "# HELP gavel_service_info Static service identity.\n\
                 # TYPE gavel_service_info gauge\n\
                 gavel_service_info{{service=\"{SERVICE_NAME}\"}} 1\n\
                 # HELP gavel_http_requests_total Requests handled by the bootstrap server.\n\
                 # TYPE gavel_http_requests_total counter\n\
                 gavel_http_requests_total{{service=\"{SERVICE_NAME}\"}} {}\n",
                REQUESTS_TOTAL.load(Ordering::Relaxed)
            ),
        ),
        _ => (
            "404 Not Found",
            "application/json",
            "{\"error\":\"not_found\"}\n".to_owned(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::response;

    #[test]
    fn exposes_health_and_metrics() {
        assert_eq!(response("/healthz").0, "200 OK");
        assert!(response("/metrics").2.contains("gavel_service_info"));
    }
}
