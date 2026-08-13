// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

pub mod keystone {
    pub mod v1 {
        tonic::include_proto!("keystone.v1");
    }
}

pub use keystone::v1::*;
