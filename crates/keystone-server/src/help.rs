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

/// Header **Help** and `GET /help` land here (the operator walkthrough).
pub const INDEX_SLUG: &str = "using";

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
        section("features", "Features", operator_md!("features.md")),
        section("install", "Install", operator_md!("install.md")),
        section("using", "User guide", operator_md!("using.md")),
        section("dashboard", "Dashboards", operator_md!("dashboard.md")),
        section("alerts", "Alerts", operator_md!("alerts.md")),
        section("docker", "Docker", operator_md!("docker.md")),
        section("system", "System", operator_md!("system.md")),
        section("audit", "Audit", operator_md!("audit.md")),
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
        section("changelog", "Changelog", operator_md!("changelog.md")),
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
    rewrite_help_chapter_hrefs(&html_out)
}

/// mdBook keeps `docker.md` in the source. `/help` is not a directory of
/// markdown files, so rewrite those hrefs to `/help/docker`.
/// Match `href="slug.md` (no closing quote) so `security.md#tls` becomes
/// `/help/security#tls`.
fn rewrite_help_chapter_hrefs(html: &str) -> String {
    let mut out = html.to_string();
    for s in sections() {
        let from = format!("href=\"{}.md", s.slug);
        let to = format!("href=\"/help/{}", s.slug);
        out = out.replace(&from, &to);
    }
    out
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
        assert!(
            secs.iter()
                .any(|s| s.slug == "using" && s.title == "User guide"),
            "operator help must include the User guide"
        );
        assert!(
            secs.iter().any(|s| s.slug == "system"),
            "operator help must include System"
        );
        assert!(
            secs.iter().any(|s| s.slug == "audit"),
            "operator help must include Audit"
        );
        let audit = secs.iter().find(|s| s.slug == "audit").unwrap();
        assert!(
            audit.markdown.contains("ingest token"),
            "Audit help must say the ingest token cannot write the log"
        );
        assert!(
            audit.markdown.contains("retention"),
            "Audit help must say Settings retention does not prune the table"
        );
        assert!(
            audit.markdown.contains("200"),
            "Audit help must match the page row cap"
        );
    }

    #[test]
    fn operator_help_matches_summary() {
        let summary = include_str!("../../../docs/src/SUMMARY.md");
        let entries: Vec<(&str, &str)> = summary
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                let start = l.find('[')?;
                let mid = l.find("](")?;
                let title = &l[start + 1..mid];
                let rest = &l[mid + 2..];
                let end = rest.find(')')?;
                let slug = rest[..end].strip_suffix(".md")?;
                Some((title, slug))
            })
            .collect();
        let help: Vec<(String, String)> =
            sections().into_iter().map(|s| (s.title, s.slug)).collect();
        assert_eq!(
            entries.iter().map(|(t, s)| (*t, *s)).collect::<Vec<_>>(),
            help.iter()
                .map(|(t, s)| (t.as_str(), s.as_str()))
                .collect::<Vec<_>>(),
            "docs/src/SUMMARY.md titles and order must match help.rs sections()"
        );
        assert_eq!(INDEX_SLUG, "using");
        assert!(
            entries.iter().any(|(_, slug)| *slug == INDEX_SLUG),
            "GET /help must land on a chapter that exists"
        );
    }

    #[test]
    fn help_html_rewrites_chapter_links() {
        let html = markdown_to_html("See [Docker](docker.md) and [TLS](security.md#tls).");
        assert!(
            html.contains("href=\"/help/docker\""),
            "chapter links must stay inside /help: {html}"
        );
        assert!(
            html.contains("href=\"/help/security#tls\""),
            "anchors must survive the rewrite: {html}"
        );
        assert!(
            !html.contains(".md"),
            "Help HTML must not leave .md hrefs: {html}"
        );
        for s in sections() {
            let rendered = markdown_to_html(&s.markdown);
            assert!(
                !rendered.contains(".md\"") && !rendered.contains(".md#"),
                "{} Help HTML still has a .md chapter href",
                s.slug
            );
        }
    }
}
