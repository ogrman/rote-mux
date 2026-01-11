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

/// Make an HTTP GET request to localhost:port and check for a successful response.
/// Returns Ok(()) if the response status is 2xx, Err otherwise.
pub async fn http_get(port: u16) -> Result<()> {
    let url = format!("http://127.0.0.1:{port}/");
    let response = reqwest::get(&url).await?;

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
}
