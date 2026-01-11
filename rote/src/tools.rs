use anyhow::{Result, anyhow};
use std::net::TcpStream;

/// Check if a port is open on localhost.
/// Returns Ok(()) if the port is open, Err if it's closed or unreachable.
pub async fn is_port_open(port: u16) -> Result<()> {
    let addr = format!("127.0.0.1:{port}");

    // Use blocking connect in a spawn_blocking since TcpStream::connect is blocking
    let result = tokio::task::spawn_blocking(move || TcpStream::connect(&addr)).await?;

    match result {
        Ok(_) => Ok(()),
        Err(_) => Err(anyhow!("port {port} is not open")),
    }
}

/// Make an HTTP GET request.
/// The URL should be a full http(s) URL.
/// Returns Ok(()) if the request completes (any status code), Err if connection fails.
pub async fn http_get(url: &str) -> Result<()> {
    let _response = reqwest::get(url).await?;
    Ok(())
}

/// Make an HTTP GET request and check for a successful response.
/// The URL should be a full http(s) URL.
/// Returns Ok(()) if the response status is 2xx, Err otherwise.
pub async fn http_get_ok(url: &str) -> Result<()> {
    let response = reqwest::get(url).await?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(anyhow!(
            "HTTP GET {} returned status {}",
            url,
            response.status()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[tokio::test]
    async fn test_is_port_open_with_open_port() {
        // Bind to a random available port
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // Port should be open
        let result = is_port_open(port).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_is_port_open_with_closed_port() {
        // Use a port that's very likely not in use (high ephemeral port)
        // We create and immediately drop a listener to get a port that was just freed
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        // Listener is now dropped, port should be closed

        let result = is_port_open(port).await;
        assert!(result.is_err());
    }

    /// Spawn a simple HTTP server that responds with the given status code.
    fn spawn_http_server(status_code: u16) -> (u16, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                // Read the request (we don't care about the content)
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);

                // Send HTTP response
                let response = format!(
                    "HTTP/1.1 {} OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    status_code
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });

        (port, handle)
    }

    #[tokio::test]
    async fn test_http_get_success() {
        let (port, handle) = spawn_http_server(200);
        let url = format!("http://127.0.0.1:{}/", port);

        let result = http_get(&url).await;
        assert!(result.is_ok(), "http_get should succeed for any response");

        handle.join().unwrap();
    }

    #[tokio::test]
    async fn test_http_get_success_with_non_2xx() {
        // http_get should succeed even with 404 status
        let (port, handle) = spawn_http_server(404);
        let url = format!("http://127.0.0.1:{}/", port);

        let result = http_get(&url).await;
        assert!(
            result.is_ok(),
            "http_get should succeed even with non-2xx status"
        );

        handle.join().unwrap();
    }

    #[tokio::test]
    async fn test_http_get_connection_refused() {
        // Get a port that's not listening
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let url = format!("http://127.0.0.1:{}/", port);

        let result = http_get(&url).await;
        assert!(
            result.is_err(),
            "http_get should fail when connection is refused"
        );
    }

    #[tokio::test]
    async fn test_http_get_ok_success() {
        let (port, handle) = spawn_http_server(200);
        let url = format!("http://127.0.0.1:{}/", port);

        let result = http_get_ok(&url).await;
        assert!(result.is_ok(), "http_get_ok should succeed for 2xx status");

        handle.join().unwrap();
    }

    #[tokio::test]
    async fn test_http_get_ok_fails_on_404() {
        let (port, handle) = spawn_http_server(404);
        let url = format!("http://127.0.0.1:{}/", port);

        let result = http_get_ok(&url).await;
        assert!(
            result.is_err(),
            "http_get_ok should fail for non-2xx status"
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("404"), "error should mention status code");

        handle.join().unwrap();
    }

    #[tokio::test]
    async fn test_http_get_ok_fails_on_500() {
        let (port, handle) = spawn_http_server(500);
        let url = format!("http://127.0.0.1:{}/", port);

        let result = http_get_ok(&url).await;
        assert!(result.is_err(), "http_get_ok should fail for 5xx status");

        handle.join().unwrap();
    }

    #[tokio::test]
    async fn test_http_get_ok_connection_refused() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let url = format!("http://127.0.0.1:{}/", port);

        let result = http_get_ok(&url).await;
        assert!(
            result.is_err(),
            "http_get_ok should fail when connection is refused"
        );
    }
}
