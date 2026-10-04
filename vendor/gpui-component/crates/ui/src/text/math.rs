//! Pluggable math rendering for [`TextView`](super::TextView).
//!
//! The component library only forwards parsed formulas. The application owns
//! the TeX engine, layout, rasterization, and caches. Without a renderer, or
//! when it returns `None`, TextView keeps its source-text fallback.

use std::rc::Rc;

use gpui::{AnyElement, App, Global, SharedString, Window};

/// A math formula found in the source document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathSpec {
    /// TeX source between the dollar fences (fences excluded).
    pub source: SharedString,
    /// `true` for `$$...$$` display blocks, `false` for `$...$` inline math.
    pub display: bool,
}

type MathRenderFn = dyn Fn(&MathSpec, &mut Window, &mut App) -> Option<AnyElement>;

struct MathRenderer(Rc<MathRenderFn>);

impl Global for MathRenderer {}

/// Register the app-wide math renderer used by every TextView.
pub fn set_math_renderer(
    cx: &mut App,
    render: impl Fn(&MathSpec, &mut Window, &mut App) -> Option<AnyElement> + 'static,
) {
    cx.set_global(MathRenderer(Rc::new(render)));
}

pub(crate) fn render_math(
    spec: &MathSpec,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let render = cx
        .try_global::<MathRenderer>()
        .map(|renderer| renderer.0.clone())?;
    (render)(spec, window, cx)
}
