use gpui::*;
use gpui_component::{
    Root, Sizable,
    input::{Input, InputState},
    table::{Column, DataTable as Table, TableDelegate, TableState},
};

struct SpikeRows {
    columns: Vec<Column>,
}
impl TableDelegate for SpikeRows {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }
    fn rows_count(&self, _: &App) -> usize {
        100_000
    }
    fn column(&self, i: usize, _: &App) -> Column {
        self.columns[i].clone()
    }
    fn render_td(
        &mut self,
        row: usize,
        col: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div().child(match col {
            0 => format!("atlas-notes-{row:05}.txt"),
            1 => "11 Oct 2026, 10:30".into(),
            2 => "1 KB".into(),
            _ => "Text document".into(),
        })
    }
}

struct Spike {
    input: Entity<InputState>,
    table: Entity<TableState<SpikeRows>>,
}

impl Render for Spike {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .font_family("Segoe UI Variable Text")
            .text_color(rgb(0x17212b))
            .bg(rgba(0xf8fafc80))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .p_4()
                    .child("Seek · GPUI spike · 100,000 synthetic rows")
                    .child(div().flex_1().child(Input::new(&self.input))),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .px_4()
                    .h(px(80.))
                    .child(
                        img(std::sync::Arc::new(Image::from_bytes(
                            ImageFormat::Png,
                            include_bytes!("../assets/spike.png").to_vec(),
                        )))
                        .size(px(64.)),
                    )
                    .child("Input, image and virtual list; glass behind the window"),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(Table::new(&self.table).stripe(true).bordered(false).small()),
            )
            .child(
                div()
                    .h(px(28.))
                    .px_4()
                    .child("100,000 virtual rows · Foundation spike"),
            )
    }
}

pub fn run() {
    gpui_platform::application()
        .with_assets(gpui_component_assets::Assets)
        .run(|cx| {
            gpui_component::init(cx);
            gpui_component::Theme::global_mut(cx).tokens.background =
                Hsla::from(rgba(0x00000000)).into();
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::centered(size(px(1100.), px(700.)), cx)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Seek — GPUI spike".into()),
                        ..Default::default()
                    }),
                    window_background: if std::env::args().any(|a| a == "--mica") {
                        WindowBackgroundAppearance::MicaBackdrop
                    } else {
                        WindowBackgroundAppearance::Blurred
                    },
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| Spike {
                        input: cx
                            .new(|cx| InputState::new(window, cx).placeholder("Search filenames")),
                        table: cx.new(|cx| {
                            TableState::new(
                                SpikeRows {
                                    columns: vec![
                                        Column::new("name", "Name").width(px(440.)),
                                        Column::new("mtime", "Date Modified").width(px(220.)),
                                        Column::new("size", "Size").width(px(100.)),
                                        Column::new("kind", "Kind").width(px(180.)),
                                    ],
                                },
                                window,
                                cx,
                            )
                        }),
                    });
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .expect("open GPUI spike");
            cx.activate(true);
        });
}
