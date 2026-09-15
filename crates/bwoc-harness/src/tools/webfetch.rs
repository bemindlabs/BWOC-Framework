//! `webfetch` — GET an http(s) URL and return its text.
//!
//! Network egress, so it is never pure-read: `ask` under the chat default
//! policy, `Gated` for the Layer-0 capability gate, and kept out of plan mode
//! (public connector sessions run in plan mode).
//!
//! Registered for `--chat` sessions only and run in-process. The isolated
//! turn-executor child installs a seccomp filter that kills the process on
//! `socket(2)`, so this tool must never be marshalled into it — which is why it
//! is not part of `default_registry`.
//!
//! Loopback, private, link-local and CGNAT literals (and `localhost`) are
//! refused, including on redirect. A public DNS name that resolves to a private
//! address is not caught; the permission prompt is the gate for that.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::OnceLock;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use regex::Regex;
use serde_json::{Value, json};

use super::{ToolContext, ToolImpl};
use crate::error::HarnessError;

const TIMEOUT_SECS: u64 = 30;
/// Body bytes read before stopping. The dispatch seam still clamps the text
/// returned to the model to `MAX_TOOL_OUTPUT_BYTES`.
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

#[derive(Default)]
pub struct WebFetch {
    /// Test-only escape hatch for a loopback mock server.
    allow_local: bool,
}

#[async_trait]
impl ToolImpl for WebFetch {
    fn name(&self) -> &'static str {
        "webfetch"
    }

    fn description(&self) -> &'static str {
        "Fetch a web page or document over http(s) with GET and return its text. \
         HTML is converted to readable text, other text types are returned as-is, \
         binary content is not returned. The body is capped at 1 MB and the request \
         at 30 seconds. Local, private and link-local addresses are refused. Reaches \
         the network, so it needs permission."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Absolute http:// or https:// URL."
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<String, HarnessError> {
        let url = args["url"]
            .as_str()
            .ok_or_else(|| err("missing `url` argument"))?;
        let parsed =
            reqwest::Url::parse(url).map_err(|e| err(format!("invalid url `{url}`: {e}")))?;
        check_target(&parsed, self.allow_local)?;
        fetch(parsed, self.allow_local).await
    }
}

fn err(reason: impl Into<String>) -> HarnessError {
    HarnessError::ToolExecution {
        tool: "webfetch".to_string(),
        reason: reason.into(),
    }
}

fn check_target(url: &reqwest::Url, allow_local: bool) -> Result<(), HarnessError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(err(format!(
            "only http and https URLs are allowed, got `{}`",
            url.scheme()
        )));
    }
    if !allow_local && is_local_host(url) {
        return Err(err(format!(
            "refusing to fetch a local or private address: `{}`",
            url.host_str().unwrap_or("")
        )));
    }
    Ok(())
}

/// `localhost`, or an IP literal in a loopback / private / link-local /
/// unspecified / CGNAT range. (The URL parser already normalizes forms such as
/// `http://2130706433/` to `127.0.0.1`.)
fn is_local_host(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return true;
    };
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => is_local_v4(ip),
        Ok(IpAddr::V6(ip)) => {
            let first = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                || (first & 0xfe00) == 0xfc00 // unique local
                || (first & 0xffc0) == 0xfe80 // link local
                || ip.to_ipv4_mapped().is_some_and(is_local_v4)
        }
        Err(_) => false,
    }
}

fn is_local_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || (a == 100 && (64..128).contains(&b)) // CGNAT 100.64.0.0/10
}

