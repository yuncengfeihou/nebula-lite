#![cfg(feature = "test-support")]

use gpui::{AppContext as _, Context, IntoElement, Render, TestAppContext, Window, div};
use gpui_component::Root;

struct RootProbe;

impl Render for RootProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[gpui::test]
fn root_renders_without_a_native_window(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| RootProbe);
        Root::new(view, window, cx)
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}
