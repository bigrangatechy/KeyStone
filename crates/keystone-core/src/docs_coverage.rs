// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::docker::DockerOp;
use crate::metrics::catalog;
use crate::rbac::Permission;
use crate::sys::SysOp;
use crate::widgets::WidgetKind;
use strum::IntoEnumIterator;

const DEV_METRICS: &str = include_str!("../../../docs/dev/src/metrics.md");
const DEV_DOCKER: &str = include_str!("../../../docs/dev/src/docker.md");
const DEV_SYSTEM: &str = include_str!("../../../docs/dev/src/system.md");
const DEV_PERMISSIONS: &str = include_str!("../../../docs/dev/src/permissions.md");
const DEV_WIDGETS: &str = include_str!("../../../docs/dev/src/widgets.md");

#[test]
fn developer_metrics_doc_lists_catalog() {
    for def in catalog() {
        let needle = format!("`{}`", def.name);
        assert!(
            DEV_METRICS.contains(&needle),
            "docs/dev/src/metrics.md missing {needle}"
        );
    }
}

#[test]
fn developer_docker_doc_lists_ops() {
    for op in DockerOp::iter() {
        let needle = format!("`{}`", op.as_str());
        assert!(
            DEV_DOCKER.contains(&needle),
            "docs/dev/src/docker.md missing {needle}"
        );
    }
}

#[test]
fn developer_system_doc_lists_ops() {
    for op in SysOp::iter() {
        let needle = format!("`{}`", op.as_str());
        assert!(
            DEV_SYSTEM.contains(&needle),
            "docs/dev/src/system.md missing {needle}"
        );
    }
}

#[test]
fn developer_permissions_doc_lists_permissions() {
    for p in Permission::iter() {
        let needle = format!("`{}`", p.as_str());
        assert!(
            DEV_PERMISSIONS.contains(&needle),
            "docs/dev/src/permissions.md missing {needle}"
        );
    }
}

#[test]
fn developer_widgets_doc_lists_kinds() {
    for kind in WidgetKind::iter() {
        let needle = format!("`{}`", kind.as_str());
        assert!(
            DEV_WIDGETS.contains(&needle),
            "docs/dev/src/widgets.md missing {needle}"
        );
    }
    for needle in [
        "`density`",
        "`cards`",
        "`accent`",
        "`donut`",
        "`bar`",
        "`line`",
        "`area`",
        "`compact`",
        "normalize",
        "`empty`",
        "`hide`",
    ] {
        assert!(
            DEV_WIDGETS.contains(needle),
            "docs/dev/src/widgets.md missing {needle}"
        );
    }
}

#[test]
fn operator_dashboard_documents_page_and_widget_styles() {
    let dash = include_str!("../../../docs/src/dashboard.md");
    for needle in [
        "density",
        "compact",
        "comfortable",
        "spacious",
        "bordered",
        "flush",
        "raised",
        "accent",
        "donut",
        "horizontal bar",
        "filled area",
        "Hide empty",
        "title",
    ] {
        assert!(
            dash.contains(needle),
            "docs/src/dashboard.md missing {needle}"
        );
    }
}

#[test]
fn operator_audit_page_is_documented() {
    let audit = include_str!("../../../docs/src/audit.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    assert!(
        audit.contains("200"),
        "operator Audit must state the row cap"
    );
    assert!(
        audit.contains("ingest token"),
        "operator Audit must say the ingest token cannot write the log"
    );
    assert!(http.contains("`/audit`"), "HTTP API must list GET /audit");
    assert!(
        http.contains("200"),
        "HTTP API must match the audit row cap"
    );
}

#[test]
fn operator_docs_cover_idle_session() {
    let security = include_str!("../../../docs/src/security.md");
    let using = include_str!("../../../docs/src/using.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    for (name, body) in [
        ("security.md", security),
        ("using.md", using),
        ("troubleshooting.md", trouble),
        ("http-api.md", http),
    ] {
        assert!(
            body.contains("two hours"),
            "{name} must say the UI session idles out after two hours"
        );
    }
    assert!(
        security.contains("Log out") && !security.contains("copied from DevTools"),
        "security.md must not claim last-tab close kills a stolen cookie"
    );
}

