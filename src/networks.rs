//! The computer's network connections (Wi-Fi, Ethernet, a phone over USB…), for downloading over
//! several at once.

use std::net::IpAddr;

#[derive(Clone, Debug, PartialEq)]
pub struct Network {
    /// The interface: en0 on macOS, wlan0 on Linux, "Wi-Fi" on Windows.
    pub name: String,
    /// What people call it: Wi-Fi, Ethernet, iPhone USB.
    pub label: String,
    pub ip: IpAddr,
}

/// The connections that could reach the internet: up, not loopback, with a routable address, and
/// not a tunnel or virtual adapter. One per interface, preferring its IPv4 address.
pub fn list() -> Vec<Network> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else { return Vec::new() };
    let labels = labels();
    let mut out: Vec<Network> = Vec::new();
    for i in interfaces {
        let ip = i.ip();
        let routable = match ip {
            IpAddr::V4(v4) => !v4.is_link_local() && !v4.is_unspecified(),
            IpAddr::V6(v6) => !v6.is_unicast_link_local() && !v6.is_unspecified(),
        };
        if i.is_loopback() || !i.is_oper_up() || !routable || virtual_adapter(&i.name) {
            continue;
        }
        if let Some(seen) = out.iter_mut().find(|n| n.name == i.name) {
            if seen.ip.is_ipv6() && ip.is_ipv4() {
                seen.ip = ip;
            }
            continue;
        }
        let label = labels.iter().find(|(device, _)| *device == i.name).map_or_else(|| i.name.clone(), |(_, l)| l.clone());
        out.push(Network { name: i.name, label, ip });
    }
    out
}

/// Tunnels (VPNs), bridges and virtual machines' adapters: not separate ways out.
fn virtual_adapter(name: &str) -> bool {
    const PREFIXES: [&str; 18] = [
        "utun", "awdl", "llw", "bridge", "gif", "stf", "anpi", "ap1", "vmnet", "vboxnet", "docker", "veth", "br-", "virbr", "tun",
        "tap", "wg", "tailscale",
    ];
    PREFIXES.iter().any(|p| name.starts_with(p))
}

/// macOS's names for its interfaces ("Wi-Fi" for en0), from `networksetup`; elsewhere the
/// interface's own name is what people see (Windows already says "Wi-Fi").
fn labels() -> Vec<(String, String)> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    let Ok(out) = std::process::Command::new("networksetup").arg("-listallhardwareports").output() else { return Vec::new() };
    parse_hardware_ports(&String::from_utf8_lossy(&out.stdout))
}

/// (device, port) pairs from `networksetup -listallhardwareports`.
fn parse_hardware_ports(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut port = None;
    for line in text.lines() {
        if let Some(p) = line.strip_prefix("Hardware Port: ") {
            port = Some(p.trim().to_string());
        } else if let (Some(device), Some(p)) = (line.strip_prefix("Device: "), port.take()) {
            out.push((device.trim().to_string(), p));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_networksetup() {
        let text = "\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: aa\n\nHardware Port: iPhone USB\nDevice: en5\nEthernet Address: bb\n";
        assert_eq!(
            parse_hardware_ports(text),
            [("en0".to_string(), "Wi-Fi".to_string()), ("en5".to_string(), "iPhone USB".to_string())]
        );
        assert!(virtual_adapter("utun3") && virtual_adapter("awdl0") && !virtual_adapter("en0") && !virtual_adapter("wlan0"));
        // Whatever this machine has, the list never includes loopback.
        assert!(list().iter().all(|n| !n.ip.is_loopback()));
    }
}
