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
            && dev.contains("`unit_restart`")
            && dev.contains("`gitlab_restore`"),
        "developer system.md must say net_set, unit_restart, and gitlab_restore need step-up"
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
