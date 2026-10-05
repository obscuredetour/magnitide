//! This computer's addresses on the local network, for the address shown to
//! the user. VPN tunnels and virtual adapters are left out: a VPN's address
//! (10.2.0.2, say) is where the internet goes, not where an e-reader on the
//! same Wi-Fi can reach this computer.

use std::net::{IpAddr, Ipv4Addr};

#[derive(Debug, Default, PartialEq)]
pub struct Addresses {
    /// Best first: private addresses on real network adapters (Wi-Fi, Ethernet).
    pub lan: Vec<IpAddr>,
    /// A VPN or other tunnel is up, which may block connections from the LAN.
    pub tunnel: bool,
}

pub fn lan_addresses() -> Addresses {
    let interfaces = if_addrs::get_if_addrs().unwrap_or_default();
    choose(interfaces.iter().filter(|i| i.is_oper_up()).filter_map(|i| match &i.addr {
        if_addrs::IfAddr::V4(v4) => Some((i.name.as_str(), v4.ip, v4.prefixlen, i.is_p2p())),
        if_addrs::IfAddr::V6(_) => None,
    }))
}

/// Adapters that aren't the computer's own network connection, by name
/// (macOS/Linux names, and words in Windows' adapter names).
const VIRTUAL_PREFIXES: &[&str] = &[
    "utun",
    "tun",
    "tap",
    "wg",
    "ppp",
    "ipsec",
    "gif",
    "stf",
    "awdl",
    "llw",
    "bridge",
    "vmnet",
    "vboxnet",
    "docker",
    "veth",
    "br-",
    "virbr",
    "tailscale",
    "zt",
    "anpi",
    "ham",
];
const VIRTUAL_WORDS: &[&str] =
    &["vpn", "wireguard", "tailscale", "zerotier", "virtual", "vethernet", "hyper-v", "tunnel"];

fn is_virtual(name: &str, prefixlen: u8, p2p: bool) -> bool {
    let name = name.to_lowercase();
    // Tunnels are point-to-point, usually with a single-address (/32) mask.
    p2p || prefixlen == 32
        || VIRTUAL_PREFIXES.iter().any(|p| name.starts_with(p))
        || VIRTUAL_WORDS.iter().any(|w| name.contains(w))
}

fn choose<'a>(interfaces: impl Iterator<Item = (&'a str, Ipv4Addr, u8, bool)>) -> Addresses {
    let mut out = Addresses::default();
    let mut public = Vec::new();
    for (name, ip, prefixlen, p2p) in interfaces {
        if ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() {
            continue;
        }
        if is_virtual(name, prefixlen, p2p) {
            out.tunnel = true;
        } else if ip.is_private() {
            out.lan.push(IpAddr::V4(ip));
        } else {
            public.push(IpAddr::V4(ip));
        }
    }
    out.lan.extend(public);
    out.lan.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    #[test]
    fn skips_vpns_and_virtual_adapters() {
        // A Mac on Wi-Fi with a WireGuard VPN up (ProtonVPN's address).
        let got = choose(
            [
                ("lo0", ip("127.0.0.1"), 8, false),
                ("utun6", ip("10.2.0.2"), 32, true),
                ("en0", ip("10.69.1.58"), 24, false),
                ("bridge100", ip("192.168.64.1"), 24, false),
                ("en5", ip("169.254.3.4"), 16, false),
            ]
            .into_iter(),
        );
        assert_eq!(got, Addresses { lan: vec![IpAddr::V4(ip("10.69.1.58"))], tunnel: true });

        // Windows adapter names; private addresses before public ones.
        let got = choose(
            [
                ("ProtonVPN", ip("10.2.0.2"), 24, false),
                ("Ethernet", ip("203.0.113.9"), 24, false),
                ("Wi-Fi", ip("192.168.1.20"), 24, false),
            ]
            .into_iter(),
        );
        assert_eq!(got.lan, [IpAddr::V4(ip("192.168.1.20")), IpAddr::V4(ip("203.0.113.9"))]);
        assert!(got.tunnel);
    }
}
