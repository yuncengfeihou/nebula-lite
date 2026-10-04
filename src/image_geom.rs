//! 图片预览的缩放 / 平移几何（纯数学，可单测）。
//!
//! 逐行移植 Pebrel 1.9.1 的 `nebula_app/src/display/image_viewer.rs`：那张
//! `ImageView` 的几何数学是它与旧 OpenGL 壳共用的同一份（文件头明确写了
//! "GPUI 壳按它摆放图片，与 draw 的旧壳渲染同一份数学"），所以我们照抄它而不是
//! 另创一套锚点/钳制规则。
//!
//! 关键约定（改之前先读懂，这几条决定了手感）：
//! - **基准缩放**是把图片**完整装进视图**的比例（contain：`min(视宽/图宽, 视高/图高)`）；
//!   `zoom` 是相对这个基准的倍率。Pebrel 原版是"铺满视图宽度"，那对宽图合适、对
//!   竖图（9:16）会把底部裁掉、必须手动缩小；改成 contain 后整张图打开即可见。
//!   见 `draw_size`。
//! - **缩放围绕指针**：指针下的那个像素在缩放前后相对图片的位置保持不变
//!   （`zoom_by` 里那四个 u/v）。没有这一步，滚轮缩放会像"图片在跑"。
//! - **平移钳在图片边缘**：看不到留白，图片小于视图时居中。
//!
//! 用的是 `f32` 而非 gpui 的 `Pixels`：本模块只关心数值，调用侧负责
//! `Bounds<Pixels>` ↔ 元组的换算，这样几何可以脱离窗口单测。

/// 最小 / 最大缩放倍率，取 Pebrel 的同名常量。
pub const MIN_ZOOM: f32 = 0.25;
pub const MAX_ZOOM: f32 = 8.0;
/// 每一"档"滚轮的缩放倍率（指数底数），取 Pebrel 的同名常量。
pub const ZOOM_PER_STEP: f32 = 1.18;

/// 视图区域：`(x, y, w, h)`，与 Pebrel 的 `area` 元组同布局。
pub type Area = (f32, f32, f32, f32);

/// 图片预览的缩放 / 平移状态。
#[derive(Clone, Debug, PartialEq)]
pub struct ImageGeometry {
    /// 原图像素尺寸；`None` = 还没解码出尺寸（缩放无从谈起）。
    dimensions: Option<(u32, u32)>,
    zoom: f32,
    pan: (f32, f32),
    drag_last: Option<(f32, f32)>,
}

impl Default for ImageGeometry {
    fn default() -> Self {
        Self { dimensions: None, zoom: 1.0, pan: (0.0, 0.0), drag_last: None }
    }
}

impl ImageGeometry {
    pub fn new(dimensions: Option<(u32, u32)>) -> Self {
        Self { dimensions, ..Self::default() }
    }

