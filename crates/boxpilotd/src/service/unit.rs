//! Pure renderer for `boxpilot-sing-box.service` (spec §7.1). No I/O.
//! The test below is the source of truth for the unit content; if the
//! string changes you must update both this file and the reference
//! template in `packaging/linux/systemd/boxpilot-sing-box.service.in`.

use std::path::Path;

pub fn render(core_path: &Path) -> String {
    format!(
        "[Unit]\n\
Description=BoxPilot managed sing-box service\n\
Documentation=https://sing-box.sagernet.org/\n\
After=network-online.target nss-lookup.target\n\
Wants=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
User=root\n\
UMask=0077\n\
WorkingDirectory=/etc/boxpilot/active\n\
ExecStartPre={core} check -c config.json\n\
ExecStart={core} run -c config.json\n\
Restart=on-failure\n\
RestartSec=5s\n\
StartLimitIntervalSec=300\n\
StartLimitBurst=5\n\
LimitNOFILE=1048576\n\
\n\
# Sandboxing — keep what TUN / auto_redirect need, drop everything else (spec \u{a7}7.1)\n\
NoNewPrivileges=true\n\
CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW\n\
AmbientCapabilities=CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW\n\
ProtectSystem=strict\n\
ProtectHome=true\n\
PrivateTmp=true\n\
ProtectControlGroups=true\n\
RestrictNamespaces=true\n\
RestrictRealtime=true\n\
LockPersonality=true\n\
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK AF_PACKET\n\
ReadWritePaths=/etc/boxpilot/active\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n",
        core = core_path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn render_substitutes_core_path_in_exec_start() {
        let s = render(&PathBuf::from("/var/lib/boxpilot/cores/current/sing-box"));
        assert!(s.contains("ExecStart=/var/lib/boxpilot/cores/current/sing-box run -c config.json"));
        assert!(s.contains(
            "ExecStartPre=/var/lib/boxpilot/cores/current/sing-box check -c config.json"
        ));
    }

    #[test]
    fn render_includes_required_sandbox_directives() {
        let s = render(&PathBuf::from("/usr/bin/sing-box"));
        for must_have in [
            "NoNewPrivileges=true",
            "CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW",
            "AmbientCapabilities=CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW",
            "ProtectSystem=strict",
            "ProtectHome=true",
            "PrivateTmp=true",
            "ProtectControlGroups=true",
            "RestrictNamespaces=true",
            "RestrictRealtime=true",
            "LockPersonality=true",
            "RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK AF_PACKET",
            "ReadWritePaths=/etc/boxpilot/active",
            "WorkingDirectory=/etc/boxpilot/active",
        ] {
            assert!(s.contains(must_have), "missing: {must_have}\n----\n{s}");
        }
    }

    /// Spec §7.1 explicitly does NOT set ProtectKernelTunables because
    /// auto_redirect writes to /proc/sys/net/* sysctls. Catch a future
    /// PR that "tightens" sandboxing and silently breaks auto_redirect.
    #[test]
    fn render_does_not_set_protect_kernel_tunables() {
        let s = render(&PathBuf::from("/x"));
        assert!(
            !s.contains("ProtectKernelTunables"),
            "auto_redirect needs sysctl writes — see spec \u{a7}7.1"
        );
    }

    #[test]
    fn render_install_section_targets_multi_user() {
        let s = render(&PathBuf::from("/x"));
        assert!(s.contains("[Install]\nWantedBy=multi-user.target"));
    }
}
