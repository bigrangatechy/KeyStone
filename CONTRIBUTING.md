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

## Documentation

Operator docs (`docs/src/`) are hand-written and compiled into the server
(`/help`, `keystone docs`). Developer docs (`docs/dev/src/`) are a separate
book: catalog, widgets, ingest, stores, HTTP API. Do not put crate-level
how-tos in the operator book.

After adding a catalog metric, `DockerOp`, or `Permission`, mention it in
the matching developer chapter (backticks around the name). `cargo test`
fails if those pages miss a variant. Update an operator chapter when an
admin would notice the change.

```
mdbook build docs
mdbook build docs/dev
```

## Checks

```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
mdbook build docs
mdbook build docs/dev
cargo deb -p keystone-agent   # optional; CI also builds arm64
cargo deb -p keystone-server
```

Use SPDX headers on new source files:

```
SPDX-FileCopyrightText: 2026 The KeyStone Authors
SPDX-License-Identifier: GPL-2.0-or-later
```
