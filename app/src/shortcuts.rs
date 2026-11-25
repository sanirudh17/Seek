pub struct Shortcut {
    pub group: &'static str,
    pub keys: &'static str,
    pub action: &'static str,
}
pub const ALL: &[Shortcut] = &[
    Shortcut {
        group: "Browse",
        keys: "Ctrl+L",
        action: "Search or go to folder",
    },
    Shortcut {
        group: "Browse",
        keys: "Enter",
        action: "Open",
    },
    Shortcut {
        group: "Browse",
        keys: "Ctrl+Enter",
        action: "Open folder in new tab",
    },
    Shortcut {
        group: "Browse",
        keys: "Ctrl+T / Ctrl+W",
        action: "New tab / close tab",
    },
    Shortcut {
        group: "Browse",
        keys: "Ctrl+Tab / Ctrl+Shift+Tab",
        action: "Next / previous tab",
    },
    Shortcut {
        group: "Browse",
        keys: "Alt+0 / Alt+1–9",
        action: "All locations / sidebar items",
    },
    Shortcut {
        group: "Browse",
        keys: "Ctrl+1 / Ctrl+2 / Ctrl+3 / Ctrl+4",
        action: "List / icons / gallery / columns",
    },
    Shortcut {
        group: "Browse",
        keys: "Ctrl+J",
        action: "Icon view options",
    },
    Shortcut {
        group: "Browse",
        keys: "Ctrl+B",
        action: "Toggle sidebar",
    },
    Shortcut {
        group: "Browse",
        keys: "Ctrl+I",
        action: "Toggle info panel",
    },
    Shortcut {
        group: "Browse",
        keys: "Ctrl+,",
        action: "Settings",
    },
    Shortcut {
        group: "Panes",
        keys: "Ctrl+\\ / Ctrl+Shift+\\",
        action: "Split right / down",
    },
    Shortcut {
        group: "Panes",
        keys: "Ctrl+Shift+W",
        action: "Close pane",
    },
    Shortcut {
        group: "Panes",
        keys: "F6 / Shift+F6",
        action: "Next / previous pane",
    },
    Shortcut {
        group: "Panes",
        keys: "Ctrl+Shift+Enter",
        action: "Zoom pane",
    },
    Shortcut {
        group: "Panes",
        keys: "Alt+F5 / Alt+F6",
        action: "Copy / move to other pane",
    },
    Shortcut {
        group: "Files",
        keys: "Arrow keys",
        action: "Move; collapse / expand folders",
    },
    Shortcut {
        group: "Files",
        keys: "Right / Left (Columns)",
        action: "Into folder / up (columns)",
    },
    Shortcut {
        group: "Files",
        keys: "Shift+Arrow / Shift+click",
        action: "Extend selection",
    },
    Shortcut {
        group: "Files",
        keys: "Ctrl+Arrow / Ctrl+click",
        action: "Move without selecting / toggle item",
    },
    Shortcut {
        group: "Files",
        keys: "Ctrl+A",
        action: "Select all",
    },
    Shortcut {
        group: "Files",
        keys: "Ctrl+C / Ctrl+X / Ctrl+V",
        action: "Copy / cut / paste",
    },
    Shortcut {
        group: "Files",
        keys: "Ctrl+Shift+C",
        action: "Copy path",
    },
    Shortcut {
        group: "Files",
        keys: "Ctrl+Shift+N",
        action: "New folder",
    },
    Shortcut {
        group: "Files",
        keys: "F2",
        action: "Rename",
    },
    Shortcut {
        group: "Files",
        keys: "Delete / Shift+Delete",
        action: "Recycle / delete permanently",
    },
    Shortcut {
        group: "Files",
        keys: "Ctrl+Z / Ctrl+Shift+Z",
        action: "Undo / redo",
    },
    Shortcut {
        group: "Quick Look",
        keys: "Space",
        action: "Quick Look",
    },
    Shortcut {
        group: "Quick Look",
        keys: "Escape",
        action: "Close",
    },
    Shortcut {
        group: "Quick Look",
        keys: "Up / Down",
        action: "Previous / next file",
    },
    Shortcut {
        group: "Quick Look",
        keys: "Left / Right",
        action: "Previous / next page",
    },
    Shortcut {
        group: "Quick Look",
        keys: "Ctrl++ / Ctrl+-",
        action: "Zoom in / out",
    },
    Shortcut {
        group: "Quick Look",
        keys: "Ctrl+0 / Ctrl+1",
        action: "Fit / actual size",
    },
    Shortcut {
        group: "Quick Look",
        keys: "Shift+Arrow / drag image",
        action: "Pan",
    },
    Shortcut {
        group: "Text and Markdown",
        keys: "Ctrl+F",
        action: "Find",
    },
    Shortcut {
        group: "Text and Markdown",
        keys: "Ctrl+S",
        action: "Save",
    },
    Shortcut {
        group: "Text and Markdown",
        keys: "Ctrl+Z",
        action: "Undo typing",
    },
    Shortcut {
        group: "Commands and sheets",
        keys: "Ctrl+K",
        action: "Jump",
    },
    Shortcut {
        group: "Commands and sheets",
        keys: "F1",
        action: "Keyboard shortcuts",
    },
    Shortcut {
        group: "Commands and sheets",
        keys: "Tab / Shift+Tab",
        action: "Next / previous field",
    },
    Shortcut {
        group: "Commands and sheets",
        keys: "Escape",
        action: "Cancel or close",
    },
];
