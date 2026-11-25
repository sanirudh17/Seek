use super::*;

impl Seek {
    pub(super) fn save_current_search(&mut self, cx: &mut Context<Self>) {
        let search = crate::settings::Search {
            query: self.input.read(cx).value().trim().into(),
            scope: self.scope.clone(),
            content: self.content_mode,
        };
        if search.query.is_empty() {
            return;
        }
        if search.query.len() > 4096 {
            self.message = Some("Saved queries can contain up to 4,096 bytes of text.".into());
            cx.notify();
            return;
        }
        if !search.content
            && let Err(error) = seek_engine::Query::parse(&search.query, "")
        {
            self.message = Some(format!("Fix the search before saving it: {error}"));
            cx.notify();
            return;
        }
        if self.preferences.saved_searches.len() == 32
            && !self.preferences.saved_searches.contains(&search)
        {
            self.message = Some("You have 32 saved searches. Remove one from its context menu before saving another.".into());
        } else {
            crate::settings::retain_search(&mut self.preferences.saved_searches, search, 32);
            self.store.save(&self.preferences);
            self.message = Some("Search saved, including its location and search mode.".into());
        }
        cx.notify();
    }
    pub(super) fn recall_search(
        &mut self,
        search: crate::settings::Search,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.remember(cx);
        self.settings = false;
        self.scope = search.scope;
        self.content_mode = search.content;
        self.largest = None;
        self.normal_results = None;
        self.all_offset = 0;
        self.all_total = None;
        self.input.update(cx, |input, cx| {
            input.set_value(search.query, window, cx);
            input.focus(window, cx);
        });
        self.submit(cx);
        self.remember(cx);
        cx.notify();
    }
}
