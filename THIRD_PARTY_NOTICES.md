# Third-party notices

pi-oven is built on open-source software. Everything shipped in the server binary, the web UI and the desktop/Android shell is under a permissive license (checked from the crates' and packages' own metadata, not by reading every source file). Full texts are in each package's own license file (`~/.cargo/registry`, `web/node_modules`), and a release that redistributes binaries should include them.

## Rust (server and desktop shell)

About 140 crates, almost all `MIT`, `Apache-2.0` or `MIT OR Apache-2.0` (axum, tokio, serde, clap, tracing, rust-embed, tauri and its plugins …). Notes:

- `r-efi`: `MIT OR Apache-2.0 OR LGPL-2.1-or-later` (used under MIT/Apache-2.0).
- `ryu`: `Apache-2.0 OR BSL-1.0` (used under Apache-2.0).
- `unicode-ident`: `(MIT OR Apache-2.0) AND Unicode-3.0`.

## Web UI (what is bundled into `web/dist`)

- Svelte, Vite (build only), marked, CodeMirror 6 and its language packages: MIT.
- highlight.js: BSD-3-Clause.
- DOMPurify: `MPL-2.0 OR Apache-2.0`, used under **Apache-2.0**.
- JetBrains Mono: SIL OFL 1.1 (see `ASSETS_LICENSE.md`).
- Build-time only, not shipped: lightningcss (MPL-2.0), TypeScript, svelte-check.

## Fonts

- Maplestory Light: © 2017 NEXON-Maplestory (designed by FONTRIX Inc.), used under Nexon Korea's free-use terms (full text in `web/src/assets/fonts/MAPLESTORY-LICENSE.txt`). This page's screens use the Maplestory typeface provided by Nexon's Maplestory.
- JetBrains Mono: SIL OFL 1.1 (see `ASSETS_LICENSE.md`).

## Icons

Possibly resembling, in a few simple shapes, the following sets (see `ASSETS_LICENSE.md`):

- Feather: MIT License, Copyright (c) 2013-2023 Cole Bemis.
- Lucide: ISC License, Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as part of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2022.

Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby granted, provided that the above copyright notice and this permission notice appear in all copies.
