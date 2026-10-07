//! A static file server for the browser build: cross-origin isolation headers (so
//! `SharedArrayBuffer` and WASM threads work), no caching, and an SPA fallback to `index.html`.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::thread::JoinHandle;

/// Headers every response carries: they make the page cross-origin isolated.
pub const ISOLATION_HEADERS: [(&str, &str); 3] = [
    ("Cross-Origin-Opener-Policy", "same-origin"),
    ("Cross-Origin-Embedder-Policy", "require-corp"),
    ("Cross-Origin-Resource-Policy", "same-origin"),
];

/// Starts serving `root` on `127.0.0.1:port` (`0` picks a free port) in the background.
pub fn start(root: PathBuf, port: u16) -> std::io::Result<(SocketAddr, JoinHandle<()>)> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let address = listener.local_addr()?;
    let handle = std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let root = root.clone();
            std::thread::spawn(move || {
                handle(stream, &root).ok();
            });
        }
    });
    Ok((address, handle))
}

/// The file `path` (a request path) names under `root`, or `None` when it escapes the root.
/// Paths without an extension that name no file fall back to `index.html`.
pub fn resolve(root: &Path, path: &str) -> Option<PathBuf> {
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let relative = Path::new(path.trim_start_matches('/'));
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let file = if path.ends_with('/') || relative.as_os_str().is_empty() {
        root.join(relative).join("index.html")
    } else {
        root.join(relative)
    };
    if file.is_file() {
        Some(file)
    } else if relative.extension().is_none() {
        Some(root.join("index.html"))
    } else {
        Some(file)
    }
}

/// The media type served for `path`.
pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ttf") => "font/ttf",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn handle(mut stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    // Drain the headers; the server needs only the request line.
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or("/");
    let (status, body, file) = match (method, resolve(root, path)) {
        ("GET" | "HEAD", Some(file)) => match std::fs::read(&file) {
            Ok(body) => ("200 OK", body, Some(file)),
            Err(_) => ("404 Not Found", b"not found".to_vec(), None),
        },
        ("GET" | "HEAD", None) => ("403 Forbidden", b"forbidden".to_vec(), None),
        _ => ("405 Method Not Allowed", Vec::new(), None),
    };
    let mut response = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: {}\r\nCache-Control: no-store\r\nConnection: close\r\n",
        body.len(),
        file.as_deref().map_or("text/plain", content_type)
    );
    for (name, value) in ISOLATION_HEADERS {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    stream.write_all(response.as_bytes())?;
    if method != "HEAD" {
        stream.write_all(&body)?;
    }
    stream.flush()
}
