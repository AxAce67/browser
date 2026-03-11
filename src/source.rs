use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE};
use reqwest::redirect::Policy;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

pub const DEFAULT_SOURCE: &str = "examples/welcome.html";
const REQUEST_TIMEOUT_SECS: u64 = 15;
const MAX_REDIRECTS: usize = 10;
const BROWSER_USER_AGENT: &str = "ToyBrowser/0.1 (+https://github.com/AxAce67/browser)";
const ACCEPT_HEADER: &str = "text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.5";

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
        SourceOrigin::Remote(url) => fetch_remote_html(&url),
    }
}

pub fn classify_source(requested: &str) -> SourceOrigin {
    if requested.starts_with("http://") || requested.starts_with("https://") {
        SourceOrigin::Remote(requested.to_string())
    } else {
        SourceOrigin::Local(Path::new(requested).to_path_buf())
    }
}

pub fn resolve_reference(current_source: &str, target: &str) -> String {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return current_source.to_string();
    }

    if matches!(classify_source(trimmed), SourceOrigin::Remote(_)) {
        return trimmed.to_string();
    }

    if let Ok(base_url) = Url::parse(current_source) {
        return base_url
            .join(trimmed)
            .map(|url| url.to_string())
            .unwrap_or_else(|_| trimmed.to_string());
    }

    let target_path = Path::new(trimmed);
    if target_path.is_absolute() {
        return target_path.to_path_buf().display().to_string();
    }

    let current_path = Path::new(current_source);
    current_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(target_path)
        .display()
        .to_string()
}

pub fn normalize_browser_url(requested: &str) -> Result<String, String> {
    let trimmed = requested.trim();
    if trimmed.is_empty() {
        return Err("address is empty".to_string());
    }

    if let Ok(url) = Url::parse(trimmed) {
        return Ok(url.to_string());
    }

    let candidate_path = Path::new(trimmed);
    if candidate_path.exists() {
        let absolute = if candidate_path.is_absolute() {
            candidate_path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|err| format!("failed to resolve current directory: {err}"))?
                .join(candidate_path)
        };
        return Url::from_file_path(&absolute)
            .map(|url| url.to_string())
            .map_err(|_| format!("failed to convert {} into a file URL", absolute.display()));
    }

    if looks_like_host(trimmed) {
        return Url::parse(&format!("https://{trimmed}"))
            .map(|url| url.to_string())
            .map_err(|err| format!("invalid address {trimmed}: {err}"));
    }

    Err(format!("unsupported address: {trimmed}"))
}

fn fetch_remote_html(url: &str) -> Result<(String, PathBuf), String> {
    let client = build_http_client()?;
    let response = client
        .get(url)
        .send()
        .map_err(|err| format!("failed to fetch HTML from {url}: {err}"))?;

    let status = response.status();
    let final_url = response.url().to_string();
    if !status.is_success() {
        return Err(format!(
            "request to {final_url} failed with status {status}"
        ));
    }

    if let Some(content_type) = response.headers().get(CONTENT_TYPE) {
        let content_type = content_type.to_str().unwrap_or_default();
        if !supports_text_response(content_type) {
            return Err(format!(
                "unsupported content type from {final_url}: {content_type}"
            ));
        }
    }

    let html = response
        .text()
        .map_err(|err| format!("failed to decode HTML from {final_url}: {err}"))?;

    Ok((html, PathBuf::from(final_url)))
}

fn build_http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .connect_timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .redirect(Policy::limited(MAX_REDIRECTS))
        .user_agent(BROWSER_USER_AGENT)
        .default_headers(default_request_headers())
        .build()
        .map_err(|err| format!("failed to build HTTP client: {err}"))
}

fn default_request_headers() -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(ACCEPT, ACCEPT_HEADER.parse().expect("valid accept header"));
    headers.insert(
        ACCEPT_LANGUAGE,
        "en-US,en;q=0.9"
            .parse()
            .expect("valid accept-language header"),
    );
    headers
}

fn supports_text_response(content_type: &str) -> bool {
    let normalized = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    normalized.starts_with("text/html")
        || normalized.starts_with("application/xhtml+xml")
        || normalized.starts_with("text/plain")
}

fn looks_like_host(value: &str) -> bool {
    value.starts_with("localhost")
        || value.starts_with("127.")
        || value.starts_with("[::1]")
        || value.contains('.')
}

#[cfg(test)]
mod tests {
    use super::{
        classify_source, load_html, normalize_browser_url, resolve_reference,
        supports_text_response, SourceOrigin, DEFAULT_SOURCE,
    };
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
            let mut buffer = [0_u8; 2048];
            let _ = stream.read(&mut buffer);
            let request = String::from_utf8_lossy(&buffer).to_ascii_lowercase();
            assert!(request.contains("user-agent: toybrowser/0.1"));
            assert!(request.contains("accept: text/html"));

            let body = "<html><body><p>remote fixture</p></body></html>";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n{}",
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

    #[test]
    fn follows_redirects_and_returns_final_url() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("read local addr");

        let handle = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept request");
                let mut buffer = [0_u8; 1024];
                let bytes_read = stream.read(&mut buffer).expect("read request");
                let request = String::from_utf8_lossy(&buffer[..bytes_read]);

                if request.starts_with("GET /start ") {
                    let response = format!(
                        "HTTP/1.1 302 Found\r\nLocation: http://{address}/final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("write redirect");
                } else {
                    let body = "<html><body>redirected</body></html>";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("write final response");
                }
            }
        });

        let start_url = format!("http://{address}/start");
        let (html, path) = load_html(Some(&start_url)).expect("redirect should succeed");
        assert!(html.contains("redirected"));
        assert_eq!(path, PathBuf::from(format!("http://{address}/final")));

        handle.join().expect("server thread should finish");
    }

    #[test]
    fn rejects_binary_content_types() {
        assert!(supports_text_response("text/html; charset=utf-8"));
        assert!(supports_text_response("application/xhtml+xml"));
        assert!(!supports_text_response("application/pdf"));
    }

    #[test]
    fn resolves_local_relative_links() {
        assert_eq!(
            resolve_reference("examples/welcome.html", "guide/getting-started.html"),
            PathBuf::from("examples")
                .join("guide/getting-started.html")
                .display()
                .to_string()
        );
    }

    #[test]
    fn resolves_remote_relative_links() {
        assert_eq!(
            resolve_reference("https://example.com/docs/index.html", "../guide"),
            "https://example.com/guide"
        );
    }

    #[test]
    fn normalizes_remote_browser_urls() {
        assert_eq!(
            normalize_browser_url("example.com").expect("host should normalize"),
            "https://example.com/"
        );
        assert_eq!(
            normalize_browser_url("https://example.com/docs").expect("url should stay remote"),
            "https://example.com/docs"
        );
    }

    #[test]
    fn normalizes_local_browser_paths() {
        let url = normalize_browser_url("examples/welcome.html").expect("fixture should normalize");
        assert!(url.starts_with("file:///"));
        assert!(url.ends_with("/examples/welcome.html"));
    }
}
