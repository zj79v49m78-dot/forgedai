//! Network & ping.
//!
//! Two honest caveats framed here rather than buried in the report:
//!
//! * Nothing in this section reduces the physical distance to Epic's servers.
//!   Geography sets the floor; these changes remove the software overhead that
//!   sits on top of it, and eliminate *jitter*, which is what actually makes
//!   hit registration feel inconsistent.
//! * Two very popular "ping fixes" — the QoS reserved-bandwidth key and
//!   disabling TCP autotuning — do nothing and cause harm respectively. Both
//!   appear here, correctly labelled, and one of them is an entry that *undoes*
//!   the bad advice.

use crate::hardware::HardwareProfile;
use crate::tweaks::model::*;

/// Nagle's algorithm is disabled per-interface, keyed by adapter GUID.
///
/// Guides tell people to apply this to every interface subkey. Forged targets
/// only the adapter carrying the default route, because writing it to a VPN or
/// virtual adapter that later becomes primary produces confusing results.
fn nagle_actions(profile: &HardwareProfile) -> Vec<Action> {
    let Some(adapter) = profile.active_adapter() else {
        return Vec::new();
    };
    if adapter.guid.is_empty() {
        return Vec::new();
    }
    let path = format!(
        r"SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces\{}",
        adapter.guid
    );
    vec![
        hklm_dword(&path, "TcpAckFrequency", 1),
        hklm_dword(&path, "TCPNoDelay", 1),
        hklm_dword(&path, "TcpDelAckTicks", 0),
    ]
}

/// Advanced NIC properties live under the network class key, indexed by a
/// four-digit subkey the scanner resolves.
fn nic_class_path(profile: &HardwareProfile) -> Option<String> {
    let adapter = profile.active_adapter()?;
    let index = adapter.class_key_index.as_ref()?;
    Some(format!(
        r"SYSTEM\CurrentControlSet\Control\Class\{{4d36e972-e325-11ce-bfc1-08002be10318}}\{index}"
    ))
}

fn offload_actions(profile: &HardwareProfile) -> Vec<Action> {
    let Some(path) = nic_class_path(profile) else {
        return Vec::new();
    };
    // Driver-independent property names. A NIC that does not expose a given
    // property simply ends up with an unused registry value, which is inert.
    vec![
        hklm_sz(&path, "*LsoV2IPv4", "0"),
        hklm_sz(&path, "*LsoV2IPv6", "0"),
        hklm_sz(&path, "*RscIPv4", "0"),
        hklm_sz(&path, "*RscIPv6", "0"),
    ]
}

fn interrupt_moderation_actions(profile: &HardwareProfile) -> Vec<Action> {
    let Some(path) = nic_class_path(profile) else {
        return Vec::new();
    };
    vec![
        hklm_sz(&path, "*InterruptModeration", "0"),
        hklm_sz(&path, "ITR", "0"),
    ]
}

fn eee_actions(profile: &HardwareProfile) -> Vec<Action> {
    let Some(path) = nic_class_path(profile) else {
        return Vec::new();
    };
    vec![
        hklm_sz(&path, "*EEE", "0"),
        hklm_sz(&path, "EnableGreenEthernet", "0"),
        hklm_sz(&path, "AdvancedEEE", "0"),
        hklm_sz(&path, "*FlowControl", "0"),
    ]
}

fn nic_power_actions(profile: &HardwareProfile) -> Vec<Action> {
    let Some(path) = nic_class_path(profile) else {
        return Vec::new();
    };
    // PnPCapabilities 24 clears both "allow the computer to turn off this
    // device" and "allow this device to wake the computer".
    vec![hklm_dword(&path, "PnPCapabilities", 24)]
}

