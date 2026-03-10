use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_SOURCE: &str = "examples/welcome.html";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceOrigin {
    Local(PathBuf),
    Remote(String),
}

pub fn load_html(source: Option<&str>) -> Result<(String, PathBuf), String> {
    let requested = source.unwrap_or(DEFAULT_SOURCE);
    match classify_source(requested) {
        SourceOrigin::Local(path) => {
            let html = fs::read_to_string(&path)
                .map_err(|err| format!("failed to read HTML from {}: {err}", path.display()))?;

            Ok((html, path))
        }
        SourceOrigin::Remote(url) => {
            let response = reqwest::blocking::get(&url)
                .map_err(|err| format!("failed to fetch HTML from {url}: {err}"))?;
            let status = response.status();
            if !status.is_success() {
                return Err(format!("request to {url} failed with status {status}"));
            }

            let html = response
                .text()
                .map_err(|err| format!("failed to decode HTML from {url}: {err}"))?;

            Ok((html, PathBuf::from(url)))
        }
    }
}

pub fn classify_source(requested: &str) -> SourceOrigin {
    if requested.starts_with("http://") || requested.starts_with("https://") {
        SourceOrigin::Remote(requested.to_string())
    } else {
        SourceOrigin::Local(Path::new(requested).to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_source, load_html, SourceOrigin, DEFAULT_SOURCE};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::thread;

    #[test]
    fn loads_default_fixture() {
        let (html, path) = load_html(None).expect("default fixture should load");
        assert!(path.ends_with(DEFAULT_SOURCE));
        assert!(html.contains("<html>"));
    }

    #[test]
    fn classifies_remote_source() {
        assert_eq!(
            classify_source("https://example.com"),
            SourceOrigin::Remote("https://example.com".to_string())
        );
    }

    #[test]
    fn classifies_local_source() {
        assert_eq!(
            classify_source("examples/welcome.html"),
            SourceOrigin::Local(Path::new("examples/welcome.html").to_path_buf())
        );
    }

    #[test]
    fn loads_html_over_http() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("read local addr");

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            let body = "<html><body><p>remote fixture</p></body></html>";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        });

        let url = format!("http://{address}");
        let (html, path) = load_html(Some(&url)).expect("remote fixture should load");
        assert!(html.contains("remote fixture"));
        assert_eq!(path, PathBuf::from(url));

        handle.join().expect("server thread should finish");
    }
}
