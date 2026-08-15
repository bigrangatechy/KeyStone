// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Maintainer scripts and units must not be able to wipe Docker or the host
//! on install/upgrade/remove. Keep `docs/dev/src/packaging.md` in sync.

const AGENT_POSTINST: &str = include_str!("../../../packaging/deb/agent/postinst");
const AGENT_PRERM: &str = include_str!("../../../packaging/deb/agent/prerm");
const AGENT_POSTRM: &str = include_str!("../../../packaging/deb/agent/postrm");
const AGENT_UNIT: &str = include_str!("../../../packaging/deb/agent/keystone-agent.service");
const SERVER_POSTINST: &str = include_str!("../../../packaging/deb/server/postinst");
const SERVER_PRERM: &str = include_str!("../../../packaging/deb/server/prerm");
const SERVER_POSTRM: &str = include_str!("../../../packaging/deb/server/postrm");
const SERVER_UNIT: &str = include_str!("../../../packaging/deb/server/keystone-server.service");
const AGENT_CARGO: &str = include_str!("../../keystone-agent/Cargo.toml");
const SERVER_CARGO: &str = include_str!("../../keystone-server/Cargo.toml");

fn scripts() -> [&'static str; 6] {
    [
        AGENT_POSTINST,
        AGENT_PRERM,
        AGENT_POSTRM,
        SERVER_POSTINST,
        SERVER_PRERM,
        SERVER_POSTRM,
    ]
}

fn active_shell(s: &str) -> String {
    s.lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#')
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn maintainer_scripts_never_recurse_chown() {
    for s in scripts() {
        let active = active_shell(s);
        assert!(
            !active.contains("chown -R"),
            "recursive chown can follow a symlink into Docker or /"
        );
        assert!(
            !active.contains("chmod -R"),
            "recursive chmod is not needed"
        );
    }
}

#[test]
fn maintainer_scripts_never_touch_docker_or_os_roots() {
    let needles = [
        "docker prune",
        "docker rm",
        "docker stop",
        "compose down",
        "systemctl stop docker",
        "systemctl restart docker",
        "systemctl stop containerd",
        "rm -rf /var/lib/docker",
        "rm -rf /usr",
        "rm -rf /etc",
        "rm -rf /home",
        "rm -rf /boot",
        "rm -rf /var/lib/keystone\n",
        "rm -rf /var/lib/keystone ",
    ];
    for s in scripts() {
        let active = active_shell(s);
        for n in needles {
            assert!(
                !active.contains(n),
                "packaging script must not contain {n:?}"
            );
        }
    }
}

#[test]
fn purge_is_scoped_to_keystone_state() {
    assert!(active_shell(AGENT_POSTRM).contains("/var/lib/keystone/agent-buffer"));
    assert!(active_shell(SERVER_POSTRM).contains("keystone.sqlite"));
    assert!(active_shell(SERVER_POSTRM).contains("series.redb"));
    assert!(
        !active_shell(SERVER_POSTRM).contains("agent-buffer"),
        "server purge must not delete the agent buffer"
    );
}

#[test]
fn systemd_units_must_not_bind_docker() {
    for unit in [AGENT_UNIT, SERVER_UNIT] {
        for line in unit.lines() {
            let t = line.trim();
            if t.starts_with('#') {
                continue;
            }
            assert!(
                !t.contains("Requires=docker"),
                "Requires=docker would fail metric-only nodes and couple upgrades"
            );
            assert!(
                !t.contains("BindsTo=docker"),
                "BindsTo=docker stops the agent when Engine stops; do not stop Engine from KeyStone"
            );
            assert!(
                !t.contains("PartOf=docker"),
                "PartOf=docker ties KeyStone restart to Engine restart"
            );
        }
    }
    assert!(
        AGENT_UNIT.contains("After=network-online.target docker.socket"),
        "agent may wait for the socket but must not require Engine"
    );
}

#[test]
fn debs_must_not_depend_on_engine() {
    for cargo in [AGENT_CARGO, SERVER_CARGO] {
        let deb = cargo
            .split("[package.metadata.deb]")
            .nth(1)
            .expect("deb metadata");
        let depends = deb
            .lines()
            .find(|l| l.trim_start().starts_with("depends"))
            .expect("depends");
        for pkg in ["docker.io", "docker-ce", "containerd", "podman"] {
            assert!(
                !depends.contains(pkg),
                "deb depends must not pull {pkg} (Engine upgrades bounce containers)"
            );
        }
    }
}

#[test]
fn etc_keystone_is_readable_by_service_user() {
    for (name, s) in [("agent", AGENT_POSTINST), ("server", SERVER_POSTINST)] {
        let active = active_shell(s);
        assert!(
            active.contains("chown root:keystone /etc/keystone"),
            "{name} postinst must make /etc/keystone traversable by group keystone"
        );
        assert!(
            active.contains("chmod 0750 /etc/keystone"),
            "{name} postinst must set /etc/keystone to 0750"
        );
        assert!(
            !active.contains("chown keystone:keystone /etc/keystone"),
            "{name} must not give the service user write on /etc/keystone"
        );
    }
}
