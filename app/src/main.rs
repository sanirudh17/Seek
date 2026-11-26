#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
extern crate windows_core;
mod app_icon;
mod archive;
mod appearance;
mod assets;
mod backend;
mod background;
mod batch_rename;
mod browse;
mod bug_report;
mod child_job;
mod cleanup;
mod cloud;
mod code_highlight;
mod content;
#[cfg(feature = "benchmarks")]
mod content_audit;
mod content_catalog;
#[cfg(feature = "benchmarks")]
mod document_audit;
mod document_preview;
mod duplicates;
mod env_secrets;
mod explorer_menu;
mod file_changes;
mod file_info;
mod file_ops;
mod file_progress;
mod highlights;
mod icon_view;
mod icons;
mod image_tools;
mod image_http;
mod inspector;
mod instance;
mod kinds;
mod location;
mod markdown_extra;
mod markdown_images;
mod markdown_tasks;
mod mcp;
mod media;
mod metrics;
mod motion;
mod media_date;
mod naming;
mod native_share;
mod ole_drag;
mod operation_control;
mod operation_journal;
mod operation_snapshot;
mod organize;
pub mod pane_layout;
mod pane_session;
mod pdf_session;
mod pdf_text;
mod picture_edit;
mod prettify;
mod preview_cache;
mod preview_diagnostics;
mod recycle_guard;
mod rename_steps;
mod safe_rename;
mod saved_commands;
mod selection;
mod settings;
mod similar;
mod shell;
mod shortcuts;
mod sidebar_groups;
mod spike;
mod text_cache;
mod text_decode;
mod text_document;
mod tokens;
mod tree;
mod ui_icons;
mod updater;
mod window_state;
mod worktrees;
use gpui::*;
use std::path::PathBuf;

