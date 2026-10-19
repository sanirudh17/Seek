use gpui::*;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub type Remembered = Arc<Mutex<WindowBounds>>;

pub fn load(data: PathBuf) -> async_channel::Receiver<Option<WindowBounds>> {
    let (tx, rx) = async_channel::bounded(1);
    std::thread::Builder::new()
        .name("seek-window-load".into())
        .spawn(move || {
            let value = std::fs::read(data.join("settings.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
            let bounds = value.and_then(|v| {
                let coordinates: [f32; 4] =
                    serde_json::from_value(v["window"]["bounds"].clone()).ok()?;
                let [x, y, width, height] = coordinates;
                if !coordinates.iter().all(|n| n.is_finite())
                    || !(800.0..=10000.0).contains(&width)
                    || !(450.0..=10000.0).contains(&height)
                {
                    return None;
                }
                let bounds = Bounds::new(point(px(x), px(y)), size(px(width), px(height)));
                Some(if v["window"]["maximized"].as_bool().unwrap_or(false) {
                    WindowBounds::Maximized(bounds)
                } else {
                    WindowBounds::Windowed(bounds)
                })
            });
            let _ = tx.send_blocking(bounds);
        })
        .expect("start window settings reader");
    rx
}

pub fn initial(saved: Option<WindowBounds>, cx: &App) -> WindowBounds {
    saved
        .filter(|saved| {
            // Keep a draggable part of the caption on a connected monitor.
            let bounds = saved.get_bounds();
            let caption = Bounds::new(bounds.origin, size(bounds.size.width, px(32.)));
            cx.displays()
                .iter()
                .any(|display| caption.intersects(&display.bounds()))
        })
        .unwrap_or_else(|| WindowBounds::centered(size(px(1100.), px(700.)), cx))
}

pub fn persist_on_quit(remembered: Remembered, data: PathBuf, cx: &mut App) {
    // GPUI waits for this small background task on quit, with its shutdown timeout.
    // A crawler/checkpoint is never joined here.
    cx.on_app_quit(move |cx| {
        let bounds = *remembered.lock().unwrap();
        let data = data.clone();
        cx.background_executor().spawn(async move {
            if let Err(error) = save(bounds, data) {
                eprintln!("Cannot save window settings: {error}");
            }
        })
    })
    .detach();
}

fn save(saved: WindowBounds, data: PathBuf) -> std::io::Result<()> {
    let _guard = crate::settings::FILE_LOCK.lock().unwrap();
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    std::fs::create_dir_all(&data)?;
    let path = data.join("settings.json");
    let temp = data.join("settings.json.tmp");
    let mut value = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    let bounds = saved.get_bounds();
    value["window"] = serde_json::json!({
        "bounds": [f32::from(bounds.origin.x), f32::from(bounds.origin.y), f32::from(bounds.size.width), f32::from(bounds.size.height)],
        "maximized": matches!(saved, WindowBounds::Maximized(_)),
    });
    std::fs::write(&temp, serde_json::to_vec_pretty(&value)?)?;
    let wide = |p: &std::path::Path| {
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