    /// 换图（外部改动后重载）时重置缩放与平移——保留旧倍率去套一张尺寸完全
    /// 不同的新图只会得到一张位置莫名其妙的画面。
    pub fn reset(&mut self, dimensions: Option<(u32, u32)>) {
        *self = Self::new(dimensions);
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// 当前是否处于"放大到超出屏幕"的状态，即图片比视图大、可以平移。
    ///
    /// 退化区域（宽或高为 0）一律返回 `false`：那是"还没有布局信息"的状态
    /// （首帧渲染时 `image_area` 还是默认空矩形），此时说"可平移"是假的，
    /// 会让光标提前变成抓手。
    pub fn pannable(&self, area: Area) -> bool {
        if area.2 <= 0.0 || area.3 <= 0.0 {
            return false;
        }
        let (draw_w, draw_h) = self.draw_size(area);
        draw_w > area.2 + 0.5 || draw_h > area.3 + 0.5
    }

    /// 围绕 `anchor`（视图内的点）缩放 `steps` 档。返回是否真的变了。
    ///
    /// 指针下那个像素的归一化坐标 u/v 在缩放前后保持不变：先算出旧矩形里的
    /// u/v，缩放后反解出需要多大的 pan 才能让同一个 u/v 还落在指针下。
    pub fn zoom_by(&mut self, steps: f32, anchor: (f32, f32), area: Area) -> bool {
        if steps.abs() < f32::EPSILON || self.dimensions.is_none() {
            return false;
        }
        let old_zoom = self.zoom;
        let next_zoom = (old_zoom * ZOOM_PER_STEP.powf(steps)).clamp(MIN_ZOOM, MAX_ZOOM);
        if (next_zoom - old_zoom).abs() < f32::EPSILON {
            return false;
        }

        let old = self.target_rect(area);
        let u = ((anchor.0 - old.0) / old.2.max(1.0)).clamp(0.0, 1.0);
        let v = ((anchor.1 - old.1) / old.3.max(1.0)).clamp(0.0, 1.0);
        self.zoom = next_zoom;

        let (draw_w, draw_h) = self.draw_size(area);
        let centered_x = area.0 + (area.2 - draw_w) * 0.5;
        let centered_y = area.1 + (area.3 - draw_h) * 0.5;
        self.pan.0 = anchor.0 - u * draw_w - centered_x;
        self.pan.1 = anchor.1 - v * draw_h - centered_y;
        self.clamp_pan(area);
        true
    }

    /// 回到"适应视图"（zoom = 1、无平移）。
    pub fn reset_view(&mut self) -> bool {
        if (self.zoom - 1.0).abs() < f32::EPSILON && self.pan == (0.0, 0.0) {
            return false;
        }
        self.zoom = 1.0;
        self.pan = (0.0, 0.0);
        true
    }

    /// 按下左键：只有落在视图内才开始拖拽。
    pub fn begin_drag(&mut self, point: (f32, f32), area: Area) -> bool {
        if !contains(area, point) {
            return false;
        }
        self.drag_last = Some(point);
        true
    }

    pub fn dragging(&self) -> bool {
        self.drag_last.is_some()
    }

    /// 拖动到 `point`：按位移平移并钳到图片边缘。返回是否真的变了。
    ///
    /// 没按下过就什么都不做（`drag_last` 为 `None`）：不能顺手用 `point` 把
    /// 拖拽"带起来"——`drag_to` 只在拖拽进行中被调用，没有拖拽就说明这是一次
    /// 独立事件，替调用侧开启拖拽会让之后每一次鼠标移动都变成平移。
    pub fn drag_to(&mut self, point: (f32, f32), area: Area) -> bool {
        let Some(previous) = self.drag_last else {
            return false;
        };
        let before = self.pan;
        self.pan.0 += point.0 - previous.0;
        self.pan.1 += point.1 - previous.1;
        self.clamp_pan(area);
        self.drag_last = Some(point);
        self.pan != before
    }

    pub fn end_drag(&mut self) -> bool {
        self.drag_last.take().is_some()
    }

    /// 图片最终要画的矩形 `(x, y, w, h)`（zoom / pan / 钳制之后）。
    pub fn target_rect(&self, area: Area) -> Area {
        let (draw_w, draw_h) = self.draw_size(area);
        let max_pan_x = ((draw_w - area.2) * 0.5).max(0.0);
        let max_pan_y = ((draw_h - area.3) * 0.5).max(0.0);
        let pan_x = self.pan.0.clamp(-max_pan_x, max_pan_x);
        let pan_y = self.pan.1.clamp(-max_pan_y, max_pan_y);
        (
            area.0 + (area.2 - draw_w) * 0.5 + pan_x,
            area.1 + (area.3 - draw_h) * 0.5 + pan_y,
            draw_w,
            draw_h,
        )
    }

    /// 基准缩放：图片**完整装进视图**（contain），`zoom` 在它之上。
    ///
    /// 取两个方向比例的较小者——宽图受宽度约束、高图（9:16）受高度约束，于是
    /// 打开任何比例的图片都整张可见。这是与 Pebrel 原版（只铺满宽度）的唯一分歧，
    /// 也是"竖图打开就被裁掉底部"的修法。
    fn draw_size(&self, area: Area) -> (f32, f32) {
        let Some((width, height)) = self.dimensions else { return (area.2, area.3) };
        let base = (area.2.max(1.0) / width.max(1) as f32)
            .min(area.3.max(1.0) / height.max(1) as f32);
        (width as f32 * base * self.zoom, height as f32 * base * self.zoom)
    }

    fn clamp_pan(&mut self, area: Area) {
        let (draw_w, draw_h) = self.draw_size(area);
        let max_x = ((draw_w - area.2) * 0.5).max(0.0);
        let max_y = ((draw_h - area.3) * 0.5).max(0.0);
        self.pan.0 = self.pan.0.clamp(-max_x, max_x);
        self.pan.1 = self.pan.1.clamp(-max_y, max_y);
    }
}

fn contains(area: Area, point: (f32, f32)) -> bool {
    point.0 >= area.0 && point.0 < area.0 + area.2 && point.1 >= area.1 && point.1 < area.1 + area.3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry(dimensions: (u32, u32)) -> ImageGeometry {
        ImageGeometry::new(Some(dimensions))
    }

    /// 初始状态图片**完整装进视图**（contain）：宽图受宽约束、高图受高约束。
    /// 原来这里是"宽度铺满视图"（Pebrel 原版），竖图会被裁掉底部、必须手动缩小。
    #[test]
    fn initial_image_fits_the_viewport() {
        // 宽图：装宽度即装下整张（高度也够）。
        let image = geometry((1600, 900));
        let area = (20.0, 30.0, 800.0, 600.0);
        let target = image.target_rect(area);
        assert!((target.0 - area.0).abs() < 0.01);
        assert!((target.2 - area.2).abs() < 0.01, "宽图应当铺满宽度");
        assert!(target.3 <= area.3 + 0.01, "高度不能超出视图");

        // 竖图（9:16）：受高度约束，整张都在视图内、上下边贴齐。
        let tall = geometry((1080, 1920));
        let target = tall.target_rect(area);
        assert!(target.3 <= area.3 + 0.01, "竖图的高度不能超出视图");
        assert!(target.2 <= area.2 + 0.01, "竖图的宽度也不能超出视图");
        // 装高为准：高度铺满。
        assert!((target.3 - area.3).abs() < 0.01, "竖图应当铺满高度");
    }

    /// 缩放围绕指针：一旦图片在某轴上大于视图，该轴就能自由平移，指针下的像素在
    /// 缩放前后落在同一相对位置。
    ///
    /// 用 `400x400` 的正方形视图与 `1600x1600` 的方图：contain 基准下初始绘制
    /// 刚好等于视图边长（两轴都钳在 0），放大两档后两轴都超过视图、平移自由，
    /// 锚点不变量因此成立。图片**小于**视图的那个轴会被钳成居中（`clamp_pan`），
    /// 此时锚点会被居中语义覆盖——这是图片查看器的标准行为，见
    /// [`fit_clamped_view_is_centered`]。
    #[test]
    fn zoom_keeps_the_pointer_anchor_stable() {
        let mut image = geometry((1600, 1600));
        let area = (0.0, 0.0, 400.0, 400.0);
        let anchor = (100.0, 100.0);
        let before = image.target_rect(area);
        let before_u = (anchor.0 - before.0) / before.2;
        let before_v = (anchor.1 - before.1) / before.3;

        assert!(image.zoom_by(2.0, anchor, area));
        let after = image.target_rect(area);
        let after_u = (anchor.0 - after.0) / after.2;
        let after_v = (anchor.1 - after.1) / after.3;
        assert!((before_u - after_u).abs() < 0.001);
        assert!((before_v - after_v).abs() < 0.001);
    }

    /// contain 基准下，比视图窄的那个轴在缩放后若仍小于视图，就被钳成居中：
    /// 图片不会因为围绕偏离中心的指针缩放而被推出视图留下空白。
    #[test]
    fn fit_clamped_view_is_centered() {
        // 竖图放进宽视图：宽度远小于视图宽，围绕右侧指针放大一倍后仍不足视图宽，
        // 于是水平方向保持居中（x 落在正中）。
        let mut image = geometry((1080, 1920));
        let area = (0.0, 0.0, 1200.0, 600.0);
        let target = image.target_rect(area);
        assert!(
            (target.0 - (area.2 - target.2) * 0.5).abs() < 0.01,
            "小于视图宽的图片应当水平居中"
        );
        // 无论如何拖动，都不应产出留白（左缘不越过视图左缘）。
        assert!(image.begin_drag((600.0, 300.0), area));
        image.drag_to((2000.0, 300.0), area);
        let dragged = image.target_rect(area);
        assert!(dragged.0 >= area.0 - 0.01, "居中的图片不应被拖出左侧留白");
    }

    /// 拖拽被钳在图片边缘，不会拖出留白（Pebrel 同名测试）。
    #[test]
    fn dragging_is_clamped_to_the_image_edges() {
        let mut image = geometry((1600, 900));
        let area = (0.0, 0.0, 800.0, 400.0);
        assert!(image.zoom_by(3.0, (400.0, 200.0), area));
        assert!(image.begin_drag((400.0, 200.0), area));
        image.drag_to((4000.0, 200.0), area);
        let target = image.target_rect(area);
        assert!(target.0 <= area.0 + 0.01);
        assert!(target.0 + target.2 >= area.0 + area.2 - 0.01);
        assert!(image.end_drag());
    }

    /// 缩放倍率钳在 [MIN_ZOOM, MAX_ZOOM]，且到顶后再缩返回"没变化"。
    #[test]
    fn zoom_is_clamped_and_bounded_steps_report_no_change() {
        let mut image = geometry((800, 600));
        let area = (0.0, 0.0, 400.0, 300.0);
        // 连续放大到上限。
        for _ in 0..200 {
            image.zoom_by(1.0, (200.0, 150.0), area);
        }
        assert!((image.zoom() - MAX_ZOOM).abs() < 0.001);
        assert!(!image.zoom_by(1.0, (200.0, 150.0), area), "已到上限，再放大应无变化");
        // 连续缩小到下限。
        for _ in 0..400 {
            image.zoom_by(-1.0, (200.0, 150.0), area);
        }
        assert!((image.zoom() - MIN_ZOOM).abs() < 0.001);
        assert!(!image.zoom_by(-1.0, (200.0, 150.0), area), "已到下限，再缩小应无变化");
    }

    /// 尺寸未知时缩放/平移一律无动作：还没解码出尺寸就没有基准比例可用。
    #[test]
    fn unknown_dimensions_make_zoom_a_no_op() {
        let mut image = ImageGeometry::new(None);
        let area = (0.0, 0.0, 400.0, 300.0);
        assert!(!image.zoom_by(1.0, (200.0, 150.0), area));
        assert!(!image.pannable(area));
    }

    /// 退化（零宽/零高）区域不算"可平移"：那是首帧还没拿到布局信息的状态，
    /// 此时说是可拖会让光标提前变成抓手。
    #[test]
    fn degenerate_area_is_never_pannable() {
        let mut image = geometry((1600, 900));
        assert!(!image.pannable((0.0, 0.0, 0.0, 0.0)), "零矩形不算可平移");
        assert!(!image.pannable((0.0, 0.0, 400.0, 0.0)), "零高不算可平移");
        // 放大到确实超出视图后，同一张图变为可平移。
        assert!(image.zoom_by(3.0, (200.0, 150.0), (0.0, 0.0, 400.0, 300.0)));
        assert!(image.pannable((0.0, 0.0, 400.0, 300.0)));
    }

    /// 未开始拖拽时 `drag_to` 无动作；视图外的按下不启动拖拽。
    #[test]
    fn drag_requires_a_press_inside_the_area() {
        let mut image = geometry((1600, 900));
        let area = (0.0, 0.0, 400.0, 300.0);
        assert!(!image.drag_to((10.0, 10.0), area), "没按下就不该平移");
        assert!(!image.begin_drag((999.0, 999.0), area), "视图外的按下不开始拖拽");
        assert!(!image.dragging());
    }

    /// `reset_view` 回到适应视图；已经在适应视图时不报告变化。
    #[test]
    fn reset_view_returns_to_fit() {
        let mut image = geometry((1600, 900));
        let area = (0.0, 0.0, 400.0, 300.0);
        assert!(image.zoom_by(3.0, (200.0, 150.0), area));
        assert!(image.reset_view());
        assert!((image.zoom() - 1.0).abs() < f32::EPSILON);
        assert!(!image.reset_view(), "已在适应视图，不应报告变化");
    }

    /// 只有放大到超出视图才可平移。
    #[test]
    fn pannable_only_when_zoomed_past_the_viewport() {
        let mut image = geometry((800, 400));
        let area = (0.0, 0.0, 400.0, 400.0);
        assert!(!image.pannable(area), "适应宽度且图更矮，无需平移");
        assert!(image.zoom_by(3.0, (200.0, 200.0), area));
        assert!(image.pannable(area));
    }
}
