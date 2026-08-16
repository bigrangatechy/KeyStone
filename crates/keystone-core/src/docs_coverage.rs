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