pub static TWEAKS: &[Tweak] = &[
    Tweak {
        id: "net.disable_nagle",
        name: "Disable Nagle's algorithm",
        section: Section::Network,
        summary: "Sets TcpAckFrequency and TCPNoDelay on the adapter carrying your default route \
                  so small packets are sent immediately.",
        rationale: "Nagle batches small outbound packets to save bandwidth, holding them up to \
                    200 ms waiting for company. Your movement and fire inputs are exactly those \
                    small packets. Disabling it trades a negligible amount of bandwidth for \
                    consistently prompt delivery.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: None,
        applies_to: |p| p.active_adapter().is_some_and(|a| !a.guid.is_empty()),
        build: nagle_actions,
    },
    Tweak {
        id: "net.throttling_index",
        name: "Remove the multimedia network throttle",
        section: Section::Network,
        summary: "Sets NetworkThrottlingIndex to disabled.",
        rationale: "By default Windows caps non-multimedia network traffic at around 10,000 \
                    packets per second while any multimedia app is running — which includes your \
                    game. Removing the cap is one of the few genuinely large network wins available.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![hklm_dword(
                r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile",
                "NetworkThrottlingIndex",
                0xFFFF_FFFF,
            )]
        },
    },
    Tweak {
        id: "net.system_responsiveness",
        name: "Reduce reserved CPU for background multimedia",
        section: Section::Network,
        summary: "Lowers SystemResponsiveness from the default 20% to 10%.",
        rationale: "This value reserves a slice of CPU for background multimedia tasks. Most \
                    guides set it to 0. Forged uses 10 deliberately: at 0 the audio subsystem is \
                    starved under load and produces crackling, which is a worse outcome than the \
                    frame or two it buys back.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![hklm_dword(
                r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile",
                "SystemResponsiveness",
                10,
            )]
        },
    },
    Tweak {
        id: "net.disable_interrupt_moderation",
        name: "Disable NIC interrupt moderation",
        section: Section::Network,
        summary: "Stops the network card batching interrupts before notifying the CPU.",
        rationale: "Interrupt moderation exists to reduce CPU load on servers pushing gigabits. \
                    On a gaming machine it adds a small, variable delay to every received packet — \
                    which is jitter, the thing that makes hit registration feel random.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: Some("Marginally higher CPU usage under heavy network load. Irrelevant on a \
                        modern CPU running one game."),
        applies_to: |p| nic_class_path(p).is_some(),
        build: interrupt_moderation_actions,
    },
    Tweak {
        id: "net.disable_rsc_lso",
        name: "Disable receive coalescing and large send offload",
        section: Section::Network,
        summary: "Turns off RSC and LSO on the active adapter.",
        rationale: "Both features merge packets to reduce per-packet overhead, and both do so by \
                    holding packets briefly. Throughput-oriented optimisations are latency \
                    pessimisations, and Fortnite cares about latency.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: |p| nic_class_path(p).is_some(),
        build: offload_actions,
    },
    Tweak {
        id: "net.disable_energy_efficient_ethernet",
        name: "Disable Energy Efficient Ethernet and flow control",
        section: Section::Network,
        summary: "Turns off EEE / Green Ethernet and 802.3x flow control on the active adapter.",
        rationale: "EEE drops the link into a low-power idle between bursts and takes microseconds \
                    to tens of microseconds to come back — per idle period. Flow control lets the \
                    switch pause your traffic entirely. Neither belongs on a gaming link.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: Some("Slightly higher idle power draw from the network card, on the order of a \
                        watt."),
        applies_to: |p| nic_class_path(p).is_some(),
        build: eee_actions,
    },
    Tweak {
        id: "net.nic_power_management",
        name: "Stop Windows powering down the network card",
        section: Section::Network,
        summary: "Clears the adapter's power management capability flags.",
        rationale: "A NIC allowed to sleep will do so during a loading screen and cost you the \
                    first exchange after the drop. There is no power saving worth having on a \
                    desktop that exists to play one game.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: |p| nic_class_path(p).is_some(),
        build: nic_power_actions,
    },
    Tweak {
        id: "net.restore_tcp_autotuning",
        name: "Restore TCP receive-window autotuning",
        section: Section::Network,
        summary: "Sets the TCP autotuning level back to 'normal'.",
        rationale: "Disabling autotuning is one of the most-repeated pieces of bad advice online. \
                    It caps the receive window at a 1990s default, which throttles throughput on \
                    any modern connection and makes downloads and streaming worse without \
                    improving latency at all. This entry exists to undo it.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![command(
                "netsh.exe",
                &["int", "tcp", "set", "global", "autotuninglevel=normal"],
                &["int", "tcp", "set", "global", "autotuninglevel=normal"],
            )]
        },
    },
    Tweak {
        id: "net.enable_rss",
        name: "Enable Receive Side Scaling",
        section: Section::Network,
        summary: "Turns on RSS so received packets are processed across multiple CPU cores.",
        rationale: "Without RSS every incoming packet is handled by core 0. If core 0 is also \
                    busy with the game's render thread, network processing waits behind it. \
                    Spreading the work removes that contention.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: |p| p.cpu.logical_threads > 2,
        build: |_| {
            vec![command(
                "netsh.exe",
                &["int", "tcp", "set", "global", "rss=enabled"],
                &["int", "tcp", "set", "global", "rss=enabled"],
            )]
        },
    },
    Tweak {
        id: "net.disable_ecn",
        name: "Disable Explicit Congestion Notification",
        section: Section::Network,
        summary: "Turns off ECN negotiation on outbound connections.",
        rationale: "ECN requires cooperation from every hop. Consumer routers frequently mishandle \
                    ECN-marked packets and drop or delay them, which presents as unexplained \
                    packet loss. Disabling it removes a failure mode you cannot otherwise diagnose.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::SituationalGain,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![command(
                "netsh.exe",
                &["int", "tcp", "set", "global", "ecncapability=disabled"],
                &["int", "tcp", "set", "global", "ecncapability=enabled"],
            )]
        },
    },
    Tweak {
        id: "net.disable_timestamps",
        name: "Disable RFC 1323 TCP timestamps",
        section: Section::Network,
        summary: "Removes the 12-byte timestamp option from TCP headers.",
        rationale: "Twelve bytes off every packet is a rounding error on bandwidth, but the option \
                    also forces additional per-packet processing. Small, free, and safe.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![command(
                "netsh.exe",
                &["int", "tcp", "set", "global", "timestamps=disabled"],
                &["int", "tcp", "set", "global", "timestamps=enabled"],
            )]
        },
    },
    Tweak {
        id: "net.disable_wifi_power_saving",
        name: "Disable Wi-Fi power saving",
        section: Section::Network,
        summary: "Forces the wireless adapter into maximum performance mode on AC power.",
        rationale: "Wireless power saving parks the radio between beacons, adding tens of \
                    milliseconds of jitter. This does not make Wi-Fi good for competitive play — \
                    only an ethernet cable does that — but it removes the worst of it.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: None,
        applies_to: |p| p.network.iter().any(|n| n.is_wifi && n.is_connected),
        build: |_| {
            vec![command(
                "powercfg.exe",
                &[
                    "/setacvalueindex",
                    "SCHEME_CURRENT",
                    "19cbb8fa-5279-450e-9fac-8a3d5fedd0c1",
                    "12bbebe6-58d6-4636-95bb-3217ef867c1a",
                    "0",
                ],
                &[
                    "/setacvalueindex",
                    "SCHEME_CURRENT",
                    "19cbb8fa-5279-450e-9fac-8a3d5fedd0c1",
                    "12bbebe6-58d6-4636-95bb-3217ef867c1a",
                    "3",
                ],
            )]
        },
    },
    Tweak {
        id: "net.disable_network_location_wizard",
        name: "Suppress the network discovery prompt",
        section: Section::Network,
        summary: "Stops the 'do you want your PC to be discoverable' dialog appearing.",
        rationale: "It appears on top of a fullscreen game when a network state changes and \
                    forces a focus loss. Purely about not being interrupted.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        // The commonly cited form of this tweak works by the mere *presence* of
        // the NewNetworkWindowOff key, which Forged cannot revert faithfully:
        // undo removes the value it wrote but leaves the now-empty key, and the
        // dialog stays suppressed. The policy value below carries the same
        // meaning in a value that can actually be restored.
        build: |_| {
            vec![hklm_dword(
                r"SOFTWARE\Policies\Microsoft\Windows\Network Connections",
                "NC_StdDomainUserSetLocation",
                1,
            )]
        },
    },
    Tweak {
        id: "net.qos_reserved_bandwidth",
        name: "QoS reserved bandwidth limit",
        section: Section::Network,
        summary: "Sets NonBestEffortLimit to 0 on the QoS packet scheduler.",
        rationale: "The famous '20% of your bandwidth is reserved by Windows' tweak. It is not \
                    true: the reservation only applies to applications that explicitly request \
                    QoS guarantees, and it is released the moment the link is busy. Applied \
                    because users look for it and it is inert, and reported as doing nothing.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::NoMeasuredBenefit,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![hklm_dword(
                r"SOFTWARE\Policies\Microsoft\Windows\Psched",
                "NonBestEffortLimit",
                0,
            )]
        },
    },
    Tweak {
        id: "net.tcp_port_exhaustion",
        name: "Widen the ephemeral port range",
        section: Section::Network,
        summary: "Raises MaxUserPort and shortens TcpTimedWaitDelay.",
        rationale: "Another perennial recommendation. Port exhaustion is a real problem on servers \
                    holding thousands of concurrent sockets; a game client holds a handful. \
                    Harmless, does nothing for ping, included and labelled as such.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::NoMeasuredBenefit,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            let path = r"SYSTEM\CurrentControlSet\Services\Tcpip\Parameters";
            vec![
                hklm_dword(path, "MaxUserPort", 65534),
                hklm_dword(path, "TcpTimedWaitDelay", 30),
            ]
        },
    },
    Tweak {
        id: "net.disable_teredo",
        name: "Disable Teredo and 6to4 tunnelling",
        section: Section::Network,
        summary: "Turns off legacy IPv6 transition tunnels.",
        rationale: "Teredo wraps IPv6 inside UDP over IPv4 and adds a hop through a relay server, \
                    which can silently route your traffic through another country. Fortnite does \
                    not need it.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::SituationalGain,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                command(
                    "netsh.exe",
                    &["interface", "teredo", "set", "state", "disabled"],
                    &["interface", "teredo", "set", "state", "default"],
                ),
                command(
                    "netsh.exe",
                    &["interface", "6to4", "set", "state", "disabled"],
                    &["interface", "6to4", "set", "state", "default"],
                ),
            ]
        },
    },
    Tweak {
        id: "net.dns_low_latency",
        name: "Use a low-latency DNS resolver",
        section: Section::Network,
        summary: "Points the active adapter at Cloudflare 1.1.1.1 with Google 8.8.8.8 as secondary.",
        rationale: "DNS does not affect in-match ping — the connection is already established. It \
                    affects how quickly matchmaking, the item shop and the launcher respond. ISP \
                    resolvers are frequently the slowest part of that path.",
        risk: Risk::Medium,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: |p| p.active_adapter().is_some(),
        build: |p| {
            let Some(adapter) = p.active_adapter() else {
                return Vec::new();
            };
            let name = adapter.name.clone();
            vec![
                Action::RunCommand {
                    program: "netsh.exe".into(),
                    args: vec![
                        "interface".into(),
                        "ipv4".into(),
                        "set".into(),
                        "dnsservers".into(),
                        format!("name={name}"),
                        "static".into(),
                        "1.1.1.1".into(),
                        "primary".into(),
                    ],
                    // Returning the interface to DHCP-supplied DNS is the correct
                    // inverse, since that is the stock state.
                    revert: RevertCommand {
                        program: "netsh.exe".into(),
                        args: vec![
                            "interface".into(),
                            "ipv4".into(),
                            "set".into(),
                            "dnsservers".into(),
                            format!("name={name}"),
                            "dhcp".into(),
                        ],
                    },
                    tolerate_exit_codes: vec![1],
                },
                Action::RunCommand {
                    program: "netsh.exe".into(),
                    args: vec![
                        "interface".into(),
                        "ipv4".into(),
                        "add".into(),
                        "dnsservers".into(),
                        format!("name={name}"),
                        "8.8.8.8".into(),
                        "index=2".into(),
                    ],
                    revert: RevertCommand {
                        program: "netsh.exe".into(),
                        args: vec![
                            "interface".into(),
                            "ipv4".into(),
                            "set".into(),
                            "dnsservers".into(),
                            format!("name={name}"),
                            "dhcp".into(),
                        ],
                    },
                    tolerate_exit_codes: vec![1],
                },
            ]
        },
    },
    Tweak {
        id: "net.flush_dns_cache",
        name: "Flush the DNS resolver cache",
        section: Section::Network,
        summary: "Clears cached DNS entries so the new resolver takes effect immediately.",
        rationale: "Without this, stale entries from the previous resolver keep being used until \
                    they expire, which makes the DNS change look like it did nothing.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![command("ipconfig.exe", &["/flushdns"], &["/flushdns"])]
        },
    },
];
