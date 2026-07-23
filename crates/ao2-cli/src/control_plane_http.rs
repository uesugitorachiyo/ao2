use std::io::{Read, Write as IoWrite};
use std::net::TcpStream;

use anyhow::{anyhow, Context, Result};

use crate::trimmed_required;

pub(crate) fn control_plane_endpoint(base_url: &str, path: &str) -> Result<String> {
    let base = trimmed_required("--control-plane-url", base_url)?;
    let base = base.trim_end_matches('/');
    let suffix = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    Ok(format!("{base}{suffix}"))
}

pub(crate) fn post_json_http(url: &str, api_token: &str, body: &str) -> Result<serde_json::Value> {
    let endpoint = parse_http_endpoint(url)?;
    let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port))
        .with_context(|| format!("connect {}", endpoint.authority))?;
    let request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        endpoint.path,
        endpoint.authority,
        api_token,
        body.len(),
        body
    );
    stream.write_all(request.as_bytes())?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let (head, response_body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow!("control-plane response missing header/body separator"))?;
    let status_line = head.lines().next().unwrap_or_default();
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("control-plane response missing HTTP status"))?
        .parse::<u16>()
        .context("parse control-plane HTTP status")?;
    if !(200..300).contains(&status) {
        return Err(anyhow!(
            "control-plane POST failed with HTTP {status}: {response_body}"
        ));
    }
    serde_json::from_str(response_body).context("parse control-plane JSON response")
}

pub(crate) fn get_text_http(url: &str, api_token: &str) -> Result<String> {
    let endpoint = parse_http_endpoint(url)?;
    let mut stream = connect_http_endpoint(&endpoint)?;
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nAccept: text/html\r\nConnection: close\r\n\r\n",
        endpoint.path, endpoint.authority, api_token
    );
    stream.write_all(request.as_bytes())?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let (head, response_body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow!("control-plane response missing header/body separator"))?;
    let status_line = head.lines().next().unwrap_or_default();
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("control-plane response missing HTTP status"))?
        .parse::<u16>()
        .context("parse control-plane HTTP status")?;
    if !(200..300).contains(&status) {
        return Err(anyhow!(
            "control-plane GET failed with HTTP {status}: {response_body}"
        ));
    }
    Ok(response_body.to_string())
}

pub(crate) fn get_json_http(url: &str, api_token: &str) -> Result<serde_json::Value> {
    let body = get_text_http(url, api_token)?;
    serde_json::from_str(&body).context("parse control-plane JSON response")
}

fn connect_http_endpoint(endpoint: &HttpEndpoint) -> Result<TcpStream> {
    let mut last_error = None;
    for _ in 0..50 {
        match TcpStream::connect((endpoint.host.as_str(), endpoint.port)) {
            Ok(stream) => return Ok(stream),
            Err(error) => {
                last_error = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
    Err(last_error
        .map(anyhow::Error::from)
        .unwrap_or_else(|| anyhow!("connection attempt did not run")))
    .with_context(|| format!("connect {}", endpoint.authority))
}

#[derive(Debug)]
pub(crate) struct HttpEndpoint {
    host: String,
    port: u16,
    authority: String,
    path: String,
}

pub(crate) fn parse_http_endpoint(url: &str) -> Result<HttpEndpoint> {
    let raw = trimmed_required("url", url)?;
    let without_scheme = raw
        .strip_prefix("http://")
        .ok_or_else(|| anyhow!("only http:// control-plane URLs are supported"))?;
    let (authority, path) = without_scheme
        .split_once('/')
        .map(|(authority, path)| (authority, format!("/{path}")))
        .unwrap_or((without_scheme, "/".to_string()));
    if authority.is_empty() {
        return Err(anyhow!("control-plane URL missing host"));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => (
            host.to_string(),
            port.parse::<u16>()
                .with_context(|| format!("parse port in {authority}"))?,
        ),
        _ => (authority.to_string(), 80),
    };
    Ok(HttpEndpoint {
        host,
        port,
        authority: authority.to_string(),
        path,
    })
}
