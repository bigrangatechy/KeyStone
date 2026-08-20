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
