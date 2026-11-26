use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
    thread,
};

pub static FILE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum View {
    #[default]
    List,
    Icons,
    Gallery,
    Columns,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Search {
    pub query: String,
    pub scope: Option<PathBuf>,
    #[serde(default)]
    pub content: bool,
}
pub fn retain_search(rows: &mut Vec<Search>, search: Search, limit: usize) {
    if search.query.trim().is_empty() || search.query.len() > 4096 {
        return;
    }
    rows.retain(|row| row != &search);
    rows.insert(0, search);
    rows.truncate(limit);
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Appearance,
    pub glass: bool,
    pub glass_opacity: u8,
    pub shell_hue: u16,
    pub shell_saturation: u8,
    pub shell_lightness: u8,
    pub sidebar_opacity: u8,
    pub main_pane_glass: bool,
    pub background_image: Option<PathBuf>,
    /// Pre-effect setting; `glass_version` 2 moved it into `background_effect`.
    pub background_blur: bool,
    /// Opacity behind files, 0-30.
    pub background_opacity: u8,
    /// Opacity in empty folders, 0-80.
    pub background_empty_opacity: u8,
    pub background_effect: crate::background::Effect,
    pub background_scope: crate::background::Scope,
    /// Fine noise over the glass.
    pub grain: bool,
    pub blur: Blur,
    /// A named palette (appearance::PALETTES): tint, text and accent. The
    /// accent comes only from the palette; there is no separate override.
    pub palette: String,
    /// 0 for settings written before the hue-aware glass (missing field).
    #[serde(default)]
    pub glass_version: u8,
    pub show_hidden: bool,
    pub default_view: View,
    pub indexed_locations: Vec<PathBuf>,
    pub exclusions: Vec<PathBuf>,
    pub content_locations: Vec<PathBuf>,
    pub remember_document_text: bool,
    pub keep_text_backups: bool,
    pub auto_save_text: bool,
    /// Reopen the panes, tabs and folders that were open when Seek closed.
    pub restore_layout: bool,
    pub automatic_previews: bool,
    pub image_thumbnails: bool,
    pub saved_searches: Vec<Search>,
    pub recent_searches: Vec<Search>,
    pub saved_commands: Vec<crate::saved_commands::SavedCommand>,
    pub info_panel_width: f32,
    pub check_updates_on_startup: bool,
    /// Gallery shows the selected item large above the strip. Clicking the
    /// selected item again, or the preview's hide button, turns it off.
    pub gallery_preview: bool,
    /// Favorites order and user groups in the sidebar.
    pub sidebar: crate::sidebar_groups::Layout,
    /// Sidebar Locations in the user's order; others follow.
    pub location_order: Vec<PathBuf>,
    /// Locations hidden from the sidebar (still indexed and searchable).
    pub hidden_locations: Vec<PathBuf>,
    /// How strongly the palette colours the window, 0-100.
    pub palette_intensity: u8,
    /// How long a drag rests on a folder or tab before it opens (ms).
    pub drag_hover_ms: u16,
    /// Saved advanced-rename presets.
    pub rename_presets: Vec<crate::rename_steps::Preset>,
    /// Icons view options per pane id (Ctrl+J).
    pub icon_views: std::collections::BTreeMap<u64, crate::icon_view::IconView>,
    /// Repositories added by hand to the Worktrees page (common Git folders).
    pub worktree_repos: Vec<PathBuf>,
    /// Let agents change files (Settings > Agents); off by default.
    pub agents_can_change: bool,
    /// The only folders agents may change.
    pub agent_folders: Vec<PathBuf>,
    /// Offer agents every tool, not only the core set.
    pub agent_tools_all: bool,
    /// Hide secret values in `.env` previews (11.3); on by default.
    pub hide_env_secrets: bool,
    /// Let coding agents read unmasked `.env` values through MCP (11.3); off by default.
    pub agent_read_secrets: bool,
    /// Line-wrap preference persisted per preview surface (11.5).
    pub wrap_info_panel: bool,
    pub wrap_quick_look: bool,
    pub wrap_document_pane: bool,
    /// Show brief confirmation toasts for quiet actions (11.6); on by default.
    pub show_confirmations: bool,
    /// Sidebar Favorites and Locations independent fold state (11.8).
    pub favorites_collapsed: bool,
    pub locations_collapsed: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Appearance::System,
            glass: true,
            // The Frost preset on the Graphite palette: a cool, very dark
            // tint, not black, so the desktop shows through as glass.
            glass_opacity: 62,
            // Graphite's surface colour (#121418).
            shell_hue: 220,
            shell_saturation: 14,
            shell_lightness: 8,
            sidebar_opacity: 40,
            main_pane_glass: true,
            background_image: None,
            background_blur: true,
            background_opacity: 10,
            background_empty_opacity: 35,
            background_effect: crate::background::Effect::Haze,
            background_scope: crate::background::Scope::All,
            grain: false,
            blur: Blur::Strong,
            palette: "graphite".into(),
            glass_version: GLASS_VERSION,
            show_hidden: false,
            default_view: View::List,
            indexed_locations: vec![],
            exclusions: vec![],
            content_locations: crate::content::default_locations(),
            remember_document_text: true,
            keep_text_backups: false,
            auto_save_text: false,
            restore_layout: true,
            automatic_previews: true,
            image_thumbnails: true,
            saved_searches: vec![],
            recent_searches: vec![],
            saved_commands: vec![],
            info_panel_width: 280.,
            check_updates_on_startup: true,
            gallery_preview: true,
            sidebar: Default::default(),
            icon_views: Default::default(),
            rename_presets: vec![],
            drag_hover_ms: 650,
            palette_intensity: 55,
            location_order: vec![],
            hidden_locations: vec![],
            worktree_repos: vec![],
            agents_can_change: false,
            agent_folders: vec![],
            agent_tools_all: false,
            hide_env_secrets: true,
            agent_read_secrets: false,
            wrap_info_panel: true,
            wrap_quick_look: true,
            wrap_document_pane: true,
            show_confirmations: true,
            favorites_collapsed: false,
            locations_collapsed: false,
        }
    }
}
/// The glass settings' format; see `read`.
pub const GLASS_VERSION: u8 = 4;

