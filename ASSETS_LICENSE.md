# Assets and third-party material

## Brand and drawings (made for this project)

These are covered by the project's Apache-2.0 license (see `LICENSE`), like the code. The fonts and the software below keep their own terms.

| File | Notes |
|---|---|
| `web/public/logo.svg` (copied to `desktop/ui/logo.svg`, and the icons in `desktop/icons/` made from it) | Original drawing for pi-oven (a flame). Colours are this app's own theme tokens. It is **not** the pi project's logo; the pi logo is not used anywhere in this repository. |
| `docs/architecture.svg` | Diagram for the README, drawn for this project in the app's colours. The pi logo in it is the one published at https://pi.dev/logo-auto.svg; it belongs to its owners and is used only to say "this box is pi". It is not pi-oven's logo, and the Apache-2.0 grant above does not cover it. |
| `web/src/lib/icons.ts`, `web/src/components/FileIcon.svelte`, `web/src/lib/filetypes.ts` | Line icons (24×24 grid, 1.8px stroke) and the file-type drawings, drawn for this project. They follow the common conventions of open line-icon sets, and a few simple shapes (menu, eye, copy, trash, bell, external link) look like their counterparts in [Feather](https://github.com/feathericons/feather) (MIT, © Cole Bemis) and [Lucide](https://github.com/lucide-icons/lucide) (ISC, © Lucide Contributors). As a precaution their notices are kept in `THIRD_PARTY_NOTICES.md`. |

## Fonts

| Font | License | Notes |
|---|---|---|
| JetBrains Mono (npm `@fontsource-variable/jetbrains-mono`) | SIL Open Font License 1.1 | Bundled into the built web UI. |
| Maplestory Light (`web/src/assets/fonts/Maplestory-Light.woff2`) | Nexon Korea's terms for the Maplestory typeface: free for everyone (personal and company), may be used and distributed freely, may be bundled with other software or embedded as a font, must not be sold as a font, must not be modified or edited ("배포되는 형태 그대로"); the notice goes with it and a credit is recommended. Full text: `web/src/assets/fonts/MAPLESTORY-LICENSE.txt`. | Included in the repository. The file is Nexon's Maplestory OTF Light in a WOFF2 container (the glyph coverage was compared with the TTF: 12,255 vs 12,256 characters, so it is not a subset); no glyph was changed. If you want to be strict about "as distributed", replace it with the file Nexon provides. The credit is in the README. |

## Software

See `THIRD_PARTY_NOTICES.md`.
