//! SSRF egress guard for outbound webhook delivery (AP3, #80).
//!
//! A registered push webhook URL is attacker-influenced data — a client picks
//! it via `CreateTaskPushNotificationConfig`. Before the [`crate::serve`]
//! watcher POSTs a task update to it we:
//!
//! 1. require `https` (an `http` webhook is only accepted for loopback, a test
//!    affordance — see `allow_loopback`);
//! 2. resolve the host and **reject** if *any* resolved address is in a
//!    loopback / private / link-local / metadata / ULA range; and
//! 3. hand back the validated addresses so the caller can **pin** the
//!    connection to one of them — a DNS rebind can't then redirect the POST to
//!    an internal service between this check and the connect.
//!
//! The classifier is pure (`blocked_reason`) so the full range matrix is
//! unit-tested without touching the network.

use std::net::{IpAddr, Ipv6Addr, SocketAddr};

/// Why a webhook URL was rejected by the egress guard.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SsrfError {
    #[error("invalid webhook URL: {0}")]
    InvalidUrl(String),
    #[error("webhook scheme `{0}` not allowed — webhooks must be https (http only for loopback)")]
    Scheme(String),
    #[error("webhook host `{0}` did not resolve to any address")]
    NoAddress(String),
    #[error("webhook address {addr} is in a blocked range ({reason})")]
    Blocked { addr: IpAddr, reason: &'static str },
}

/// A webhook target that cleared the guard: the host plus every resolved
/// address (all validated), for the caller to pin the connection to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validated {
    pub host: String,
    pub addrs: Vec<SocketAddr>,
}

/// Why `ip` is unsafe as an egress target, or `None` if it is a routable public
/// address. Covers the ranges an SSRF would pivot through: loopback, private
/// (RFC 1918), CGNAT (RFC 6598), link-local (incl. the `169.254.169.254` cloud
/// metadata endpoint), broadcast/unspecified/multicast/documentation, and the
/// IPv6 equivalents (ULA `fc00::/7`, link-local `fe80::/10`, IPv4-mapped).
pub fn blocked_reason(ip: IpAddr) -> Option<&'static str> {
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_loopback() {
                Some("loopback")
            } else if v4.is_private() {
                Some("private (RFC 1918)")
            } else if is_cgnat(v4) {
                Some("CGNAT (100.64.0.0/10)")
            } else if v4.is_link_local() {
                Some("link-local (incl. cloud metadata 169.254.169.254)")
            } else if v4.is_unspecified() {
                Some("unspecified")
            } else if v4.is_broadcast() {
                Some("broadcast")
            } else if v4.is_documentation() {
                Some("documentation")
            } else if v4.is_multicast() {
                Some("multicast")
            } else {
                None
            }
        }
        IpAddr::V6(v6) => {
            // An IPv4-mapped address (`::ffff:a.b.c.d`) reaches the same host as
            // its embedded v4 — classify by that so a mapped-private can't slip
            // through the v6 arm.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return blocked_reason(IpAddr::V4(v4));
            }
            if v6.is_loopback() {
                Some("loopback")
            } else if v6.is_unspecified() {
                Some("unspecified")
            } else if is_ula(v6) {
                Some("unique-local (fc00::/7)")
            } else if is_v6_link_local(v6) {
                Some("link-local (fe80::/10)")
            } else if v6.is_multicast() {
                Some("multicast")
            } else {
                None
            }
        }
    }
}

/// `100.64.0.0/10` (carrier-grade NAT, RFC 6598) — not covered by std helpers.
fn is_cgnat(v4: std::net::Ipv4Addr) -> bool {
    let [a, b, ..] = v4.octets();
    a == 100 && (64..=127).contains(&b)
}

/// `fc00::/7` (IPv6 unique-local) — `Ipv6Addr::is_unique_local` is not stable.
fn is_ula(v6: Ipv6Addr) -> bool {
    v6.segments()[0] & 0xfe00 == 0xfc00
}

/// `fe80::/10` (IPv6 link-local) — `is_unicast_link_local` is not stable.
fn is_v6_link_local(v6: Ipv6Addr) -> bool {
    v6.segments()[0] & 0xffc0 == 0xfe80
}

