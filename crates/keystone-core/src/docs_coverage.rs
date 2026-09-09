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
fn developer_ingest_doc_covers_stdin_stream() {
    let ingest = include_str!("../../../docs/dev/src/ingest.md");
    let docker = include_str!("../../../docs/dev/src/docker.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    let feat = include_str!("../../../docs/src/features.md");
    assert!(
        ingest.contains("chunk")
            && ingest.contains("stdin")
            && ingest.contains("cols")
            && ingest.contains("not `cancel`"),
        "ingest.md must document server→agent StreamChunk as stdin, not cancel"
    );
    assert!(
        docker.contains("stdin") && docker.contains("Interactive exec is not in"),
        "developer docker.md must say logs ignore stdin and exec stays out of the UI"
    );
    assert!(
        arch.contains("server → agent as stdin") && arch.contains("Interactive exec is not in"),
        "architecture.md must say stdin is on ingest and exec is not in the UI"
    );
    assert!(
        feat.contains("Bidirectional `StreamChunk`") && feat.contains("exec checkbox"),
        "features.md Later must say the protocol is in and the UI is not"
    );
}

#[test]
fn developer_agent_api_plan_is_documented() {
    let plan = include_str!("../../../docs/dev/src/agent-api.md");
    let feat = include_str!("../../../docs/src/features.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    let summary = include_str!("../../../docs/dev/src/SUMMARY.md");
    assert!(
        summary.contains("agent-api.md") && plan.contains("# LLM Agent API"),
        "developer SUMMARY must list the Agent API plan"
    );
    assert!(
        plan.contains("never exposed to the internet")
            && plan.contains("keystone-sys.socket")
            && plan.contains("capability flags")
            && plan.contains("ingest token"),
        "Agent API plan must keep the internet, sys-socket, capability, and token rules"
    );
    assert!(
        plan.contains("needs_step_up") && plan.contains("denied"),
        "Agent API plan must deny UI step-up ops (no authenticator on this path)"
    );
    assert!(
        plan.contains("Conversational") && plan.contains("cookie session"),
        "UI chat must stay on the existing session, not this socket"
    );
    assert!(
        feat.contains("LLM Agent API") && feat.contains("never on the internet"),
        "features.md Later must mention the LAN-only Agent API"
    );
    assert!(
        feat.contains("Exposing the LLM Agent API to the internet"),
        "features.md stay-out must forbid internet exposure"
    );
    assert!(
        arch.contains("LLM Agent API") && arch.contains("agent-api.md"),
        "architecture.md must point at the Agent API plan"
    );
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
        system.contains("unattended-upgrades.service"),
        "System chapter must list unattended-upgrades.service on the journal allowlist"
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
        system.contains("timedatectl set-timezone") && system.contains("timezone textbox"),
        "System chapter must document timezone as a dropdown, not a textbox"
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
fn operator_docs_cover_timezone_set() {
    let system = include_str!("../../../docs/src/system.md");
    let using = include_str!("../../../docs/src/using.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    let audit = include_str!("../../../docs/src/audit.md");
    let feat = include_str!("../../../docs/src/features.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/system.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    assert!(
        system.contains("timedatectl set-timezone") && system.contains("timezone textbox"),
        "System chapter must document timezone as a dropdown, not a textbox"
    );
    assert!(
        using.contains("timezone") && using.contains("dropdown"),
        "using.md must mention timezone from a dropdown"
    );
    assert!(
        trouble.contains("Timezone refused") && trouble.contains("live list"),
        "troubleshooting must cover a stale timezone dropdown"
    );
    assert!(
        audit.contains("timezone"),
        "Audit must list timezone as a System mutation"
    );
    assert!(
        http.contains("`timezone_set`") && http.contains("`timezone`"),
        "HTTP API must mention timezone_set and the timezone form field"
    );
    assert!(
        dev.contains("`timezone_set`") && dev.contains("timedatectl set-timezone"),
        "developer system.md must list timezone_set and say tests must not invoke it"
    );
    assert!(
        arch.contains("timezone_set") && arch.contains("timezone textbox"),
        "architecture.md must say timezone dropdown is in and a textbox stays out"
    );
    assert!(
        feat.contains("timezone dropdown") && !feat.contains("Timezone from a dropdown"),
        "features.md Current must list timezone; Later must drop the timezone slice"
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
        system.contains("99-keystone-unattended") && system.contains("enable/disable"),
        "System chapter must document the unattended-upgrades toggle drop-in"
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
    assert!(
        http.contains("`unattended_set`") && dev.contains("`unattended_set`"),
        "HTTP API and developer system.md must list unattended_set"
    );
    assert!(
        arch.contains("unattended_set") && using.contains("unattended-upgrades enable"),
        "architecture and using.md must mention the unattended-upgrades toggle"
    );
    assert!(
        audit.contains("unattended-upgrades enable"),
        "Audit must list unattended-upgrades enable/disable as a mutation"
    );
    assert!(
        trouble.contains("Unattended refused"),
        "troubleshooting must cover a refused unattended-upgrades toggle"
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
            && dev.contains("`unit_enable`")
            && dev.contains("`gitlab_restore`"),
        "developer system.md must say net_set, vlan_add, wifi_join, ssh_password, unit_restart, unit_enable, and gitlab_restore need step-up"
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
        !security.contains("There is no Hub login"),
        "security.md must not deny Hub login after image_login shipped"
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

#[test]
fn operator_docs_cover_compose_cards() {
    let docker = include_str!("../../../docs/src/docker.md");
    let using = include_str!("../../../docs/src/using.md");
    let dev = include_str!("../../../docs/dev/src/docker.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    assert!(
        docker.contains("one card per project")
            && docker.contains("running/exited")
            && docker.contains("service table")
            && docker.contains("CasaOS-style app shop"),
        "operator Docker doc must describe Compose glance cards, not a shop"
    );
    assert!(
        using.contains("Compose are cards") && using.contains("Volumes and Networks are cards"),
        "using.md must mention Compose cards and Volumes/Networks as cards"
    );
    assert!(
        dev.contains("glance cards") && dev.contains("No extra inspect"),
        "developer docker.md must say Compose cards reuse compose_ps"
    );
    assert!(
        arch.contains("compose_ps") && arch.contains("service table") && arch.contains("app shop"),
        "architecture.md must say Compose cards are not an app shop"
    );
}

#[test]
fn operator_docs_cover_image_inspect() {
    let docker = include_str!("../../../docs/src/docker.md");
    let using = include_str!("../../../docs/src/using.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/docker.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    assert!(
        docker.contains("one card per local image")
            && docker.contains("entrypoint")
            && docker.contains("`Env`"),
        "operator Docker doc must describe image cards and that Env is not shown"
    );
    assert!(
        using.contains("inspect drops") && using.contains("`Env`"),
        "using.md must mention image inspect without Env"
    );
    assert!(
        http.contains("/api/v1/nodes/{id}/images/{iid}") && http.contains("image_inspect"),
        "HTTP API must list summarized image inspect"
    );
    assert!(
        dev.contains("`image_inspect`") && dev.contains("drops `Env`"),
        "developer docker.md must say image inspect is summarized without Env"
    );
    assert!(
        arch.contains("image_inspect") && arch.contains("Env"),
        "architecture.md must say image inspect drops Env"
    );
}

#[test]
fn operator_docs_cover_volume_network_inspect() {
    let docker = include_str!("../../../docs/src/docker.md");
    let using = include_str!("../../../docs/src/using.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/docker.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    assert!(
        docker.contains("one card per volume")
            && docker.contains("one card per network")
            && docker.contains("Labels"),
        "operator Docker doc must describe volume/network cards and that Labels are not shown"
    );
    assert!(
        using.contains("Volumes and Networks are cards") && using.contains("Labels"),
        "using.md must mention volume/network cards without Labels"
    );
    assert!(
        http.contains("/api/v1/nodes/{id}/volumes/{vname}")
            && http.contains("volume_inspect")
            && http.contains("/api/v1/nodes/{id}/networks/{nid}")
            && http.contains("network_inspect"),
        "HTTP API must list summarized volume and network inspect"
    );
    assert!(
        dev.contains("`volume_inspect`")
            && dev.contains("`network_inspect`")
            && dev.contains("drops Labels"),
        "developer docker.md must say volume/network inspect is summarized without Labels"
    );
    assert!(
        arch.contains("volume_inspect")
            && arch.contains("network_inspect")
            && arch.contains("Labels"),
        "architecture.md must say volume/network inspect drops Labels"
    );
}

#[test]
fn operator_docs_cover_system_df() {
    let docker = include_str!("../../../docs/src/docker.md");
    let using = include_str!("../../../docs/src/using.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/docker.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    let audit = include_str!("../../../docs/src/audit.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    assert!(
        docker.contains("Engine disk use") && docker.contains("prune build cache"),
        "operator Docker doc must describe df glance and build-cache prune"
    );
    assert!(
        using.contains("Engine disk use") && using.contains("build cache"),
        "using.md must mention Engine disk use and build cache"
    );
    assert!(
        http.contains("/api/v1/nodes/{id}/system-df") && http.contains("system_df"),
        "HTTP API must list summarized system df"
    );
    assert!(
        dev.contains("`system_df`") && dev.contains("`build_cache_prune`"),
        "developer docker.md must list system_df and build_cache_prune"
    );
    assert!(
        arch.contains("system_df") && arch.contains("build_cache_prune"),
        "architecture.md must mention system_df and build_cache_prune"
    );
    assert!(
        audit.contains("build cache") || audit.contains("build-cache"),
        "audit.md must mention build-cache prune"
    );
    assert!(
        trouble.contains("Engine disk use") && trouble.contains("build cache"),
        "troubleshooting must point disk-full at Images df and prune"
    );
}

/// Walkthrough needles for every SysOp. A new variant fails to compile here
/// until `docs/src/using.md` (Help → User guide) mentions it.
fn user_guide_sysop_needle(op: SysOp) -> &'static str {
    match op {
        SysOp::Status => "health vs actions",
        SysOp::UpdatesList | SysOp::UpdatesApply => "apt",
        SysOp::UpdatesAutoremove => "autoremove",
        SysOp::NetSet => "IPv4",
        SysOp::VlanAdd => "VLAN",
        SysOp::WifiScan | SysOp::WifiJoin => "Wi-Fi",
        SysOp::SshPassword => "SSH password",
        SysOp::GitlabBackup => "GitLab backup",
        SysOp::GitlabRestore => "GitLab restore",
        SysOp::Reboot => "reboot",
        SysOp::Journal => "journals",
        SysOp::UnitRestart => "leftover restart",
        SysOp::UnitEnable => "Start KeyStone on boot",
        SysOp::TimezoneSet => "timezone",
        SysOp::UnattendedSet => "unattended-upgrades",
    }
}

#[test]
fn operator_docs_cover_changelog_and_features() {
    let log = include_str!("../../../docs/src/changelog.md");
    let feat = include_str!("../../../docs/src/features.md");
    let using = include_str!("../../../docs/src/using.md");
    let intro = include_str!("../../../docs/src/introduction.md");
    assert!(log.contains("# Changelog") && log.contains("AEST"));
    let mut headings = 0usize;
    for line in log.lines() {
        let Some(rest) = line.strip_prefix("## ") else {
            continue;
        };
        headings += 1;
        let parts: Vec<&str> = rest.split_whitespace().collect();
        assert_eq!(
            parts.len(),
            3,
            "changelog heading must be HH:MM:SS DD/MM/YYYY AEST, got {rest}"
        );
        assert_eq!(parts[0].matches(':').count(), 2, "{rest}");
        assert_eq!(parts[1].matches('/').count(), 2, "{rest}");
        assert_eq!(parts[2], "AEST", "{rest}");
    }
    assert!(
        headings >= 50,
        "changelog must keep dated history, got {headings}"
    );
    assert!(
        feat.contains("# Features")
            && feat.contains("## Current")
            && feat.contains("## Later")
            && feat.contains("## Not this product")
            && feat.contains("keystone-sys")
            && feat.contains("docker.sock"),
        "features.md must split current, later, and stay-out"
    );
    assert!(
        using.contains("Features") && using.contains("Changelog"),
        "using.md must point at Features and Changelog"
    );
    assert!(
        intro.contains("HH:MM:SS DD/MM/YYYY AEST"),
        "introduction must say how changelog times are written"
    );
}

#[test]
fn operator_user_guide_covers_every_sysop() {
    let using = include_str!("../../../docs/src/using.md");
    assert!(
        using.contains("# User guide"),
        "using.md is the Help walkthrough"
    );
    for op in SysOp::iter() {
        let needle = user_guide_sysop_needle(op);
        assert!(
            using.contains(needle),
            "docs/src/using.md must mention {needle:?} for SysOp {op:?}"
        );
    }
}

#[test]
fn operator_docs_cover_keystone_boot_enable() {
    let system = include_str!("../../../docs/src/system.md");
    let using = include_str!("../../../docs/src/using.md");
    let trouble = include_str!("../../../docs/src/troubleshooting.md");
    let security = include_str!("../../../docs/src/security.md");
    let audit = include_str!("../../../docs/src/audit.md");
    let http = include_str!("../../../docs/dev/src/http-api.md");
    let dev = include_str!("../../../docs/dev/src/system.md");
    let arch = include_str!("../../../docs/dev/src/architecture.md");
    let packaging = include_str!("../../../docs/dev/src/packaging.md");
    assert!(
        system.contains("Start KeyStone on boot")
            && system.contains("systemctl is-enabled")
            && system.contains("without `--now`"),
        "System chapter must document the boot checkbox as enable, not --now"
    );
    assert!(
        using.contains("Start KeyStone on boot") && using.contains("current authenticator code"),
        "using.md must mention Start KeyStone on boot and step-up"
    );
    assert!(
        trouble.contains("Start KeyStone on boot") && trouble.contains("without `--now`"),
        "troubleshooting must cover the Settings boot checkbox"
    );
    assert!(
        security.contains("Start KeyStone on boot") && security.contains("reboot"),
        "security.md must treat boot enable like leftover restart for step-up"
    );
    assert!(
        audit.contains("Start KeyStone on boot"),
        "Audit must list boot enable as a mutation"
    );
    assert!(
        http.contains("`unit_enable`") && http.contains("`enabled`"),
        "HTTP API must mention unit_enable and enabled"
    );
    assert!(
        dev.contains("`unit_enable`") && dev.contains("`--now`"),
        "developer system.md must say unit_enable is not --now"
    );
    assert!(
        arch.contains("unit_enable") && arch.contains("`--now`"),
        "architecture.md must say boot enable is in and --now stays out of that op"
    );
    assert!(
        packaging.contains("start = false") && packaging.contains("without `--now`"),
        "packaging.md must say upgrades do not start the daemon and the UI checkbox is not --now"
    );
}
