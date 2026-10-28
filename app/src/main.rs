#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
extern crate windows_core;
mod app_icon;
mod appearance;
mod assets;
mod backend;
mod browse;
mod child_job;
mod content;
#[cfg(feature = "benchmarks")]
mod content_audit;
mod content_catalog;
#[cfg(feature = "benchmarks")]
mod document_audit;
mod document_preview;
mod file_changes;
mod file_ops;
mod file_progress;
mod highlights;
mod icons;
mod instance;
mod kinds;
mod location;
mod media;
mod metrics;
mod ole_drag;
mod pdf_session;
mod pdf_text;
mod presentation;
mod preview_cache;
mod recycle_guard;
mod selection;
mod settings;
mod shell;
mod spike;
mod text_cache;
mod text_decode;
mod tree;
mod updater;
mod window_state;
use gpui::*;
use std::path::PathBuf;

fn argument(name: &str) -> Option<String> {
    let args: Vec<_> = std::env::args().collect();
    args.windows(2).find(|a| a[0] == name).map(|a| a[1].clone())
}
fn main() {
    if std::env::args().any(|a| a == "--content-catalog-host") {
        std::process::exit(content_catalog::host());
    }
    #[cfg(feature = "benchmarks")]
    if let Some(manifest) = argument("--content-audit") {
        std::process::exit(content_audit::run(&manifest));
    }
    #[cfg(feature = "benchmarks")]
    if let Some(manifest) = argument("--document-audit") {
        std::process::exit(document_audit::run(&manifest));
    }
    if std::env::args().any(|a| a == "--content-extract-pdf") {
        std::process::exit(pdf_text::run());
    }
    if std::env::args().any(|a| a == "--document-preview-host") {
        std::process::exit(document_preview::host());
    }
    metrics::init(std::time::Instant::now());
    metrics::phase("entry");
    if std::env::args().any(|a| a == "--spike") {
        spike::run();
        return;
    }
    let root = argument("--root").map(PathBuf::from);
    let data = argument("--data").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Seek")
    });
    let saved_window = window_state::load(data.clone());
    let instance = match instance::acquire(&data) {
        Ok(Some(instance)) => instance,
        Ok(None) => return,
        Err(error) => {
            eprintln!("Cannot establish Seek instance: {error}");
            return;
        }
    };
    let focus_requests = instance.focus.clone();
    let source = if std::env::args().any(|a| a == "--walker") {
        seek_engine::SourceMode::Walker
    } else {
        seek_engine::SourceMode::Auto
    };
    let (startup_tx, startup_rx) = async_channel::bounded(1);
    let (atlas_tx, atlas_rx) = async_channel::bounded(1);
    let (shell_atlas_tx, shell_atlas_rx) = async_channel::bounded(1);
    let startup_data = data.clone();
    let startup_root = root;
    std::thread::Builder::new()
        .name("seek-assets-settings".into())
        .spawn(move || {
            let settings = settings::read(&startup_data);
            let (backend, updates, _worker) = backend::start(
                startup_root,
                startup_data,
                source,
                settings.indexed_locations.clone(),
            );
            metrics::phase("backend_spawned");
            let _ = startup_tx.send_blocking((settings, backend, updates));
            let atlas = icons::Atlas::bundled();
            metrics::phase("startup_assets_ready");
            let _ = atlas_tx.send_blocking(atlas.clone());
            let _ = shell_atlas_tx.send_blocking(atlas.with_shell());
        })
        .expect("startup assets");
    let application = gpui_platform::application();
    metrics::phase("gpui_application_created");
    application.with_assets(assets::Assets).run(move |cx| {
        metrics::phase("gpui_finish_launching");
        gpui_component::init(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-t", shell::NewTab, None),
            KeyBinding::new("ctrl-w", shell::CloseTab, None),
            KeyBinding::new("ctrl-tab", shell::NextTab, None),
            KeyBinding::new("ctrl-shift-tab", shell::PreviousTab, None),
            KeyBinding::new("right", shell::ExpandFolder, Some("DataTable || SeekFiles")),
            KeyBinding::new(
                "left",
                shell::CollapseFolder,
                Some("DataTable || SeekFiles"),
            ),
            KeyBinding::new("ctrl-c", shell::CopyFiles, Some("DataTable || SeekFiles")),
            KeyBinding::new("ctrl-x", shell::CutFiles, Some("DataTable || SeekFiles")),
            KeyBinding::new("ctrl-v", shell::PasteFiles, Some("DataTable || SeekFiles")),
            KeyBinding::new(
                "ctrl-shift-c",
                shell::CopyPath,
                Some("DataTable || SeekFiles"),
            ),
            KeyBinding::new("f2", shell::RenameFile, Some("DataTable || SeekFiles")),
            KeyBinding::new(
                "shift-delete",
                shell::DeletePermanently,
                Some("DataTable || SeekFiles"),
            ),
            KeyBinding::new(
                "delete",
                shell::RecycleFiles,
                Some("DataTable || SeekFiles"),
            ),
        ]);
        metrics::phase("kit_initialized");
        cx.spawn(async move |cx| {
            let (settings, backend, updates) = startup_rx.recv().await.expect("startup settings");
            let store = settings::Store::new(data.clone(), settings.clone());
            let (media, media_updates) = media::Worker::start(data.clone());
            let saved = saved_window.recv().await.ok().flatten();
            let atlas = atlas_rx.recv().await.expect("startup artwork");
            cx.update(move |cx| {
                cx.on_window_closed({
                    let backend = backend.clone();
                    move |cx, _| {
                        if cx.windows().is_empty() {
                            backend
                                .generation
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let _ = backend.tx.send(backend::Command::Stop);
                            cx.quit();
                        }
                    }
                })
                .detach();
                let saved_settings = store.value.clone();
                let settings_dir = data.clone();
                cx.on_app_quit(move |cx| {
                    let value = saved_settings.lock().unwrap().clone();
                    let data = settings_dir.clone();
                    cx.background_executor().spawn(async move {
                        let _ = settings::write(&data, &value);
                    })
                })
                .detach();
                let bounds = window_state::initial(saved, cx);
                metrics::phase("window_bounds_ready");
                let remembered = std::sync::Arc::new(std::sync::Mutex::new(bounds));
                window_state::persist_on_quit(remembered.clone(), data, cx);
                let handle = cx
                    .open_window(
                        WindowOptions {
                            window_bounds: Some(bounds),
                            window_min_size: Some(size(px(960.), px(550.))),
                            titlebar: Some(TitlebarOptions {
                                title: Some("Seek".into()),
                                appears_transparent: true,
                                ..Default::default()
                            }),
                            app_owns_titlebar_drag: true,
                            window_background: if !settings.glass {
                                WindowBackgroundAppearance::Opaque
                            } else if std::env::args().any(|a| a == "--mica") {
                                WindowBackgroundAppearance::MicaBackdrop
                            } else {
                                WindowBackgroundAppearance::Blurred
                            },
                            ..Default::default()
                        },
                        move |window, cx| {
                            instance::round(window);
                            app_icon::apply(window);
                            appearance::apply(&settings, Some(window), cx);
                            shell::root(
                                shell::Startup {
                                    backend,
                                    updates,
                                    remembered,
                                    preferences: settings,
                                    store,
                                    atlas,
                                    shell_atlas_rx,
                                    media,
                                    media_updates,
                                },
                                window,
                                cx,
                            )
                        },
                    )
                    .expect("open Seek window");
                metrics::phase("window_opened");
                cx.activate(true);
                cx.spawn(async move |cx| {
                    while focus_requests.recv().await.is_ok() {
                        let _ = handle.update(cx, |_, window, _| instance::restore(window));
                    }
                })
                .detach();
            });
        })
        .detach();
    });
    drop(instance);
}