/// Validate a webhook URL for egress. `allow_loopback` is a **test-only**
/// affordance: when `true`, loopback addresses (and `http`) are permitted so a
/// local mock server can be targeted; production passes `false`.
pub async fn validate(url: &str, allow_loopback: bool) -> Result<Validated, SsrfError> {
    let parsed = reqwest::Url::parse(url).map_err(|e| SsrfError::InvalidUrl(e.to_string()))?;
    match parsed.scheme() {
        "https" => {}
        "http" if allow_loopback => {}
        other => return Err(SsrfError::Scheme(other.to_string())),
    }
    // `host_str` keeps IPv6 literals bracketed (`[::1]`); strip for resolution.
    let host_raw = parsed
        .host_str()
        .ok_or_else(|| SsrfError::InvalidUrl("missing host".to_string()))?;
    let host = host_raw
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| SsrfError::InvalidUrl("missing port".to_string()))?;

    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|_| SsrfError::NoAddress(host.clone()))?
        .collect();
    if addrs.is_empty() {
        return Err(SsrfError::NoAddress(host));
    }
    for sa in &addrs {
        let ip = sa.ip();
        if allow_loopback && ip.is_loopback() {
            continue;
        }
        if let Some(reason) = blocked_reason(ip) {
            return Err(SsrfError::Blocked { addr: ip, reason });
        }
    }
    Ok(Validated { host, addrs })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(s.parse().unwrap())
    }
    fn v6(s: &str) -> IpAddr {
        IpAddr::V6(s.parse().unwrap())
    }

    #[test]
    fn blocks_the_unsafe_v4_ranges() {
        assert_eq!(blocked_reason(v4("127.0.0.1")), Some("loopback"));
        assert_eq!(blocked_reason(v4("10.0.0.1")), Some("private (RFC 1918)"));
        assert_eq!(blocked_reason(v4("172.16.5.4")), Some("private (RFC 1918)"));
        assert_eq!(
            blocked_reason(v4("192.168.1.1")),
            Some("private (RFC 1918)")
        );
        assert_eq!(
            blocked_reason(v4("100.64.0.1")),
            Some("CGNAT (100.64.0.0/10)")
        );
        assert_eq!(
            blocked_reason(v4("169.254.169.254")),
            Some("link-local (incl. cloud metadata 169.254.169.254)")
        );
        assert_eq!(blocked_reason(v4("0.0.0.0")), Some("unspecified"));
        assert_eq!(blocked_reason(v4("255.255.255.255")), Some("broadcast"));
    }

    #[test]
    fn blocks_the_unsafe_v6_ranges() {
        assert_eq!(blocked_reason(v6("::1")), Some("loopback"));
        assert_eq!(blocked_reason(v6("::")), Some("unspecified"));
        assert_eq!(
            blocked_reason(v6("fc00::1")),
            Some("unique-local (fc00::/7)")
        );
        assert_eq!(
            blocked_reason(v6("fd12::1")),
            Some("unique-local (fc00::/7)")
        );
        assert_eq!(
            blocked_reason(v6("fe80::1")),
            Some("link-local (fe80::/10)")
        );
        // IPv4-mapped private is classified by the embedded v4, not let through.
        assert_eq!(
            blocked_reason(v6("::ffff:10.0.0.1")),
            Some("private (RFC 1918)")
        );
    }

    #[test]
    fn allows_public_addresses() {
        assert_eq!(blocked_reason(v4("93.184.216.34")), None); // example.com
        assert_eq!(blocked_reason(v4("8.8.8.8")), None);
        assert_eq!(
            blocked_reason(v6("2606:2800:220:1:248:1893:25c8:1946")),
            None
        );
    }

    #[tokio::test]
    async fn validate_rejects_http_for_non_loopback() {
        let err = validate("http://93.184.216.34/hook", false)
            .await
            .unwrap_err();
        assert!(matches!(err, SsrfError::Scheme(s) if s == "http"));
    }

    #[tokio::test]
    async fn validate_rejects_private_and_metadata_targets() {
        for url in [
            "https://10.0.0.1/hook",
            "https://169.254.169.254/latest/meta-data",
            "https://[::1]/hook",
            "https://192.168.0.5/x",
        ] {
            let err = validate(url, false).await.unwrap_err();
            assert!(matches!(err, SsrfError::Blocked { .. }), "{url}: {err:?}");
        }
    }

    #[tokio::test]
    async fn validate_accepts_public_https_literal() {
        let ok = validate("https://93.184.216.34/hook", false).await.unwrap();
        assert_eq!(ok.host, "93.184.216.34");
        assert!(!ok.addrs.is_empty());
    }

    #[tokio::test]
    async fn validate_loopback_only_with_the_test_affordance() {
        assert!(validate("http://127.0.0.1:8080/hook", false).await.is_err());
        let ok = validate("http://127.0.0.1:8080/hook", true).await.unwrap();
        assert_eq!(ok.host, "127.0.0.1");
    }

    #[tokio::test]
    async fn validate_rejects_garbage_url() {
        assert!(matches!(
            validate("not a url", false).await.unwrap_err(),
            SsrfError::InvalidUrl(_)
        ));
    }
}
