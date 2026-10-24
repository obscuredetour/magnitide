//! Whether this computer's firewall may be keeping devices out of the server,
//! and the command that lets them in. Linux only: there a firewall that drops
//! incoming connections is common (ufw, firewalld) and says nothing, so a
//! device's request just times out. macOS asks the user itself, and Windows
//! asks when the server first listens.
//!
//! Read without root where possible: ufw's settings and rules are files
//! (usually readable), firewalld answers queries over D-Bus. When the rules
//! can't be read, the hint is shown anyway; Settings words it as "if devices
//! can't connect", so a firewall that does allow the port costs a sentence.

use std::process::{Command, Stdio};

/// A firewall that may block the server, and how to open its port.
#[derive(Debug, PartialEq)]
pub struct FirewallHint {
    /// "ufw" or "firewalld".
    pub name: &'static str,
    /// The command that allows the port, from `network` when known.
    pub command: String,
}

/// The hint for the server on `port`, with devices on `network` ("192.168.1.0/24").
pub fn firewall_hint(port: u16, network: Option<&str>) -> Option<FirewallHint> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    ufw(port, network).or_else(|| firewalld(port))
}

fn ufw(port: u16, network: Option<&str>) -> Option<FirewallHint> {
    let conf = std::fs::read_to_string("/etc/ufw/ufw.conf").ok()?;
    if !conf.lines().any(|l| l.trim() == "ENABLED=yes") || !service_active("ufw") {
        return None;
    }
    let policy = std::fs::read_to_string("/etc/default/ufw").unwrap_or_default();
    if policy.lines().any(|l| l.trim() == r#"DEFAULT_INPUT_POLICY="ACCEPT""#) {
        return None;
    }
    // Unreadable (some distributions keep the rules root-only): can't tell, so say.
    if std::fs::read_to_string("/etc/ufw/user.rules").is_ok_and(|rules| ufw_allows(&rules, port)) {
        return None;
    }
    let command = match network {
        Some(net) => format!("sudo ufw allow from {net} to any port {port} proto tcp"),
        None => format!("sudo ufw allow {port}/tcp"),
    };
    Some(FirewallHint { name: "ufw", command })
}

/// Whether ufw's user rules (iptables-restore lines) accept TCP connections to `port`.
fn ufw_allows(rules: &str, port: u16) -> bool {
    rules.lines().any(|line| {
        let words: Vec<&str> = line.split_whitespace().collect();
        let after = |flag: &str| words.iter().position(|w| *w == flag).and_then(|i| words.get(i + 1)).copied();
        words.first() == Some(&"-A")
            && words.get(1) == Some(&"ufw-user-input")
            && after("-j") == Some("ACCEPT")
            && after("-p") != Some("udp")
            && after("--dport").or_else(|| after("--dports")).is_some_and(|ports| in_ports(ports, port))
    })
}

/// Whether `ports` ("8180", "8000:8200", "80,443,8180") includes `port`.
fn in_ports(ports: &str, port: u16) -> bool {
    ports.split(',').any(|p| match p.split_once(':') {
        Some((lo, hi)) => lo.parse().is_ok_and(|lo: u16| hi.parse().is_ok_and(|hi: u16| (lo..=hi).contains(&port))),
        None => p.parse() == Ok(port),
    })
}

fn firewalld(port: u16) -> Option<FirewallHint> {
    if !succeeds("firewall-cmd", &["--state"]) {
        return None;
    }
    // Includes port ranges (Fedora Workstation opens 1025–65535) and the default zone's services.
    if succeeds("firewall-cmd", &[&format!("--query-port={port}/tcp")]) {
        return None;
    }
    let command = format!("sudo firewall-cmd --permanent --add-port={port}/tcp && sudo firewall-cmd --reload");
    Some(FirewallHint { name: "firewalld", command })
}

/// Whether systemd says `unit` is running; true without systemd, which can't say.
fn service_active(unit: &str) -> bool {
    match Command::new("systemctl").args(["is-active", "--quiet", unit]).stdin(Stdio::null()).status() {
        Ok(s) => s.success(),
        Err(_) => true,
    }
}

fn succeeds(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ufw_rules() {
        // What `ufw allow from 10.69.1.0/24 to any port 8180 proto tcp` writes, among others.
        let rules = "\
*filter
:ufw-user-input - [0:0]
### tuple ### allow tcp 8180 0.0.0.0/0 any 10.69.1.0/24 in
-A ufw-user-input -p tcp --dport 8180 -s 10.69.1.0/24 -j ACCEPT
-A ufw-user-input -p udp --dport 9000 -j ACCEPT
-A ufw-user-input -p tcp -m multiport --dports 6000:6010,7000 -j ACCEPT
-A ufw-user-input -p tcp --dport 5000 -j DROP
-A ufw-user-output -p tcp --dport 4000 -j ACCEPT
COMMIT";
        assert!(ufw_allows(rules, 8180));
        assert!(ufw_allows(rules, 6005));
        assert!(ufw_allows(rules, 7000));
        assert!(!ufw_allows(rules, 9000), "udp only");
        assert!(!ufw_allows(rules, 5000), "denied");
        assert!(!ufw_allows(rules, 4000), "outgoing");
        assert!(!ufw_allows(rules, 8181));
        // `ufw allow 8190` (no protocol) is a tcp line and a udp line.
        assert!(ufw_allows("-A ufw-user-input -p tcp --dport 8190 -j ACCEPT", 8190));
    }

    #[test]
    fn port_lists_and_ranges() {
        assert!(in_ports("8180", 8180));
        assert!(in_ports("80,443,8180", 8180));
        assert!(in_ports("8000:8200", 8180));
        assert!(!in_ports("8000:8100,8200", 8180));
        assert!(!in_ports("junk", 8180));
    }
}
