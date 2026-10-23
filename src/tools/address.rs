//! Shared classification of URL literals, without resolving provider-side fetches.
use std::net::IpAddr;
use url::{Host, Url};

pub fn literal(url: &Url) -> Option<IpAddr> {
    match url.host()? {
        Host::Ipv4(ip) => Some(IpAddr::V4(ip)),
        Host::Ipv6(ip) => Some(normalize(IpAddr::V6(ip))),
        Host::Domain(_) => None,
    }
}

pub fn is_localhost(host: &str) -> bool {
    let host = host.trim_end_matches('.');
    host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".localhost")
}

pub fn is_loopback(ip: IpAddr) -> bool {
    normalize(ip).is_loopback()
}

pub fn is_public(ip: IpAddr) -> bool {
    match normalize(ip) {
        IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_broadcast())
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || (segments[0] & 0xfe00) == 0xfc00 // Unique local: fc00::/7.
                || (segments[0] & 0xffc0) == 0xfe80) // Link local: fe80::/10.
        }
    }
}

fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        // An IPv6 spelling must not hide an IPv4 loopback, LAN or metadata target.
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(ip)),
        ip => ip,
    }
}
