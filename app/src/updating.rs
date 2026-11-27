use super::*;
fn log_update_error(data: &std::path::Path, action: &str, error: &str) {
    use std::io::Write;
    let logs = data.join("logs");
    let _ = std::fs::create_dir_all(&logs);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("updater.log"))
    {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{now}] {action} ({}): {error}", crate::updater::CURRENT);
    }
}

fn friendly_download_error(error: &str) -> String {
    if error.contains("timed out") {
        "Download timed out. Try again.".into()
    } else if error.contains("resolve") || error.contains("connect") || error.contains("reset") {
        "Couldn't connect to GitHub. Try again.".into()
    } else if error.contains("TLS") {
        "Secure connection failed. Try again.".into()
    } else if error.contains("checksum") {
        "Checksum didn't match. Try again.".into()
    } else if error.contains("HTTP ") {
        format!("{error}. Try again.")
    } else {
        "Couldn't download. Try again.".into()
    }
}

impl Seek {
    /// Opens a prefilled issue in the browser; nothing is sent from here.
    pub(super) fn report_bug(&mut self, cx: &mut Context<Self>) {
        let url = crate::bug_report::issue_url(
            crate::updater::CURRENT,
            &crate::bug_report::windows_version(),
            &crate::preview_diagnostics::path(),
        );
        cx.open_url(&url);
    }
    pub(super) fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        if self.app_updates.busy {
            return;
        }
        self.app_updates.busy = true;
        self.app_updates.failed = false;
        self.app_updates.message = "Checking…".into();
        let data = self.store.directory.clone();
        let work = cx
            .background_executor()
            .spawn(async { crate::updater::check() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.app_updates.busy = false;
                match result {
                    Ok(crate::updater::Check::Available(release)) => {
                        this.app_updates.failed = false;
                        this.app_updates.message = format!("{} available", release.version);
                        this.app_updates.available = Some(release);
                    }
                    Ok(crate::updater::Check::Current) => {
                        this.app_updates.failed = false;
                        this.app_updates.message = "Up to date".to_string();
                        this.app_updates.available = None;
                    }
                    Ok(crate::updater::Check::NoRelease) => {
                        this.app_updates.failed = false;
                        this.app_updates.message = "No release yet".into();
                        this.app_updates.available = None;
                    }
                    Err(error) => {
                        eprintln!("Could not check for updates: {error}");
                        log_update_error(&data, "check", &error);
                        this.app_updates.failed = true;
                        this.app_updates.message = "Couldn't check. Try again.".into()
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    /// Download and verify the update, then wait for Restart. The ring
    /// redraws only while bytes arrive; nothing repaints once it settles.
    pub(super) fn install_app_update(&mut self, cx: &mut Context<Self>) {
        if self.app_updates.busy {
            return;
        }
        if self.app_updates.staged.is_some() {
            self.restart_into_update(cx);
            return;
        }
        let Some(release) = self.app_updates.available.clone() else {
            return;
        };
        self.app_updates.busy = true;
        self.app_updates.downloading = true;
        self.app_updates.failed = false;
        self.app_updates
            .progress
            .store(0, std::sync::atomic::Ordering::Relaxed);
        self.app_updates.message = format!("Downloading Seek {}…", release.version);
        let data = self.store.directory.clone();
        let progress = self.app_updates.progress.clone();
        let executor = cx.background_executor().clone();
        let work = cx.background_executor().spawn(async move {
            if release.preview {
                // Debug preview: walk the ring to full, then stage nothing.
                for step in 1..=40u64 {
                    executor.timer(std::time::Duration::from_millis(60)).await;
                    progress.store(
                        release.size * step / 40,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                }
                return Ok(std::path::PathBuf::new());
            }
            crate::updater::prepare(release, &progress)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.app_updates.busy = false;
                this.app_updates.downloading = false;
                match result {
                    Ok(stage) => {
                        this.app_updates.staged = Some(stage);
                        this.app_updates.popover = true;
                        this.app_updates.failed = false;
                        this.app_updates.message = "Ready to install".into();
                    }
                    Err(error) => {
                        eprintln!("Update was not installed: {error}");
                        log_update_error(&data, "download", &error);
                        this.app_updates.failed = true;
                        this.app_updates.message = friendly_download_error(&error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        // Redraw the ring while the download runs; the loop ends with it.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(120))
                    .await;
                let Ok(running) = this.update(cx, |this, cx| {
                    cx.notify();
                    this.app_updates.downloading
                }) else {
                    break;
                };
                if !running {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }
    pub(super) fn restart_into_update(&mut self, cx: &mut Context<Self>) {
        if self.app_updates.busy {
            return;
        }
        let Some(stage) = self.app_updates.staged.clone() else {
            return;
        };
        if self
            .app_updates
            .available
            .as_ref()
            .is_some_and(|release| release.preview)
        {
            self.app_updates.staged = None;
            self.app_updates.popover = false;
            self.app_updates.message = String::new();
            self.message = Some("Update preview finished. Nothing was installed.".into());
            cx.notify();
            return;
        }
        self.app_updates.busy = true;
        self.app_updates.failed = false;
        self.app_updates.message = "Installing update…".into();
        let data = self.store.directory.clone();
        let work = cx
            .background_executor()
            .spawn(async move { crate::updater::launch(&stage) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.app_updates.busy = false;
                match result {
                    Ok(()) => cx.quit(),
                    Err(error) => {
                        log_update_error(&data, "launch", &error);
                        this.app_updates.failed = true;
                        this.app_updates.message = format!("Update was not installed: {error}");
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }
    /// The update icon at the sidebar's foot, when an update exists; its
    /// popover shows the release notes, the download ring, then Restart.
    pub(super) fn update_indicator(
        &self,
        p: crate::appearance::Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        use gpui_component::popover::Popover;
        let release = self.app_updates.available.clone()?;
        let state = &self.app_updates;
        let ready = state.staged.is_some();
        let downloading = state.downloading;
        let installing = state.busy && !downloading && ready;
        let fraction = state.fraction();
        let message = if ready || installing || state.failed || state.message.starts_with("Couldn't") {
            state.message.clone()
        } else {
            String::new()
        };
        let trigger = Button::new("update-indicator")
            .ghost()
            .h(px(22.))
            .w(px(22.))
            .p_0()
            .rounded(px(6.))
            .accessibility_label(format!("Seek {} is available", release.version))
            .tooltip(format!("Seek {} is available", release.version))
            .child(if downloading {
                gpui_component::progress::ProgressCircle::new("update-ring")
                    .value(fraction * 100.)
                    .color(p.accent)
                    .size(px(13.))
                    .into_any_element()
            } else {
                Icon::empty()
                    .path("seek/update-circle.svg")
                    .size(px(14.))
                    .text_color(if ready { p.tints.green } else { p.tints.blue })
                    .into_any_element()
            });
        let owner = cx.entity().downgrade();
        let open_owner = owner.clone();
        Some(
            Popover::new("update-popover")
                .anchor(gpui::Anchor::BottomLeft)
                .offset(px(8.))
                .open(state.popover)
                .on_open_change(move |open, _, cx| {
                    let _ = open_owner.update(cx, |this, cx| {
                        this.app_updates.popover = *open;
                        cx.notify();
                    });
                })
                .trigger(trigger)
                .content(move |_, _, cx| {
                    let owner = owner.clone();
                    let notes: Vec<String> = release
                        .notes
                        .lines()
                        .map(|line| line.trim().trim_start_matches(['-', '*', '•']).trim())
                        .filter(|line| !line.is_empty() && !line.starts_with('#'))
                        .take(8)
                        .map(str::to_string)
                        .collect();
                    let action = super::settings_view::control(
                        "update-action",
                        if installing {
                            "Installing…"
                        } else if ready {
                            "Restart"
                        } else {
                            "Download"
                        },
                        p,
                        cx,
                    )
                    .h(px(28.))
                    .with_size(px(28.))
                    .px_3()
                    .text_size(px(13.))
                    .disabled(installing)
                    .on_click(move |_, _, cx| {
                        let _ = owner.update(cx, |this, cx| this.install_app_update(cx));
                    });
                    let status: AnyElement = if downloading {
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(12.))
                            .text_color(p.muted)
                            .child(format!("{:.0}%", fraction * 100.))
                            .child(
                                gpui_component::progress::ProgressCircle::new("update-ring-large")
                                    .value(fraction * 100.)
                                    .color(p.accent)
                                    .size(px(18.)),
                            )
                            .into_any_element()
                    } else {
                        action.into_any_element()
                    };
                    div()
                        .w(px(300.))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(p.text)
                                                .child(format!("Seek {}", release.version)),
                                        )
                                        .when(!message.is_empty() && !downloading, |d| {
                                            d.child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(p.muted)
                                                    .child(message.clone()),
                                            )
                                        }),
                                )
                                .child(status),
                        )
                        .when(!notes.is_empty(), |d| {
                            d.child(div().h(px(1.)).bg(p.line)).child(
                                div()
                                    .id("update-notes")
                                    .max_h(px(200.))
                                    .overflow_y_scroll()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .children(notes.into_iter().map(move |line| {
                                        div()
                                            .flex()
                                            .gap_2()
                                            .text_size(px(12.))
                                            .line_height(px(18.))
                                            .text_color(p.text.opacity(0.8))
                                            .child(
                                                div()
                                                    .mt(px(7.))
                                                    .size(px(4.))
                                                    .flex_shrink_0()
                                                    .rounded(px(2.))
                                                    .bg(p.muted),
                                            )
                                            .child(div().flex_1().min_w_0().child(line))
                                    })),
                            )
                        })
                })
                .into_any_element(),
        )
    }
}
