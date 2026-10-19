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
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Appearance,
    pub glass: bool,
    pub accent: String,
    pub show_hidden: bool,
    pub default_view: View,
    pub indexed_locations: Vec<PathBuf>,
    pub exclusions: Vec<PathBuf>,
    pub content_locations: Vec<PathBuf>,
    pub remember_document_text: bool,
    pub info_panel_width: f32,
    pub check_updates_on_startup: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Appearance::System,
            glass: true,
            accent: "#909090".into(),
            show_hidden: false,
            default_view: View::List,
            indexed_locations: vec![],
            exclusions: vec![],
            content_locations: crate::content::default_locations(),
            remember_document_text: true,
            info_panel_width: 280.,
            check_updates_on_startup: true,
        }
    }
}
pub fn accent(value: &str) -> Option<u32> {
    let value = value.strip_prefix('#').unwrap_or(value);
    (value.len() == 6 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(value, 16).ok())
        .flatten()
}
pub fn read(data: &Path) -> Settings {
    let mut settings: Settings = std::fs::read(data.join("settings.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    if accent(&settings.accent).is_none() {
        settings.accent = Settings::default().accent;
    }
    if settings.accent.eq_ignore_ascii_case("#707070") {
        settings.accent = "#909090".into();
    }
    settings.indexed_locations.truncate(64);
    settings.exclusions.truncate(256);
    settings.content_locations.truncate(64);
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
    pub value: Arc<Mutex<Settings>>,
    tx: std::sync::mpsc::SyncSender<()>,
}
impl Store {
    pub fn new(data: PathBuf, settings: Settings) -> Self {
        let value = Arc::new(Mutex::new(settings));
        let worker = value.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        thread::Builder::new()
            .name("seek-settings".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    while rx.try_recv().is_ok() {}
                    let _guard = FILE_LOCK.lock().unwrap();
                    let snapshot = worker.lock().unwrap().clone();
                    if let Err(e) = write_locked(&data, &snapshot) {
                        eprintln!("Cannot save settings: {e}");
                    }
                }
            })
            .expect("settings writer");
        Self { value, tx }
    }
    pub fn save(&self, settings: &Settings) {
        *self.value.lock().unwrap() = settings.clone();
        let _ = self.tx.try_send(());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
        settings.accent = "#aabbcc".into();
        write(temp.path(), &settings).unwrap();
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(temp.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(v["future"], 42);
        assert_eq!(read(temp.path()).info_panel_width, 480.);
        assert_eq!(v["window"]["bounds"][2], 1100);
        assert_eq!(read(temp.path()).theme, Appearance::Dark);
        assert!(!read(temp.path()).check_updates_on_startup);
    }
    #[test]
    fn invalid_accent_is_rejected() {
        assert_eq!(accent("#ff0088"), Some(0xff0088));
        for value in ["#fff", "red", "1234567", "$(bad)"] {
            assert_eq!(accent(value), None);
        }
    }
}
