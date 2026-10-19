# Seek

<!-- impeccable:product-schema 1 -->

## Platform

Native Windows desktop, Rust with GPUI; no browser engine.

## Users and purpose

Find files immediately and correctly, especially recent downloads. File correctness takes priority over speed, memory and visual polish in that order.

## Stack

MSVC Rust workspace. Vendor the MIT fsearch-crossplatform portable index and matcher. GPUI pinned to Zed, gpui-component controls. Optional elevated helper belongs to Stage 2.

## Capabilities and constraints

Finished v1 is the baseline. Seek v2 Stage A is closed and Stage B is authorized: a bounded native drag-out experiment and scoped content search. Stage C and packaging remain outside scope. No tray, global hotkeys or app autostart. Closing the window exits. Only the opt-in elevated helper can remain as a service. Filename search plus on-demand content search within configured local folders (Downloads and Documents by default), with cancellation, visible limits, protected-file exclusions and no persistent content index. Do not silently omit hidden, system or noisy folders. Report denied locations. Worker-thread search and event-driven watchers.

## Brand commitments

Finder-style compact file list, sidebar and toolbar, adapted to native Windows caption controls. Segoe UI Variable and readable glass. The supplied Finder, Windows, Zeron and user feedback captures govern the Stage 3 redesign. Neutral obsidian/white themes, real whole-window glass, restrained native Windows icons, compact type and one selected-item preview are required.

## Assumptions

The explicit request to work autonomously replaces design interviews. Build native UI directly from the supplied specification; do not generate a speculative visual mockup.