/// How the backdrop behind dark glass is made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Blur {
    /// Acrylic: whatever is behind the window, blurred.
    #[default]
    Strong,
    /// Mica: the desktop wallpaper only, softly blurred.
    Soft,
}

pub fn read(data: &Path) -> Settings {
    let mut settings: Settings = std::fs::read(data.join("settings.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    settings.indexed_locations.truncate(64);
    settings.exclusions.truncate(256);
    settings.shell_hue = settings.shell_hue.min(360);
    settings.shell_saturation = settings.shell_saturation.min(100);
    settings.shell_lightness = settings.shell_lightness.min(30);
    settings.sidebar_opacity = settings.sidebar_opacity.min(100);
    settings.glass_opacity = settings.glass_opacity.min(100);
    if settings.glass_version == 2 {
        // Version 2 turned grain on by default; the glass is clear again.
        settings.grain = false;
    }
    if settings.glass_version < GLASS_VERSION {
        // Pure black glass (the old default) becomes the Frost tint; any
        // tint someone chose is kept.
        if settings.glass_version < 2
            && settings.shell_saturation == 0
            && settings.shell_lightness == 0
        {
            let fresh = Settings::default();
            settings.shell_hue = fresh.shell_hue;
            settings.shell_saturation = fresh.shell_saturation;
            settings.shell_lightness = fresh.shell_lightness;
            settings.glass_opacity = settings.glass_opacity.min(fresh.glass_opacity);
            settings.sidebar_opacity = settings.sidebar_opacity.max(fresh.sidebar_opacity);
        }
        if settings.glass_version < 2 && !settings.background_blur {
            settings.background_effect = crate::background::Effect::None;
        }
        if settings.glass_version < 4 && settings.shell_saturation <= 10 {
            // Version 4: the tint takes the palette's colour, not only its
            // hue. Only the old presets' near-grey tints move; a tint someone
            // chose with the sliders stays.
            let id = settings.palette.clone();
            crate::appearance::apply_palette(&mut settings, &id);
        }
        settings.glass_version = GLASS_VERSION;
    }
    settings.rename_presets.truncate(32);
    settings.drag_hover_ms = settings.drag_hover_ms.clamp(200, 2000);
    settings.palette_intensity = settings.palette_intensity.min(100);
    settings.location_order.truncate(64);
    settings.hidden_locations.truncate(64);
    settings.worktree_repos.truncate(64);
    settings.agent_folders.truncate(64);
    while settings.icon_views.len() > crate::icon_view::REMEMBERED {
        settings.icon_views.pop_first();
    }
    for options in settings.icon_views.values_mut() {
        *options = options.bounded();
    }
    settings.background_opacity = settings.background_opacity.min(30);
    settings.background_empty_opacity = settings.background_empty_opacity.min(80);
    settings.content_locations.truncate(64);
    settings.saved_commands.truncate(32);
    settings.sidebar.sanitize();
    for (rows, limit) in [
        (&mut settings.saved_searches, 32),
        (&mut settings.recent_searches, 16),
    ] {
        rows.retain(|row| !row.query.trim().is_empty() && row.query.len() <= 4096);
        let mut seen = vec![];
        rows.retain(|row| {
            if seen.contains(row) {
                false
            } else {
                seen.push(row.clone());
                true
            }
        });
        rows.truncate(limit);
    }
    settings.info_panel_width = if settings.info_panel_width.is_finite() {
        settings.info_panel_width.clamp(240., 720.)
    } else {
        280.
    };
    settings
}
pub fn write(data: &Path, settings: &Settings) -> std::io::Result<()> {
    let _guard = FILE_LOCK.lock().unwrap();
    write_locked(data, settings)
}
fn write_locked(data: &Path, settings: &Settings) -> std::io::Result<()> {
    let mut value = std::fs::read(data.join("settings.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    for (key, item) in serde_json::to_value(settings)?.as_object().unwrap() {
        value[key] = item.clone();
    }
    atomic_json(data, "settings.json", &value)
}
pub fn atomic_json(data: &Path, name: &str, value: &serde_json::Value) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    std::fs::create_dir_all(data)?;
    let path = data.join(name);
    let temp = data.join(format!("{name}.tmp"));
    std::fs::write(&temp, serde_json::to_vec_pretty(value)?)?;
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    if unsafe {
        MoveFileExW(
            wide(&temp).as_ptr(),
            wide(&path).as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
#[derive(Clone)]
pub struct Store {
    pub directory: std::sync::Arc<PathBuf>,
    pub value: Arc<Mutex<Settings>>,
    tx: std::sync::mpsc::SyncSender<()>,
}
impl Store {
    pub fn new(data: PathBuf, settings: Settings) -> Self {
        let directory = Arc::new(data.clone());
        let value = Arc::new(Mutex::new(settings));
        let worker = value.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        thread::Builder::new()
            .name("seek-settings".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    // Sliders preview in memory immediately. Coalesce a burst
                    // before touching disk; no timer runs while the app is idle.
                    while rx
                        .recv_timeout(std::time::Duration::from_millis(50))
                        .is_ok()
                    {}
                    let _guard = FILE_LOCK.lock().unwrap();
                    let snapshot = worker.lock().unwrap().clone();
                    if let Err(e) = write_with_retry(&data, &snapshot) {
                        eprintln!("Cannot save settings: {e}");
                    }
                }
            })
            .expect("settings writer");
        Self {
            value,
            tx,
            directory,
        }
    }
    pub fn save(&self, settings: &Settings) {
        *self.value.lock().unwrap() = settings.clone();
        let _ = self.tx.try_send(());
    }
}
fn write_with_retry(data: &Path, settings: &Settings) -> std::io::Result<()> {
    // Short-lived readers and antivirus handles can outlast the first 120 ms,
    // especially while startup work is contending for CPU. This worker never
    // blocks the UI; keep retries bounded to 560 ms of total backoff.
    for attempt in 0..8 {
        match write_locked(data, settings) {
            Ok(()) => return Ok(()),
            Err(error) if attempt < 7 && matches!(error.raw_os_error(), Some(5 | 32 | 33)) => {
                std::thread::sleep(std::time::Duration::from_millis(20 * (attempt + 1)));
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_history_preserves_scope_mode_and_bounds_duplicates() {
        let temp = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        let search = Search {
            query: "quoted \"café\" ext:md".into(),
            scope: Some(PathBuf::from(r"C:\Owned")),
            content: true,
        };
        retain_search(&mut settings.saved_searches, search.clone(), 32);
        retain_search(&mut settings.saved_searches, search.clone(), 32);
        assert_eq!(settings.saved_searches.len(), 1);
        for index in 0..20 {
            retain_search(
                &mut settings.recent_searches,
                Search {
                    query: format!("query {index}"),
                    scope: None,
                    content: false,
                },
                16,
            );
        }
        write(temp.path(), &settings).unwrap();
        let restored = read(temp.path());
        assert_eq!(restored.saved_searches, vec![search]);
        assert_eq!(restored.recent_searches.len(), 16);
        assert_eq!(restored.recent_searches[0].query, "query 19");
        let too_long = Search {
            query: "x".repeat(4097),
            scope: None,
            content: false,
        };
        retain_search(&mut settings.saved_searches, too_long.clone(), 32);
        assert_eq!(settings.saved_searches.len(), 1);
        assert!(settings.wrap_info_panel);
        assert!(settings.wrap_quick_look);
        assert!(settings.wrap_document_pane);
        settings.wrap_info_panel = false;
        settings.wrap_document_pane = false;
        write(temp.path(), &settings).unwrap();
        let restored = read(temp.path());
        assert!(!restored.wrap_info_panel);
        assert!(restored.wrap_quick_look);
        assert!(!restored.wrap_document_pane);
        settings
            .saved_searches
            .extend([too_long, settings.saved_searches[0].clone()]);
        write(temp.path(), &settings).unwrap();
        assert_eq!(read(temp.path()).saved_searches.len(), 1);
    }
    #[test]
    fn defaults_and_legacy_geometry_survive_a_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("settings.json"),
            r##"{"window":{"bounds":[0,0,1100,700]},"future":42}"##,
        )
        .unwrap();
        let mut settings = read(temp.path());
        assert_eq!(settings.theme, Appearance::System);
        assert!(settings.glass);
        assert!(settings.check_updates_on_startup);
        assert_eq!(settings.info_panel_width, 280.);
        settings.info_panel_width = 480.;
        settings.theme = Appearance::Dark;
        settings.check_updates_on_startup = false;
        assert!(!settings.auto_save_text);
        assert!(!settings.keep_text_backups);
        assert!(settings.automatic_previews && settings.image_thumbnails);
        settings.automatic_previews = false;
        settings.image_thumbnails = false;
        settings.auto_save_text = true;
        settings.keep_text_backups = true;
        write(temp.path(), &settings).unwrap();
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(temp.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(v["future"], 42);
        assert_eq!(read(temp.path()).info_panel_width, 480.);
        assert_eq!(v["window"]["bounds"][2], 1100);
        assert_eq!(read(temp.path()).theme, Appearance::Dark);
        assert!(!read(temp.path()).check_updates_on_startup);
        assert!(read(temp.path()).auto_save_text);
        assert!(read(temp.path()).keep_text_backups);
        assert!(!read(temp.path()).automatic_previews);
        assert!(!read(temp.path()).image_thumbnails);
    }
    #[test]
    fn palette_tint_takes_the_palettes_colour_once() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("settings.json");
        std::fs::write(&file, r#"{"glass_version":3,"palette":"harbor","shell_hue":215,"shell_saturation":6,"shell_lightness":6}"#).unwrap();
        let moved = read(temp.path());
        assert!(moved.shell_saturation > 30, "{}", moved.shell_saturation);
        assert_eq!(moved.glass_version, GLASS_VERSION);
        // Presets change how see-through the glass is, never its colour.
        let mut tinted = moved.clone();
        crate::appearance::Preset::Tinted.apply(&mut tinted);
        assert_eq!(tinted.shell_saturation, moved.shell_saturation);
    }
    #[test]
    fn glass_settings_migrate_and_bound_untrusted_values() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("settings.json"), r#"{"glass":true,"shell_hue":65535,"shell_saturation":255,"shell_lightness":255,"sidebar_opacity":0,"background_opacity":255}"#).unwrap();
        let settings = read(temp.path());
        assert_eq!(
            (
                settings.shell_hue,
                settings.shell_saturation,
                settings.shell_lightness
            ),
            (360, 100, 30)
        );
        assert_eq!(
            (settings.sidebar_opacity, settings.background_opacity),
            (0, 30)
        );
        assert!(settings.main_pane_glass);
        assert!(settings.background_image.is_none());
        let defaults = Settings::default();
        assert_eq!(defaults.shell_lightness, 8, "glass is a tint, not black");
        assert_eq!(defaults.glass_version, GLASS_VERSION);
    }
    #[test]
    fn old_black_glass_becomes_the_frost_tint_once_and_chosen_tints_stay() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("settings.json");
        std::fs::write(&file, r#"{"glass":true,"glass_opacity":72,"shell_hue":0,"shell_saturation":0,"shell_lightness":0,"sidebar_opacity":0,"background_blur":false}"#).unwrap();
        let settings = read(temp.path());
        let fresh = Settings::default();
        assert_eq!(
            (
                settings.shell_hue,
                settings.shell_saturation,
                settings.shell_lightness
            ),
            (
                fresh.shell_hue,
                fresh.shell_saturation,
                fresh.shell_lightness
            )
        );
        assert_eq!(settings.glass_opacity, 62, "capped to the preset's opacity");
        assert_eq!(settings.background_effect, crate::background::Effect::None);
        assert_eq!(settings.glass_version, GLASS_VERSION);
        // A tint someone picked survives, and the migration runs only once.
        std::fs::write(
            &file,
            r#"{"shell_hue":30,"shell_saturation":40,"shell_lightness":12}"#,
        )
        .unwrap();
        let chosen = read(temp.path());
        assert_eq!(
            (
                chosen.shell_hue,
                chosen.shell_saturation,
                chosen.shell_lightness
            ),
            (30, 40, 12)
        );
        std::fs::write(
            &file,
            format!(
                r#"{{"glass_version":{GLASS_VERSION},"shell_saturation":0,"shell_lightness":0}}"#
            ),
        )
        .unwrap();
        assert_eq!(
            read(temp.path()).shell_lightness,
            0,
            "a later black choice is kept"
        );
    }
    #[test]
    fn a_temporary_windows_reader_lock_does_not_lose_the_latest_preferences() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::default();
        write(dir.path(), &settings).unwrap();
        let reader = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(dir.path().join("settings.json"))
            .unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(25));
            drop(reader);
        });
        let mut changed = settings;
        changed.glass_opacity = 61;
        write_with_retry(dir.path(), &changed).unwrap();
        release.join().unwrap();
        assert_eq!(read(dir.path()).glass_opacity, 61);
    }

    #[test]
    fn sidebar_fold_and_locations_preferences_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        assert!(!settings.favorites_collapsed);
        assert!(!settings.locations_collapsed);
        settings.favorites_collapsed = true;
        settings.locations_collapsed = true;
        settings.location_order = vec![PathBuf::from(r"D:\"), PathBuf::from(r"C:\")];
        settings.hidden_locations = vec![PathBuf::from(r"E:\")];
        write(dir.path(), &settings).unwrap();
        let loaded = read(dir.path());
        assert!(loaded.favorites_collapsed);
        assert!(loaded.locations_collapsed);
        assert_eq!(loaded.location_order, settings.location_order);
        assert_eq!(loaded.hidden_locations, settings.hidden_locations);
    }
}
