# Seek

<!-- impeccable:product-schema 1 -->

## Platform

Native Windows desktop, Rust with GPUI; no browser engine.

## Users and purpose

Find files immediately and correctly, especially recent downloads. File correctness takes priority over speed, memory and visual polish in that order.

## Stack

MSVC Rust workspace. Vendor the MIT fsearch-crossplatform portable index and matcher. GPUI pinned to Zed, gpui-component controls. Optional elevated helper belongs to Stage 2.

## Capabilities and constraints

The November 4 revamp is authorized on an isolated worktree; published 1.0.2 remains the baseline. Implement the numbered reliability, native Quick Look, inspector, text/code previews, operations and panes/workspaces brief in order. Keep Windows-only GPUI pinning and the fsearch engine. No tray, global hotkey, app autostart, remote/cloud locations, AI rename, Office automation/converter, audio/video playback or MCP server. Only the explicitly installed helper may remain as a service. Tests use owned profiles. No version bump, push, tag or release until requested. See docs/REVAMP_PLAN.md for implementation and verification state.

## Brand commitments

Finder-style compact file list, sidebar and toolbar, adapted to native Windows caption controls. Bundled Figtree and Hugeicons accompany native Windows file artwork and readable glass. The supplied Monodrive, Monocode, Zeron, Tilde and user feedback captures govern the revamp. Neutral obsidian/white themes, real whole-window glass, compact type and one selected-item preview are required.

## Assumptions

The explicit request to work autonomously replaces design interviews. Build native UI directly from the supplied specification; do not generate a speculative visual mockup.


Revamp settings direction: preserve all existing options in a compact native popup with separate search/navigation, grouped labelled rows and live dark-glass preview. Opacity is adjustable without repeatedly recreating the native material. Light mode and Windows accessibility policy retain opaque surfaces. Additional Previews, Saved commands and Keyboard shortcuts pages follow their dependent features; do not expose nonfunctional placeholder controls.


Preview correctness: presentations are details-only, including slide shows/templates and related formats. Do not offer saved-slide or extracted-text substitutes for complete graphical slides. Open delegates to the Windows association. PDF rendering remains intact; Word retains text fallback.

Phase-two continuation: the isolated branch implements durable operation evidence, guarded copy/move/rename and recycle inverses, batch rename, explicit interrupted review, New Folder with Selection, mode-aware saved/recent queries, saved programs with direct argv and eleven functional settings pages. There is no automatic recovery replay or command execution. Grouping undo returns selected items and retains the created folder. Original image pixels are decoded within explicit bounds; preview-only fallback is labelled. Native acceptance and host coverage remain separate from implementation status in the ledger. Phase three is not implied complete by these changes.
