<!--
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
-->

# Contributing to KeyStone

KeyStone is licensed under [GPL-2.0-or-later](COPYING). By contributing, you
agree that your contribution is licensed under the same terms.

## Developer Certificate of Origin

Every commit must be signed off (`git commit -s`) to certify the
[Developer Certificate of Origin](https://developercertificate.org/) (DCO)
reproduced below. There is no CLA.

```
Developer Certificate of Origin
Version 1.1

Copyright (C) 2004, 2006 The Linux Foundation and its contributors.

Everyone is permitted to copy and distribute verbatim copies of this
license document, but changing it is not allowed.


Developer's Certificate of Origin 1.1

By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I
    have the right to submit it under the open source license
    indicated in the file; or

(b) The contribution is based upon previous work that, to the best
    of my knowledge, is covered under an appropriate open source
    license and I have the right under that license to submit that
    work with modifications, whether created in whole or in part
    by me, under the same open source license (unless I am
    permitted to submit under a different license), as indicated
    in the file; or

(c) The contribution was provided directly to me by some other
    person who certified (a), (b) or (c) and I have not modified
    it.

(d) I understand and agree that this project and the contribution
    are public and that a record of the contribution (including all
    personal information I submit with it, including my sign-off) is
    maintained indefinitely and may be redistributed consistent with
    this project or the open source license(s) involved.
```

## Living documentation

Reference material is generated from Rust types. Do not edit files under
`docs/src/generated/` by hand. After changing metrics, config, Docker ops,
RBAC, CLI, or HTTP APIs, run:

```
cargo xtask docs
```

CI fails if that output is dirty. Conceptual pages in `docs/src/` (install,
threat model) are hand-written.

## Checks

```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
cargo xtask docs
```

Use SPDX headers on new source files:

```
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
```