async fn fetch(url: reqwest::Url, allow_local: bool) -> Result<String, HarnessError> {
    let policy = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else if check_target(attempt.url(), allow_local).is_err() {
            attempt.error("redirect to a disallowed URL (not http(s), or a local address)")
        } else {
            attempt.follow()
        }
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .redirect(policy)
        .user_agent(concat!("bwoc-harness/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| err(format!("cannot build HTTP client: {e}")))?;

    let resp = client
        .get(url.clone())
        .send()
        .await
        .map_err(|e| err(format!("request to `{url}` failed: {e}")))?;
    let status = resp.status();
    let final_url = resp.url().to_string();
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !is_textual(&ctype) {
        return Ok(format!(
            "[{status}] {final_url}\n\n[bwoc: content-type `{ctype}` is not text; body not shown]"
        ));
    }

    let mut body = Vec::new();
    let mut truncated = false;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| err(format!("reading `{final_url}` failed: {e}")))?;
        let room = MAX_BODY_BYTES - body.len();
        if chunk.len() > room {
            body.extend_from_slice(&chunk[..room]);
            truncated = true;
            break;
        }
        body.extend_from_slice(&chunk);
    }

    // A missing or wrong content-type can still carry binary: sniff for NUL.
    if body[..body.len().min(BINARY_SNIFF_BYTES)].contains(&0) {
        return Ok(format!(
            "[{status}] {final_url}\n\n[bwoc: body looks binary (NUL bytes); not shown]"
        ));
    }
    let raw = String::from_utf8_lossy(&body);
    let text = if ctype.contains("html") {
        html_to_text(&raw)
    } else {
        raw.into_owned()
    };
    let note = if truncated {
        format!("\n\n[bwoc: body truncated at {MAX_BODY_BYTES} bytes]")
    } else {
        String::new()
    };
    Ok(format!("[{status}] {final_url}\n\n{text}{note}"))
}

/// Leading bytes checked for NUL before a body is treated as text.
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

fn is_textual(ctype: &str) -> bool {
    ctype.is_empty()
        || ctype.starts_with("text/")
        || ["json", "xml", "javascript", "yaml", "toml"]
            .iter()
            .any(|t| ctype.contains(t))
}

/// Readable text from HTML: drop script/style/noscript/comments, turn block
/// boundaries into newlines and list items into `- `, strip remaining tags,
/// decode entities, collapse whitespace. Not a renderer — enough to read docs.
fn html_to_text(html: &str) -> String {
    static RES: OnceLock<[Regex; 5]> = OnceLock::new();
    let [hidden, block, item, tag, blank] = RES.get_or_init(|| {
        [
            Regex::new(
                r"(?is)<script\b.*?</script\s*>|<style\b.*?</style\s*>|<noscript\b.*?</noscript\s*>|<!--.*?-->",
            )
            .expect("static regex"),
            Regex::new(
                r"(?i)<(?:br|/?p|/?div|/?h[1-6]|/tr|/?pre|/blockquote|/?section|/?article|/?ul|/?ol|/?table|/header|/footer)\b[^>]*>",
            )
            .expect("static regex"),
            Regex::new(r"(?i)<li\b[^>]*>").expect("static regex"),
            Regex::new(r"(?s)<[^>]*>").expect("static regex"),
            Regex::new(r"\n{3,}").expect("static regex"),
        ]
    });
    let s = hidden.replace_all(html, "");
    let s = block.replace_all(&s, "\n");
    let s = item.replace_all(&s, "\n- ");
    let s = tag.replace_all(&s, "");
    let s = decode_entities(&s);
    let s = s
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    blank.replace_all(s.trim(), "\n\n").into_owned()
}

