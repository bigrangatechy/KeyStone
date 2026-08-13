// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    std::env::set_var("PROTOC", protoc);
    let proto_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../proto");
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&[proto_dir.join("ingest.proto")], &[proto_dir])?;
    println!("cargo:rerun-if-changed=../../proto/ingest.proto");
    Ok(())
}