#[test]
fn operator_docs_cover_headless_system_manage() {
    let system = include_str!("../../../docs/src/system.md");
    let using = include_str!("../../../docs/src/using.md");
    assert!(
        system.contains("TrueNAS") && system.contains("Proxmox"),
        "System chapter must say NAS/hypervisors are not the manage target"
    );
    assert!(
        system.contains("Observe"),
        "appliance hosts stay on Observe"
    );
    assert!(
        using.contains("stay on Observe"),
        "using.md must point Proxmox/TrueNAS at Observe"
    );
}

#[test]
fn operator_docs_cover_boot_compose_and_updates_list() {
    let install = include_str!("../../../docs/src/install.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    let docker = include_str!("../../../docs/src/docker.md");
    let system = include_str!("../../../docs/src/system.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    assert!(
        install.contains("is-enabled"),
        "install must tell operators to enable units for reboot"
    );
    assert!(
        trouble.contains("Did not start after reboot"),
        "troubleshooting must cover a missing KeyStone after apt/kernel reboot"
    );
    assert!(
        docker.contains("stays on this tab"),
        "operator Docker doc must say Down does not drop the Compose project"
    );
    assert!(
        docker.contains("Stop") && docker.contains("Restart"),
        "operator Docker doc must list Compose stop/restart"
    );
    assert!(
        system.contains("apt list --upgradable"),
        "System tab must list apt list --upgradable, not only apt-get -s upgrade"
    );
    assert!(
        system.contains("Restart=always"),
        "System tab must say units come back after reboot before Apply is safe"
    );
    assert!(
        http.contains("500"),
        "HTTP API must mention the updates list cap"
    );
}

#[test]
fn operator_docs_cover_needrestart_and_reboot() {
    let system = include_str!("../../../docs/src/system.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    let security = include_str!("../../../docs/src/security.md");
    let audit = include_str!("../../../docs/src/audit.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    assert!(
        system.contains("NEEDRESTART_MODE=list"),
        "System chapter must say Apply will not auto-restart docker/ssh"
    );
    assert!(
        system.contains("needrestart -b") && system.contains("systemctl --failed"),
        "System chapter must document leftover services and failed units"
    );
    assert!(
        system.contains("systemctl reboot") && system.contains("Poweroff"),
        "System chapter must document confirmed reboot and that poweroff stays out"
    );
    assert!(
        trouble.contains("NEEDRESTART_MODE=list"),
        "troubleshooting must mention leftover services after Apply"
    );
    assert!(
        security.contains("reboot"),
        "security.md must treat reboot as the same trust class as apt apply"
    );
    assert!(
        audit.contains("confirmed reboot"),
        "Audit must list reboot as a System mutation"
    );
    assert!(
        http.contains("`reboot`"),
        "HTTP API must mention the reboot POST"
    );
    assert!(
        !arch.contains("System reboot/shutdown"),
        "architecture.md must not list reboot as still out of this slice"
    );
    assert!(
        arch.contains("System shutdown from the UI"),
        "shutdown stays out; reboot is in"
    );
}

#[test]
fn operator_docs_cover_journal_ntp_gitlab_age() {
    let system = include_str!("../../../docs/src/system.md");
    let using = include_str!("../../../docs/src/using.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/system.md");
    assert!(
        system.contains("journalctl") && system.contains("timedatectl"),
        "System chapter must document journal follow and NTP"
    );
    assert!(
        system.contains("ssh.service") && system.contains("keystone-agent.service"),
        "System chapter must name the allowlisted units"
    );
    assert!(
        system.contains("unit-name textbox"),
        "System chapter must say journal is not a unit-name textbox"
    );
    assert!(
        system.contains("/var/opt/gitlab/backups"),
        "System chapter must say dump age comes from the Omnibus backups dir"
    );
    assert!(
        using.contains("journals") && using.contains("NTP"),
        "using.md must mention journals and NTP on the System tab"
    );
    assert!(
        trouble.contains("unit-name textbox") && trouble.contains("timedatectl"),
        "troubleshooting must cover unknown journal units and clock sync"
    );
    assert!(
        http.contains("/sys/journal/"),
        "HTTP API must list journal follow routes"
    );
    assert!(
        dev.contains("`journal`"),
        "developer system.md must list the journal op"
    );
}

#[test]
fn operator_docs_cover_autoremove_and_unattended() {
    let system = include_str!("../../../docs/src/system.md");
    let using = include_str!("../../../docs/src/using.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    let audit = include_str!("../../../docs/src/audit.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/system.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    assert!(
        system.contains("autoremove") && system.contains("not `dist-upgrade`"),
        "System chapter must document apt-get autoremove and say it is not dist-upgrade"
    );
    assert!(
        system.contains("unattended-upgrades") && system.contains("20auto-upgrades"),
        "System chapter must document unattended-upgrades observe"
    );
    assert!(
        system.contains("/var/lib/apt/periodic/unattended-upgrades-stamp"),
        "System chapter must name the unattended stamp path"
    );
    assert!(
        system.contains("no config editor"),
        "System chapter must say unattended-upgrades is not an editor"
    );
    assert!(
        using.contains("unattended-upgrades") && using.contains("autoremove"),
        "using.md must mention unattended-upgrades and autoremove on the System tab"
    );
    assert!(
        trouble.contains("20auto-upgrades") && trouble.contains("autoremove"),
        "troubleshooting must cover autoremove and unattended config staying out"
    );
    assert!(
        audit.contains("apt autoremove"),
        "Audit chapter must list autoremove as a mutation"
    );
    assert!(
        !audit.contains("unattended-upgrades config"),
        "observing unattended-upgrades is not an audit mutation"
    );
    assert!(
        http.contains("/sys/autoremove"),
        "HTTP API must list the autoremove follow page"
    );
    assert!(
        dev.contains("`updates_autoremove`") && dev.contains("not `dist-upgrade`"),
        "developer system.md must list updates_autoremove"
    );
    assert!(
        arch.contains("unattended-upgrades config editor")
            && arch.contains("editing `20auto-upgrades` is not"),
        "architecture.md must keep the unattended config editor out"
    );
}

#[test]
fn operator_docs_cover_container_cards_and_system_split() {
    let docker = include_str!("../../../docs/src/docker.md");
    let using = include_str!("../../../docs/src/using.md");
    let system = include_str!("../../../docs/src/system.md");
    let config = include_str!("../../../docs/src/configuration.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/docker.md");
    assert!(
        docker.contains("cards") && docker.contains("`Env`"),
        "operator Docker doc must describe container cards and that Env is not shown"
    );
    assert!(
        using.contains("cards") && using.contains("health vs actions"),
        "using.md must mention container cards and the System split"
    );
    assert!(
        system.contains("Health is on the left") && system.contains("Actions"),
        "System chapter must describe the health vs actions columns"
    );
    assert!(
        system.contains("warning") && config.contains("warning"),
        "Settings must document the System Manage warning"
    );
    assert!(
        http.contains("/api/v1/nodes/{id}/containers/{cid}") && http.contains("Env"),
        "HTTP API must list summarized inspect and say Env is dropped"
    );
    assert!(
        dev.contains("summarized") && dev.contains("Env"),
        "developer docker.md must say inspect is summarized without Env"
    );
}

#[test]
fn operator_docs_cover_ipv4_step_up() {
    let using = include_str!("../../../docs/src/using.md");
    let security = include_str!("../../../docs/src/security.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    let system = include_str!("../../../docs/src/system.md");
    let audit = include_str!("../../../docs/src/audit.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/system.md");
    let docker = include_str!("../../../docs/dev/src/docker.md");
    assert!(
        using.contains("current authenticator code") && using.contains("IPv4"),
        "using.md must say IPv4 asks for a current authenticator code when 2FA is on"
    );
    assert!(
        security.contains("current")
            && security.contains("backup code")
            && security.contains("IPv4"),
        "security.md must say IPv4 needs a current code, not a backup code"
    );
    assert!(
        trouble.contains("IPv4 wants a code") && trouble.contains("not a backup code"),
        "troubleshooting must cover IPv4 step-up"
    );
    assert!(
        system.contains("current") && system.contains("6-digit"),
        "System chapter must say Apply IPv4 asks for a current 6-digit code"
    );
    assert!(
        audit.contains("authenticator") && audit.contains("ok"),
        "Audit must mention refused IPv4 step-up"
    );
    assert!(
        http.contains("`totp`") && http.contains("needs_step_up") && http.contains("net_set"),
        "HTTP API must document the totp form field on net_set"
    );
    assert!(
        dev.contains("needs_step_up()")
            && dev.contains("`net_set`")
            && dev.contains("`vlan_add`")
            && dev.contains("`wifi_join`")
            && dev.contains("`ssh_password`")
            && dev.contains("`unit_restart`")
            && dev.contains("`gitlab_restore`"),
        "developer system.md must say net_set, vlan_add, wifi_join, ssh_password, unit_restart, and gitlab_restore need step-up"
    );
    assert!(
        docker.contains("needs_step_up()") && docker.contains("confirm"),
        "developer docker.md must say no Docker op needs step-up yet"
    );
}

#[test]
fn operator_docs_cover_leftover_unit_restart() {
    let system = include_str!("../../../docs/src/system.md");
    let using = include_str!("../../../docs/src/using.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    let security = include_str!("../../../docs/src/security.md");
    let audit = include_str!("../../../docs/src/audit.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/system.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    assert!(
        system.contains("Restart")
            && system.contains("systemctl restart")
            && system.contains("unit-name textbox"),
        "System chapter must document listed-name restart, not a textbox"
    );
    assert!(
        using.contains("leftover restart") && using.contains("current authenticator code"),
        "using.md must mention leftover restart and step-up"
    );
    assert!(
        trouble.contains("Restart refused") && trouble.contains("live leftover"),
        "troubleshooting must cover a stale leftover restart"
    );
    assert!(
        security.contains("Restart") && security.contains("leftover"),
        "security.md must treat leftover restart like IPv4 for step-up"
    );
    assert!(
        audit.contains("leftover/failed unit restart"),
        "Audit must list unit restart as a mutation"
    );
    assert!(
        http.contains("`unit_restart`") && http.contains("unit"),
        "HTTP API must mention unit_restart"
    );
    assert!(
        dev.contains("`unit_restart`") && dev.contains("live leftover"),
        "developer system.md must say the helper re-checks leftover/failed lists"
    );
    assert!(
        arch.contains("unit restart") && arch.contains("unit-name textbox"),
        "architecture.md must say listed restart is in and a textbox is not"
    );
}

#[test]
fn operator_docs_cover_gitlab_restore() {
    let system = include_str!("../../../docs/src/system.md");
    let using = include_str!("../../../docs/src/using.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    let security = include_str!("../../../docs/src/security.md");
    let audit = include_str!("../../../docs/src/audit.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/system.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    assert!(
        system.contains("gitlab-backup restore")
            && system.contains("not a path textbox")
            && system.contains("replaces GitLab application data"),
        "System chapter must document listed-dump restore, not a path"
    );
    assert!(
        using.contains("GitLab restore") && using.contains("current authenticator code"),
        "using.md must mention GitLab restore and step-up"
    );
    assert!(
        trouble.contains("Restore refused") && trouble.contains("listed dump"),
        "troubleshooting must cover a stale restore pick and missing ticket"
    );
    assert!(
        security.contains("Restore") && security.contains("replaces application data"),
        "security.md must treat GitLab restore as data-destroy step-up"
    );
    assert!(
        audit.contains("GitLab Omnibus restore"),
        "Audit must list restore as a mutation"
    );
    assert!(
        http.contains("`gitlab_restore`") && http.contains("ticket"),
        "HTTP API must mention gitlab_restore and the one-shot ticket"
    );
    assert!(
        dev.contains("`gitlab_restore`") && dev.contains("live backups dir"),
        "developer system.md must say the helper re-checks the backups directory"
    );
    assert!(
        arch.contains("GitLab restore") && !arch.contains("Watchtower, GitLab restore"),
        "architecture.md must say Omnibus restore is in"
    );
}

#[test]
fn operator_docs_cover_ethernet_ipv6() {
    let system = include_str!("../../../docs/src/system.md");
    let using = include_str!("../../../docs/src/using.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    let security = include_str!("../../../docs/src/security.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/system.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    assert!(
        system.contains("IPv6") && system.contains("SLAAC") && system.contains("Ethernet"),
        "System chapter must document Ethernet IPv6"
    );
    assert!(
        using.contains("IPv6") && using.contains("current authenticator code"),
        "using.md must mention IPv6 step-up"
    );
    assert!(
        trouble.contains("Static IPv6") && trouble.contains("zone id"),
        "troubleshooting must cover IPv6 lockout and rejected zone ids"
    );
    assert!(
        security.contains("IPv6") && security.contains("IPv4"),
        "security.md must treat IPv6 like IPv4 for lockout"
    );
    assert!(
        http.contains("`ipv6_method`"),
        "HTTP API must mention ipv6_method"
    );
    assert!(
        dev.contains("IPv6") && dev.contains("netplan apply"),
        "developer system.md must say IPv6 is on net_set and tests must not apply it"
    );
    assert!(
        arch.contains("IPv6") && arch.contains("net_set"),
        "architecture.md must say Ethernet IPv6 is in on net_set"
    );
}

#[test]
fn operator_docs_cover_vlan_add() {
    let system = include_str!("../../../docs/src/system.md");
    let using = include_str!("../../../docs/src/using.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    let security = include_str!("../../../docs/src/security.md");
    let audit = include_str!("../../../docs/src/audit.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/system.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    assert!(
        system.contains("Add VLAN") && system.contains("1–4094") && system.contains("name textbox"),
        "System chapter must document listed-parent VLAN create, not a name textbox"
    );
    assert!(
        using.contains("VLAN") && using.contains("current authenticator code"),
        "using.md must mention VLAN create and step-up"
    );
    assert!(
        trouble.contains("Add VLAN refused") && trouble.contains("live address"),
        "troubleshooting must cover a stale VLAN parent"
    );
    assert!(
        security.contains("Add VLAN") && security.contains("IPv4"),
        "security.md must treat VLAN create like IPv4 for step-up"
    );
    assert!(
        audit.contains("VLAN create"),
        "Audit must list VLAN create as a mutation"
    );
    assert!(
        http.contains("`vlan_add`") && http.contains("`vlan`"),
        "HTTP API must mention vlan_add"
    );
    assert!(
        dev.contains("`vlan_add`") && dev.contains("live address list"),
        "developer system.md must say the helper re-checks the live address list"
    );
    assert!(
        arch.contains("VLAN create") && arch.contains("QinQ"),
        "architecture.md must say VLAN create is in and QinQ stays out"
    );
}

#[test]
fn operator_docs_cover_wifi_join() {
    let system = include_str!("../../../docs/src/system.md");
    let using = include_str!("../../../docs/src/using.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    let security = include_str!("../../../docs/src/security.md");
    let audit = include_str!("../../../docs/src/audit.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/system.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    assert!(
        system.contains("Join Wi-Fi")
            && system.contains("listed SSID")
            && system.contains("SSID textbox"),
        "System chapter must document scan-then-join, not an SSID textbox"
    );
    assert!(
        using.contains("Wi-Fi") && using.contains("current authenticator code"),
        "using.md must mention Wi-Fi join and step-up"
    );
    assert!(
        trouble.contains("Join Wi-Fi refused") && trouble.contains("live scan"),
        "troubleshooting must cover a stale Wi-Fi scan"
    );
    assert!(
        security.contains("Join Wi-Fi") && security.contains("PSK"),
        "security.md must say the Wi-Fi password is not audited"
    );
    assert!(
        audit.contains("Wi-Fi join") && audit.contains("PSK"),
        "Audit must list Wi-Fi join and say the PSK is stripped"
    );
    assert!(
        http.contains("`wifi_join`") && http.contains("`wifi_scan`") && http.contains("`psk`"),
        "HTTP API must mention wifi_scan and wifi_join"
    );
    assert!(
        dev.contains("`wifi_join`") && dev.contains("live scan"),
        "developer system.md must say the helper re-checks the live scan"
    );
    assert!(
        arch.contains("Wi-Fi join") && arch.contains("802.1X"),
        "architecture.md must say Wi-Fi join is in and 802.1X stays out"
    );
}

#[test]
fn operator_docs_cover_ssh_password() {
    let system = include_str!("../../../docs/src/system.md");
    let using = include_str!("../../../docs/src/using.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    let security = include_str!("../../../docs/src/security.md");
    let audit = include_str!("../../../docs/src/audit.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/system.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    assert!(
        system.contains("SSH password")
            && system.contains("keys only")
            && system.contains("user editor"),
        "System chapter must document SSH password as a yes/no toggle, not a user editor"
    );
    assert!(
        using.contains("SSH password") && using.contains("current authenticator code"),
        "using.md must mention SSH password and step-up"
    );
    assert!(
        trouble.contains("SSH password refused") && trouble.contains("sshd"),
        "troubleshooting must cover a refused SSH password change"
    );
    assert!(
        security.contains("SSH password") && security.contains("lock you out"),
        "security.md must say turning SSH passwords off can lock you out"
    );
    assert!(
        audit.contains("SSH password"),
        "Audit must list SSH password as a mutation"
    );
    assert!(
        http.contains("`ssh_password`") && http.contains("`password_auth`"),
        "HTTP API must mention ssh_password and password_auth"
    );
    assert!(
        dev.contains("`ssh_password`") && dev.contains("sshd -T"),
        "developer system.md must say observe is sshd -T"
    );
    assert!(
        arch.contains("SSH password") && arch.contains("firewall"),
        "architecture.md must say SSH password is in and firewall stays out"
    );
}

#[test]
fn operator_docs_cover_image_login() {
    let docker = include_str!("../../../docs/src/docker.md");
    let using = include_str!("../../../docs/src/using.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    let security = include_str!("../../../docs/src/security.md");
    let audit = include_str!("../../../docs/src/audit.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/docker.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    assert!(
        docker.contains("Log in")
            && docker.contains("not kept in KeyStone")
            && docker.contains("GHCR"),
        "Docker chapter must document Hub/GHCR login on the node, not a server store"
    );
    assert!(
        using.contains("GHCR") && using.contains("Hub"),
        "using.md must mention Hub/GHCR login on Images"
    );
    assert!(
        trouble.contains("Login refused") && trouble.contains("Harbor"),
        "troubleshooting must cover a refused registry login"
    );
    assert!(
        security.contains("GHCR") && security.contains("database"),
        "security.md must say Hub/GHCR passwords are not in the server database"
    );
    assert!(
        audit.contains("Hub/GHCR login") && audit.contains("password omitted"),
        "Audit must list registry login and say the password is omitted"
    );
    assert!(
        http.contains("`image_login`") && http.contains("`password`"),
        "HTTP API must mention image_login"
    );
    assert!(
        dev.contains("`image_login`") && dev.contains("docker login"),
        "developer docker.md must say the agent runs docker login"
    );
    assert!(
        arch.contains("image_login") && arch.contains("browse"),
        "architecture.md must say login is in and GHCR browse stays out"
    );
}