/// Decode named (`&amp;` `&lt;` `&gt;` `&quot;` `&apos;` `&nbsp;`) and numeric
/// entities in one pass, so `&amp;lt;` becomes `&lt;`, not `<`.
fn decode_entities(s: &str) -> String {
    static ENTITY: OnceLock<Regex> = OnceLock::new();
    let re = ENTITY.get_or_init(|| {
        Regex::new(r"&(#[0-9]{1,7}|#[xX][0-9a-fA-F]{1,6}|[a-zA-Z]+);").expect("static regex")
    });
    re.replace_all(s, |c: &regex::Captures| {
        let e = &c[1];
        let decoded = if let Some(hex) = e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")) {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        } else if let Some(dec) = e.strip_prefix('#') {
            dec.parse().ok().and_then(char::from_u32)
        } else {
            match e {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => None,
            }
        };
        decoded.map_or_else(|| c[0].to_string(), String::from)
    })
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ctx() -> ToolContext {
        ToolContext::new(std::env::temp_dir())
    }

    fn url(s: &str) -> reqwest::Url {
        reqwest::Url::parse(s).unwrap()
    }

    #[test]
    fn refuses_local_and_private_targets() {
        for local in [
            "http://localhost/",
            "http://api.localhost:8080/",
            "http://127.0.0.1/",
            "http://2130706433/",
            "http://10.1.2.3/",
            "http://192.168.1.113:30400/",
            "http://172.17.0.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://100.99.8.60:30300/",
            "http://0.0.0.0/",
            "http://[::1]/",
            "http://[fd00::1]/",
            "http://[fe80::1]/",
            "http://[::ffff:127.0.0.1]/",
        ] {
            assert!(is_local_host(&url(local)), "{local} must be refused");
            assert!(check_target(&url(local), false).is_err());
        }
        for public in [
            "https://example.com/",
            "http://8.8.8.8/",
            "https://[2606:4700::1111]/",
        ] {
            assert!(!is_local_host(&url(public)), "{public} is public");
        }
    }

    #[tokio::test]
    async fn refuses_non_http_schemes() {
        for bad in ["file:///etc/passwd", "ftp://example.com/x", "gopher://x/"] {
            let e = WebFetch::default()
                .execute(json!({ "url": bad }), &ctx())
                .await
                .unwrap_err();
            assert!(e.to_string().contains("only http and https"), "{bad}: {e}");
        }
        let e = WebFetch::default()
            .execute(json!({ "url": "not a url" }), &ctx())
            .await
            .unwrap_err();
        assert!(e.to_string().contains("invalid url"), "{e}");
    }

    #[test]
    fn html_to_text_is_readable() {
        let html = "<html><head><style>p{color:red}</style><script>evil()</script></head>\
                    <body><!-- hidden --><h1>Title</h1><p>Hello &amp; welcome&nbsp;here</p>\
                    <ul><li>one</li><li>two &lt;b&gt;</li></ul><p>&#x41;&#66; &amp;lt;</p></body></html>";
        let text = html_to_text(html);
        assert!(!text.contains("evil") && !text.contains("color") && !text.contains("hidden"));
        assert!(!text.contains("<p>"), "{text}");
        assert!(text.contains("Title\n"), "{text}");
        assert!(text.contains("Hello & welcome here"), "{text}");
        assert!(text.contains("- one\n- two <b>"), "{text}");
        assert!(text.contains("AB &lt;"), "single-pass decode: {text}");
    }

    async fn serve(body: Vec<u8>, mime: &str) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/doc"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, mime))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn fetches_html_as_text() {
        let server = serve(b"<p>Hello &amp; bye</p>".to_vec(), "text/html").await;
        let tool = WebFetch { allow_local: true };
        let out = tool
            .execute(json!({ "url": format!("{}/doc", server.uri()) }), &ctx())
            .await
            .unwrap();
        assert!(out.starts_with("[200 OK]"), "{out}");
        assert!(out.ends_with("Hello & bye"), "{out}");
        // The default tool refuses the same loopback mock.
        let e = WebFetch::default()
            .execute(json!({ "url": format!("{}/doc", server.uri()) }), &ctx())
            .await
            .unwrap_err();
        assert!(e.to_string().contains("local or private"), "{e}");
    }

    #[tokio::test]
    async fn caps_the_body_and_skips_binary() {
        let tool = WebFetch { allow_local: true };
        let big = serve(vec![b'a'; MAX_BODY_BYTES + 10], "text/plain").await;
        let out = tool
            .execute(json!({ "url": format!("{}/doc", big.uri()) }), &ctx())
            .await
            .unwrap();
        assert!(out.contains("body truncated"), "{}", &out[out.len() - 80..]);
        let png = serve(vec![0x89, b'P', b'N', b'G'], "image/png").await;
        let out = tool
            .execute(json!({ "url": format!("{}/doc", png.uri()) }), &ctx())
            .await
            .unwrap();
        assert!(out.contains("is not text"), "{out}");
        // Binary under a textual (or missing) content-type is sniffed out.
        let lying = serve(b"GIF89a\x00\x01\x02 not text".to_vec(), "text/plain").await;
        let out = tool
            .execute(json!({ "url": format!("{}/doc", lying.uri()) }), &ctx())
            .await
            .unwrap();
        assert!(out.contains("looks binary"), "{out}");
    }
}