fn argument(name: &str) -> Option<String> {
    let args: Vec<_> = std::env::args().collect();
    args.windows(2).find(|a| a[0] == name).map(|a| a[1].clone())
}
fn main() {
    if std::env::args().any(|a| a == "--preview-self-test") {
        std::process::exit(preview_diagnostics::self_test());
    }
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
        preview_diagnostics::host_stage("host_entry", serde_json::json!({}));
        std::process::exit(document_preview::host());
    }
    if std::env::args().any(|a| a == "--mcp") {
        let data = argument("--data").map(PathBuf::from).unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Seek")
        });
        std::process::exit(mcp::serve_stdio(data, argument("--profile")));
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
    // "Open in Seek" / "Show in Seek" from Explorer, and folders opened while
    // Seek is the default file manager: seek.exe --open <folder or file>.
    // What isn't a file or folder (This PC, Control Panel, Recycle Bin,
    // shell: places) goes to File Explorer, so nothing stops opening.
    let open_argument = argument("--open");
    if let Some(target) = &open_argument
        && !std::path::Path::new(target).exists()
    {
        let _ = std::process::Command::new(
            PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
                .join("explorer.exe"),
        )
        .arg(target)
        .spawn();
        return;
    }
    let open_folder = open_argument.map(PathBuf::from);
    let instance = match instance::acquire(&data, open_folder.as_deref()) {
        Ok(Some(instance)) => instance,
        Ok(None) => return,
        Err(error) => {
            eprintln!("Cannot establish Seek instance: {error}");
            return;
        }
    };
    let own_profile = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Seek");
    let refresh_explorer_menu = data.as_os_str().eq_ignore_ascii_case(own_profile.as_os_str());
    let focus_requests = instance.focus.clone();
    let (open_tx, open_rx) = async_channel::unbounded::<PathBuf>();
    if let Some(folder) = open_folder {
        let _ = open_tx.try_send(folder);
    }
    let request_file = instance::request_file(&data);
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
            let session = pane_session::load(&startup_data);
            let (backend, updates, _worker) = backend::start(
                startup_root,
                startup_data,
                source,
                settings.indexed_locations.clone(),
            );
            metrics::phase("backend_spawned");
            let _ = startup_tx.send_blocking((settings, backend, updates, session));
            let atlas = icons::Atlas::bundled();
            metrics::phase("startup_assets_ready");
            let _ = atlas_tx.send_blocking(atlas.clone());
            // Refresh Explorer context menu registration off the critical
            // startup path so registry I/O never delays first window paint.
            if refresh_explorer_menu {
                explorer_menu::refresh();
            }
            let _ = shell_atlas_tx.send_blocking(atlas.with_shell());
        })
        .expect("startup assets");
    let application = gpui_platform::application();
    metrics::phase("gpui_application_created");
    application.with_assets(assets::Assets).run(move |cx| {
        metrics::phase("gpui_finish_launching");
        cx.text_system()
            .add_fonts(vec![
                std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Geist.ttf").as_slice()),
                std::borrow::Cow::Borrowed(
                    include_bytes!("../assets/fonts/Geist-Medium.ttf").as_slice(),
                ),
                std::borrow::Cow::Borrowed(
                    include_bytes!("../assets/fonts/Geist-SemiBold.ttf").as_slice(),
                ),
                std::borrow::Cow::Borrowed(
                    include_bytes!("../assets/fonts/GeistMono.ttf").as_slice(),
                ),
                std::borrow::Cow::Borrowed(
                    include_bytes!("../assets/fonts/GeistMono-Medium.ttf").as_slice(),
                ),
            ])
            .expect("bundled Geist fonts");
        cx.set_http_client(std::sync::Arc::new(image_http::Client(
            cx.background_executor().clone(),
        )));
        gpui_component::init(cx);
        cx.bind_keys([
            KeyBinding::new("shift-left", shell::PanImageLeft, Some("SeekImagePreview")),
            KeyBinding::new(
                "shift-right",
                shell::PanImageRight,
                Some("SeekImagePreview"),
            ),
            KeyBinding::new("shift-up", shell::PanImageUp, Some("SeekImagePreview")),
            KeyBinding::new("shift-down", shell::PanImageDown, Some("SeekImagePreview")),
            KeyBinding::new("ctrl-t", shell::NewTab, None),
            KeyBinding::new("ctrl-w", shell::CloseTab, None),
            KeyBinding::new("ctrl-tab", shell::NextTab, None),
            KeyBinding::new("ctrl-shift-tab", shell::PreviousTab, None),
            KeyBinding::new("ctrl-\\", shell::SplitRight, None),
            KeyBinding::new("ctrl-shift-\\", shell::SplitDown, None),
            KeyBinding::new("ctrl-|", shell::SplitDown, None),
            KeyBinding::new("ctrl-shift-w", shell::ClosePane, None),
            KeyBinding::new("f6", shell::NextPane, None),
            KeyBinding::new("shift-f6", shell::PreviousPane, None),
            KeyBinding::new("alt-f5", shell::CopyToOtherPane, None),
            KeyBinding::new("alt-f6", shell::MoveToOtherPane, None),
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
            KeyBinding::new("ctrl-shift-o", shell::ShowInFolder, None),
            KeyBinding::new("f2", shell::RenameFile, Some("DataTable || SeekFiles")),
            KeyBinding::new("ctrl-shift-n", shell::NewFolder, None),
            KeyBinding::new("ctrl-b", shell::ToggleSidebar, None),
            KeyBinding::new("ctrl-z", shell::UndoFile, Some("DataTable || SeekFiles")),
            KeyBinding::new(
                "ctrl-shift-z",
                shell::RedoFile,
                Some("DataTable || SeekFiles"),
            ),
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
            let (settings, backend, updates, session) =
                startup_rx.recv().await.expect("startup settings");
            let store = settings::Store::new(data.clone(), settings.clone());
            let (media, media_updates) = media::Worker::start(data.clone());
            let saved = saved_window.recv().await.ok().flatten();
            let atlas = atlas_rx.recv().await.expect("startup artwork");
            cx.update(move |cx| {
                cx.on_window_closed({
                    let backend = backend.clone();
                    move |cx, _| {
                        if cx.windows().is_empty() {
                            // A Seek started from now on waits for this one
                            // to finish saving and opens its own window.
                            instance::mark_closing();
                            // Save the index if it changed, without reloading
                            // the 200 MB map back into RAM before exit. Cap
                            // the wait and abort immediately if the user just
                            // relaunched Seek so reopening is always instant.
                            if let Some(engine) = backend::ENGINE.get() {
                                let (done, wait) = std::sync::mpsc::channel();
                                std::thread::spawn(move || {
                                    let _ = done.send(engine.checkpoint_for_shutdown());
                                });
                                let started = std::time::Instant::now();
                                while started.elapsed() < std::time::Duration::from_secs(3) {
                                    if instance::relaunch_requested() {
                                        engine.abort_shutdown();
                                        break;
                                    }
                                    if wait
                                        .recv_timeout(std::time::Duration::from_millis(25))
                                        .is_ok()
                                    {
                                        break;
                                    }
                                }
                            }
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
                // Establish saved theme tokens before constructing the native
                // HWND; factory applies caption/backdrop while it is hidden.
                appearance::apply(&settings, None, cx);
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
                            // Started for an agent (`--background`): don't take
                            // focus from what the person is doing.
                            focus: !std::env::args().any(|a| a == "--background"),
                            window_background: if !appearance::glass_enabled(
                                &settings,
                                gpui_component::Theme::global(cx).mode.is_dark(),
                            ) {
                                WindowBackgroundAppearance::Opaque
                            } else if std::env::args().any(|a| a == "--mica") {
                                WindowBackgroundAppearance::MicaBackdrop
                            } else {
                                WindowBackgroundAppearance::Blurred
                            },
                            ..Default::default()
                        },
                        move |window, cx| {
                            #[cfg(feature = "benchmarks")]
                            if let Some(delay) = argument("--startup-test-delay-ms")
                                .and_then(|s| s.parse::<u64>().ok())
                            {
                                std::thread::sleep(std::time::Duration::from_millis(
                                    delay.min(2000),
                                ));
                            }
                            instance::round(window);
                            app_icon::apply(window);
                            appearance::apply(&settings, Some(window), cx);
                            metrics::phase("saved_appearance_applied_while_hidden");
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
                                    session,
                                    open_requests: open_rx.clone(),
                                },
                                window,
                                cx,
                            )
                        },
                    )
                    .expect("open Seek window");
                if std::env::args().any(|a| a == "--background") {
                    let _ = handle.update(cx, |_, window, _| window.minimize_window());
                }
                metrics::phase("window_opened");
                cx.activate(true);
                cx.spawn(async move |cx| {
                    while focus_requests.recv().await.is_ok() {
                        let _ = handle.update(cx, |_, window, _| instance::restore(window));
                        if let Ok(text) = std::fs::read_to_string(&request_file) {
                            let _ = std::fs::remove_file(&request_file);
                            let target = PathBuf::from(text.trim());
                            if target.exists() {
                                let _ = open_tx.try_send(target);
                            }
                        }
                    }
                })
                .detach();
            });
        })
        .detach();
    });
    drop(instance);
}
