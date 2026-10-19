use super::*;
impl Seek {
    pub(super) fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        if self.app_updates.busy {
            return;
        }
        self.app_updates.busy = true;
        self.app_updates.message = "Checking GitHub...".into();
        let work = cx
            .background_executor()
            .spawn(async { crate::updater::check() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.app_updates.busy = false;
                match result {
                    Ok(crate::updater::Check::Available(release)) => {
                        this.app_updates.message =
                            format!("Seek {} is available.", release.version);
                        this.app_updates.available = Some(release);
                        if this.message.is_none() {
                            this.message =
                                Some("An update is available in Settings > Updates.".into());
                        }
                    }
                    Ok(crate::updater::Check::Current) => {
                        this.app_updates.message =
                            format!("Seek {} is up to date.", crate::updater::CURRENT);
                        this.app_updates.available = None;
                    }
                    Ok(crate::updater::Check::NoRelease) => {
                        this.app_updates.message =
                            "No published release yet. Try again later.".into();
                        this.app_updates.available = None;
                    }
                    Err(error) => {
                        this.app_updates.message = format!("Could not check for updates: {error}")
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn install_app_update(&mut self, cx: &mut Context<Self>) {
        if self.app_updates.busy {
            return;
        }
        let Some(release) = self.app_updates.available.clone() else {
            return;
        };
        self.app_updates.busy = true;
        self.app_updates.message =
            "Downloading and verifying update. Seek will restart when ready...".into();
        let work = cx.background_executor().spawn(async move {
            let stage = crate::updater::prepare(release)?;
            crate::updater::launch(&stage)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => cx.quit(),
                Err(error) => {
                    this.app_updates.busy = false;
                    this.app_updates.message = format!("Update was not installed: {error}");
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }
}
