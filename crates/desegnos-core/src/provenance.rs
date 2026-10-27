//! Found-content metadata is private to the editor; visible credits stay public.
use crate::{Result, ensure};
use std::net::IpAddr;
use url::{Host, Url};

pub const FOUND_CONTENT_ATTRIBUTE: &str = "data-desegnos-found";

pub fn validate_found_source(value: &str) -> Result<()> {
    ensure(
        value.len() <= 4096 && !value.chars().any(char::is_control),
        "The found-content source address is invalid.",
    )?;
    let url = Url::parse(value)
        .map_err(|_| crate::Error("Use a complete public HTTPS source address.".into()))?;
    ensure(
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port_or_known_default() == Some(443),
        "Use a public HTTPS source address without credentials or a private port.",
    )?;
    let public = match url.host() {
        Some(Host::Domain(name)) => {
            name.contains('.')
                && !name.ends_with('.')
                && !name.ends_with(".localhost")
                && !name.ends_with(".local")
                && !name.ends_with(".internal")
                && name != "localhost"
        }
        Some(Host::Ipv4(ip)) => public_ip(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => public_ip(IpAddr::V6(ip)),
        None => false,
    };
    ensure(
        public,
        "Found content must link to a public website, not a local address.",
    )
}
fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !matches!(a, 0 | 10 | 127 | 224..=255)
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 169 && b == 254)
                && !(a == 172 && (16..=31).contains(&b))
                && !(a == 192 && (b == 168 || b == 0 || (b == 2 && c == 0)))
                && !(a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                && !(a == 203 && b == 0 && c == 113)
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            s[0] & 0xe000 == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}
