// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

fn main() {
    println!("cargo:rerun-if-changed=../keystone-agent/Cargo.toml");
    println!("cargo:rerun-if-changed=../keystone-server/Cargo.toml");
    let agent =
        std::fs::read_to_string("../keystone-agent/Cargo.toml").expect("keystone-agent Cargo.toml");
    let server = std::fs::read_to_string("../keystone-server/Cargo.toml")
        .expect("keystone-server Cargo.toml");
    println!(
        "cargo:rustc-env=KEYSTONE_AGENT_DEB_REVISION={}",
        deb_revision(&agent)
    );
    println!(
        "cargo:rustc-env=KEYSTONE_SERVER_DEB_REVISION={}",
        deb_revision(&server)
    );
}

fn deb_revision(toml: &str) -> String {
    let mut in_deb = false;
    for line in toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_deb = t == "[package.metadata.deb]";
            continue;
        }
        if in_deb && t.starts_with("revision") {
            if let Some(v) = t.split('"').nth(1) {
                return v.to_string();
            }
        }
    }
    panic!("missing [package.metadata.deb] revision");
}
