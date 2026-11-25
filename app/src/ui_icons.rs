//! Needed Hugeicons assets adapted from the read-only Monodrive/Monocode references.
pub fn load(path: &str) -> Option<&'static [u8]> {
    match path {
        // Agent marks from Glint (lobe-icons, MIT).
        "agents/antigravity.svg" => Some(include_bytes!("../assets/agents/antigravity.svg")),
        "agents/claude-code.svg" => Some(include_bytes!("../assets/agents/claude-code.svg")),
        "agents/claude-desktop.svg" => Some(include_bytes!("../assets/agents/claude-desktop.svg")),
        "agents/codex.svg" => Some(include_bytes!("../assets/agents/codex.svg")),
        "agents/copilot.svg" => Some(include_bytes!("../assets/agents/copilot.svg")),
        "agents/cursor.svg" => Some(include_bytes!("../assets/agents/cursor.svg")),
        "agents/grok.svg" => Some(include_bytes!("../assets/agents/grok.svg")),
        "agents/oh-my-pi.svg" => Some(include_bytes!("../assets/agents/oh-my-pi.svg")),
        "agents/opencode.svg" => Some(include_bytes!("../assets/agents/opencode.svg")),
        "agents/pi.svg" => Some(include_bytes!("../assets/agents/pi.svg")),
        "icons/terminal.svg" => Some(include_bytes!("../assets/saved-commands.svg")),
        "icons/arrow-down-up.svg" => Some(include_bytes!("../assets/hugeicons/sort-arrows.svg")),
        "icons/check.svg" => Some(include_bytes!("../assets/hugeicons/check-hi.svg")),
        "icons/chevron-down.svg" => Some(include_bytes!("../assets/hugeicons/chevron-down-hi.svg")),
        "icons/chevron-left.svg" => Some(include_bytes!("../assets/hugeicons/chevron-left.svg")),
        "icons/chevron-right.svg" => Some(include_bytes!("../assets/hugeicons/chevron-right.svg")),
        "icons/chevron-up.svg" => Some(include_bytes!("../assets/hugeicons/chevron-up.svg")),
        "icons/chevrons-up-down.svg" => {
            Some(include_bytes!("../assets/hugeicons/fold-vertical.svg"))
        }
        "icons/clock.svg" => Some(include_bytes!("../assets/hugeicons/clock.svg")),
        "icons/close.svg" => Some(include_bytes!("../assets/hugeicons/cancel-01.svg")),
        "icons/copy.svg" => Some(include_bytes!("../assets/hugeicons/copy.svg")),
        "icons/download.svg" => Some(include_bytes!("../assets/hugeicons/download.svg")),
        "icons/cloud-download.svg" => {
            Some(include_bytes!("../assets/hugeicons/cloud-download.svg"))
        }
        "icons/ellipsis.svg" => Some(include_bytes!("../assets/hugeicons/more-horizontal.svg")),
        "icons/external-link.svg" => Some(include_bytes!("../assets/hugeicons/external-link.svg")),
        "seek/pie-chart.svg" => Some(include_bytes!("../assets/hugeicons/pie-chart.svg")),
        "seek/git-branch.svg" => Some(include_bytes!("../assets/hugeicons/git-branch.svg")),
        "seek/source-code.svg" => Some(include_bytes!("../assets/hugeicons/source-code.svg")),
        "icons/eye.svg" => Some(include_bytes!("../assets/hugeicons/eye.svg")),
        "icons/eye-off.svg" => Some(include_bytes!("../assets/hugeicons/eye-off.svg")),
        "icons/file-text.svg" => Some(include_bytes!("../assets/hugeicons/file-script.svg")),
        "icons/film.svg" => Some(include_bytes!("../assets/hugeicons/play.svg")),
        "icons/folder.svg" => Some(include_bytes!("../assets/hugeicons/folder-01.svg")),
        "icons/folder-open.svg" => Some(include_bytes!("../assets/hugeicons/folder-open.svg")),
        "icons/gallery-horizontal.svg" => {
            Some(include_bytes!("../assets/hugeicons/gallery-view.svg"))
        }
        "icons/hard-drive.svg" => Some(include_bytes!("../assets/hugeicons/hard-drive.svg")),
        "icons/image.svg" => Some(include_bytes!("../assets/hugeicons/image-add.svg")),
        "icons/info.svg" => Some(include_bytes!("../assets/hugeicons/information.svg")),
        "icons/help.svg" => Some(include_bytes!("../assets/hugeicons/help-circle.svg")),
        "icons/keyboard.svg" => Some(include_bytes!("../assets/hugeicons/keyboard.svg")),
        "icons/columns-3.svg" => Some(include_bytes!("../assets/hugeicons/layout-3-column.svg")),
        "icons/maximize-2.svg" => Some(include_bytes!("../assets/hugeicons/pane-zoom-in.svg")),
        "icons/minimize-2.svg" => Some(include_bytes!("../assets/hugeicons/pane-zoom-out.svg")),
        "icons/layout-grid.svg" => Some(include_bytes!("../assets/hugeicons/grid-view.svg")),
        "icons/list.svg" => Some(include_bytes!("../assets/hugeicons/list-view.svg")),
        "icons/list-bullet.svg" => Some(include_bytes!("../assets/hugeicons/list-bullet.svg")),
        "icons/monitor.svg" => Some(include_bytes!("../assets/hugeicons/app-window.svg")),
        "icons/moon.svg" => Some(include_bytes!("../assets/hugeicons/sliders.svg")),
        "icons/music.svg" => Some(include_bytes!("../assets/hugeicons/music.svg")),
        "icons/panel-right.svg" => Some(include_bytes!("../assets/hugeicons/sidebar-right.svg")),
        "icons/panel-left.svg" => Some(include_bytes!("../assets/hugeicons/sidebar-left.svg")),
        "icons/pencil.svg" => Some(include_bytes!("../assets/hugeicons/pencil.svg")),
        "icons/pipette.svg" => Some(include_bytes!("../assets/hugeicons/pipette.svg")),
        "icons/plus.svg" => Some(include_bytes!("../assets/hugeicons/add-01.svg")),
        "icons/refresh-cw.svg" => Some(include_bytes!("../assets/hugeicons/refresh-cw.svg")),
        "icons/search.svg" => Some(include_bytes!("../assets/hugeicons/search.svg")),
        "icons/settings.svg" => Some(include_bytes!("../assets/hugeicons/settings.svg")),
        "icons/share.svg" => Some(include_bytes!("../assets/hugeicons/share.svg")),
        "icons/sun.svg" => Some(include_bytes!("../assets/hugeicons/palette.svg")),
        "icons/trash.svg" => Some(include_bytes!("../assets/hugeicons/trash.svg")),
        "icons/window-close.svg" => Some(include_bytes!("../assets/hugeicons/cancel-01.svg")),
        "icons/window-maximize.svg" => {
            Some(include_bytes!("../assets/hugeicons/window-maximize.svg"))
        }
        "icons/window-minimize.svg" => Some(include_bytes!("../assets/hugeicons/minus-sign.svg")),
        "icons/window-restore.svg" => {
            Some(include_bytes!("../assets/hugeicons/window-restore.svg"))
        }
        "icons/x.svg" => Some(include_bytes!("../assets/hugeicons/cancel-01.svg")),
        "seek/hidden-files.svg" => Some(include_bytes!("../assets/hugeicons/eye-off.svg")),
        "icons/sort-ascending.svg" => Some(include_bytes!("../assets/hugeicons/mono-arrow-up.svg")),
        "icons/sort-descending.svg" => Some(include_bytes!("../assets/hugeicons/arrow-down.svg")),
        _ => None,
    }
}
