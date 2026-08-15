// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Operator documentation compiled into this binary (`/help`, `keystone docs`).
//! Sources are `docs/src/*.md`. Developer docs in `docs/dev/` are not embedded.

use pulldown_cmark::{html, Options, Parser};

pub struct HelpSection {
    pub slug: String,
    pub title: String,
    pub markdown: String,
}

macro_rules! operator_md {
    ($file:expr) => {
        include_str!(concat!("../../../docs/src/", $file))
    };
}

fn strip_spdx(md: &str) -> &str {
    let md = md.trim_start();
    if let Some(rest) = md.strip_prefix("<!--") {
        if let Some(end) = rest.find("-->") {
            return rest[end + 3..].trim_start();
        }
    }
    md
}

fn section(slug: &str, title: &str, raw: &str) -> HelpSection {
    HelpSection {
        slug: slug.into(),
        title: title.into(),
        markdown: strip_spdx(raw).to_string(),
    }
}

pub fn sections() -> Vec<HelpSection> {
    vec![
        section(
            "introduction",
            "Introduction",
            operator_md!("introduction.md"),
        ),
        section("install", "Install", operator_md!("install.md")),
        section("using", "Using the UI", operator_md!("using.md")),
        section("dashboard", "Dashboards", operator_md!("dashboard.md")),
        section("alerts", "Alerts", operator_md!("alerts.md")),
        section("docker", "Docker", operator_md!("docker.md")),
        section(
            "configuration",
            "Configuration",
            operator_md!("configuration.md"),
        ),
        section("metrics", "Metrics", operator_md!("metrics.md")),
        section("security", "Security", operator_md!("security.md")),
        section(
            "troubleshooting",
            "Troubleshooting",
            operator_md!("troubleshooting.md"),
        ),
    ]
}

pub fn section_by_slug(slug: &str) -> Option<HelpSection> {
    sections().into_iter().find(|s| s.slug == slug)
}

pub fn markdown_to_html(md: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(md, options);
    let mut html_out = String::new();
    html::push_html(&mut html_out, parser);
    html_out
}

pub fn all_markdown() -> String {
    let mut out = String::from("# KeyStone\n\nOperator documentation for this version.\n\n");
    for s in sections() {
        out.push_str(&s.markdown);
        out.push_str("\n\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_help_sections_are_nonempty() {
        let secs = sections();
        assert!(secs.len() >= 8);
        for s in &secs {
            assert!(!s.slug.is_empty(), "empty slug");
            assert!(!s.title.is_empty(), "{}", s.slug);
            assert!(
                s.markdown.contains("# "),
                "{} should start with a heading",
                s.slug
            );
            assert!(
                !s.markdown.contains("cargo xtask"),
                "{} still mentions generated docs",
                s.slug
            );
        }
    }
}
