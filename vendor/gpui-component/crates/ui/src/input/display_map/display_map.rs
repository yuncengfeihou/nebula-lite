/// DisplayMap: Public facade for Editor/Input display mapping.
///
/// This combines WrapMap and FoldMap to provide a unified API:
/// - BufferPoint ↔ DisplayPoint conversion
/// - Fold management (candidates, toggle, query)
/// - Automatic projection updates on text/layout changes
use std::ops::Range;

use gpui::{App, Font, Pixels};
use ropey::Rope;

use super::fold_map::FoldMap;
use super::folding::FoldRange;
pub use super::text_wrapper::WrappingIndent;
use super::text_wrapper::{LineItem, WrapDisplayPoint};
use super::wrap_map::WrapMap;
use super::{BufferPoint, DisplayPoint};
use crate::input::Point as TreeSitterPoint;
use crate::input::display_map::WrapPoint;
use crate::input::rope_ext::RopeExt as _;

/// DisplayMap is the main interface for Editor/Input coordinate mapping.
///
/// It manages the two-layer projection:
/// 1. Buffer → Wrap (soft-wrapping)
/// 2. Wrap → Display (folding)
///
/// Editor/Input only needs to work with BufferPoint and DisplayPoint.
pub struct DisplayMap {
    wrap_map: WrapMap,
    fold_map: FoldMap,
    /// 大文档上**推迟生效**的新折行宽度；见 [`DisplayMap::on_layout_changed`]。
    pending_wrap_width: Option<Pixels>,
    /// 每次登记新的待生效宽度就加一。前台防抖任务按它判断"自己是不是最后那一次"。
    pending_wrap_generation: u64,
}

/// 超过这个体积的文档，折行宽度变化（拖动窗口 / 侧栏、改窗口大小）推迟到
/// 停手之后合并做一次。
///
/// 整篇重排是 O(全文) 的塑形：4.8 MB 的 .txt 一次约 200 ms（见本文件
/// `TextWrapper::update` 上方的说明）。拖动时每一帧都变宽度，同步重排就等于
/// 每一帧冻 200 ms；推迟之后拖动期间沿用旧折行（文字暂时按旧宽度断行，右缘可能
/// 被裁掉），停手 150 ms 后一次性重排。256 KiB 以下的文档重排只要几毫秒，
/// 仍然即时生效，观感不变。
const DEFERRED_WRAP_MIN_BYTES: usize = 256 * 1024;

impl DisplayMap {
    pub fn new(font: Font, font_size: Pixels, wrap_width: Option<Pixels>) -> Self {
        Self {
            wrap_map: WrapMap::new(font, font_size, wrap_width),
            fold_map: FoldMap::new(),
            pending_wrap_width: None,
            pending_wrap_generation: 0,
        }
    }

    // ==================== Core Coordinate Mapping ====================

    /// Convert buffer position to display position
    pub fn buffer_pos_to_display_pos(&self, pos: BufferPoint) -> DisplayPoint {
        // Buffer → Wrap
        let wrap_pos = self.wrap_map.buffer_pos_to_wrap_pos(pos);

        // Wrap → Display
        if let Some(display_row) = self.fold_map.wrap_row_to_display_row(wrap_pos.row) {
            DisplayPoint::new(display_row, wrap_pos.col)
        } else {
            // Cursor is in a folded region, find nearest visible row
            let display_row = self.fold_map.nearest_visible_display_row(wrap_pos.row);
            DisplayPoint::new(display_row, 0) // Column 0 at fold boundary
        }
    }

    /// Convert display position to buffer position
    pub fn display_pos_to_buffer_pos(&self, pos: DisplayPoint) -> BufferPoint {
        // Display → Wrap
        let wrap_row = self.fold_map.display_row_to_wrap_row(pos.row).unwrap_or(0);

        // Wrap → Buffer
        let wrap_pos = WrapPoint::new(wrap_row, pos.col);
        self.wrap_map.wrap_pos_to_buffer_pos(wrap_pos)
    }

    /// Get total number of visible display rows
    #[inline]
    pub fn display_row_count(&self) -> usize {
        self.fold_map.display_row_count()
    }

    /// Get the buffer line for a given display row
    pub fn display_row_to_buffer_line(&self, display_row: usize) -> usize {
        // Display → Wrap
        let wrap_row = self
            .fold_map
            .display_row_to_wrap_row(display_row)
            .unwrap_or(0);

        // Wrap → Buffer line
        self.wrap_map.wrap_row_to_buffer_line(wrap_row)
    }

    /// Get the display row range for a buffer line: [start, end)
    /// Returns None if the buffer line is completely hidden
    pub fn buffer_line_to_display_row_range(&self, line: usize) -> Option<Range<usize>> {
        // Buffer line → Wrap row range
        let wrap_row_range = self.wrap_map.buffer_line_to_wrap_row_range(line);

        // Find first and last visible display rows in this range
        let mut first_display_row = None;
        let mut last_display_row = None;

        for wrap_row in wrap_row_range {
            if let Some(display_row) = self.fold_map.wrap_row_to_display_row(wrap_row) {
                if first_display_row.is_none() {
                    first_display_row = Some(display_row);
                }
                last_display_row = Some(display_row);
            }
        }

        if let (Some(start), Some(end)) = (first_display_row, last_display_row) {
            Some(start..end + 1)
        } else {
            None // Completely folded
        }
    }

    /// Check if a buffer line is completely hidden
    #[inline]
    pub fn is_buffer_line_hidden(&self, line: usize) -> bool {
        self.buffer_line_to_display_row_range(line).is_none()
    }

    /// First display row of a buffer line. If the line is fully folded, returns the
    /// nearest visible display row.
    pub fn buffer_line_to_display_row(&self, line: usize) -> usize {
        match self.buffer_line_to_display_row_range(line) {
            Some(range) => range.start,
            None => {
                let wrap_row = self.wrap_map.buffer_line_to_first_wrap_row(line);
                self.fold_map.nearest_visible_display_row(wrap_row)
            }
        }
    }

    /// Set fold candidates (from tree-sitter/LSP)
    pub fn set_fold_candidates(&mut self, candidates: Vec<FoldRange>) {
        self.fold_map.set_candidates(candidates);
        self.rebuild_fold_projection();
    }

    /// Set a fold at the given start_line (must be in candidates)
    pub fn set_folded(&mut self, start_line: usize, folded: bool) {
        self.fold_map.set_folded(start_line, folded);
        self.rebuild_fold_projection();
    }

    /// Toggle fold at the given start_line
    pub fn toggle_fold(&mut self, start_line: usize) {
        self.fold_map.toggle_fold(start_line);
        self.rebuild_fold_projection();
    }

    /// Check if a line is currently folded
    #[inline]
    pub fn is_folded_at(&self, start_line: usize) -> bool {
        self.fold_map.is_folded_at(start_line)
    }

    /// Check if a line is a fold candidate
    #[inline]
    pub fn is_fold_candidate(&self, start_line: usize) -> bool {
        self.fold_map.is_fold_candidate(start_line)
    }

    /// Get all currently folded ranges
    #[inline]
    pub fn folded_ranges(&self) -> &[FoldRange] {
        self.fold_map.folded_ranges()
    }

    /// Clear all folds
    pub fn clear_folds(&mut self) {
        self.fold_map.clear_folds();
        self.rebuild_fold_projection();
    }

    // ==================== Text and Layout Updates ====================

    /// Adjust folds and candidates for a text edit before updating the wrap map.
    ///
    /// Must be called with the OLD text (before replacement) and the edit range/new_text
    /// so we can compute which old lines were affected.
    pub fn adjust_folds_for_edit(&mut self, old_text: &Rope, range: &Range<usize>, new_text: &str) {
        if self.fold_map.folded_ranges().is_empty() && self.fold_map.fold_candidates().is_empty() {
            return;
        }

        let edit_start_line = old_text.offset_to_point(range.start).row;
        let edit_end_line = old_text.offset_to_point(range.end.min(old_text.len())).row;

        let old_lines_in_range = edit_end_line.saturating_sub(edit_start_line);
        let new_lines_in_range = new_text.chars().filter(|c| *c == '\n').count();
        let line_delta = new_lines_in_range as isize - old_lines_in_range as isize;

        self.fold_map
            .adjust_folds_for_edit(edit_start_line, edit_end_line, line_delta);
    }

    /// Incrementally update fold candidates after a text edit.
    ///
    /// Extracts new fold candidates only within the edited byte range
    /// and merges them with existing (already adjusted) candidates.
    pub fn update_fold_candidates_for_edit(
        &mut self,
        tree: &super::folding::Tree,
        edit_byte_range: Range<usize>,
        new_text: &Rope,
    ) {
        let new_start_line = new_text.offset_to_point(edit_byte_range.start).row;
        let new_end_line = new_text
            .offset_to_point(edit_byte_range.end.min(new_text.len()))
            .row;

        let new_candidates = super::folding::extract_fold_ranges_in_range(tree, edit_byte_range);
        self.fold_map
            .merge_candidates_for_edit(new_start_line, new_end_line, new_candidates);
    }

    /// Update text (incremental or full)
    pub fn on_text_changed(
        &mut self,
        changed_text: &Rope,
        range: &Range<usize>,
        new_text: &Rope,
        cx: &mut App,
    ) {
        self.wrap_map
            .on_text_changed(changed_text, range, new_text, cx);
        self.rebuild_fold_projection();
    }

    /// Update layout parameters (wrap width or font)
    pub fn on_layout_changed(&mut self, wrap_width: Option<Pixels>, cx: &mut App) {
        let applied = self.wrap_map.applied_wrap_width();
        if applied == wrap_width {
            self.pending_wrap_width = None;
            return;
        }

        // 大文档 + 折行开着（已生效宽度与目标宽度都是 `Some`）：宽度变化多半来自
        // 拖动窗口 / 侧栏，每帧重排会持续冻结界面。先记下目标宽度，由前台防抖
        // 任务在停手后调用 [`DisplayMap::apply_pending_wrap_width`] 合并做一次。
        // 首次折行（`applied == None`）与关掉折行（目标 `None`）都不走这条路：
        // 前者没有旧折行可沿用，后者本身很便宜。
        if self.wrap_map.text().len() >= DEFERRED_WRAP_MIN_BYTES
            && applied.is_some()
            && wrap_width.is_some()
        {
            if self.pending_wrap_width != Some(wrap_width.unwrap()) {
                self.pending_wrap_width = wrap_width;
                self.pending_wrap_generation = self.pending_wrap_generation.wrapping_add(1);
            }
            return;
        }

        self.pending_wrap_width = None;
        self.wrap_map.on_layout_changed(wrap_width, cx);
        self.rebuild_fold_projection();
    }

    /// 当前**已经生效**的折行宽度（推迟期间仍是旧值，渲染要按它排版）。
    #[inline]
    pub fn applied_wrap_width(&self) -> Option<Pixels> {
        self.wrap_map.applied_wrap_width()
    }

    /// 是否有一次推迟中的宽度变更；返回的代数供防抖任务判重。
    #[inline]
    pub fn pending_wrap_generation(&self) -> Option<u64> {
        self.pending_wrap_width
            .map(|_| self.pending_wrap_generation)
    }

    /// 把推迟中的宽度真正应用（整篇重排）。返回是否真的做了重排。
    pub fn apply_pending_wrap_width(&mut self, cx: &mut App) -> bool {
        let Some(wrap_width) = self.pending_wrap_width.take() else {
            return false;
        };
        if self.wrap_map.applied_wrap_width() == Some(wrap_width) {
            return false;
        }
        self.wrap_map.on_layout_changed(Some(wrap_width), cx);
        self.rebuild_fold_projection();
        true
    }

    /// Set the wrapping indent for continuation lines.
    pub fn set_wrapping_indent(&mut self, wrapping_indent: WrappingIndent, cx: &mut App) {
        self.wrap_map.set_wrapping_indent(wrapping_indent, cx);
        self.rebuild_fold_projection();
    }

    /// Set font parameters
    pub fn set_font(&mut self, font: Font, font_size: Pixels, cx: &mut App) {
        self.wrap_map.set_font(font, font_size, cx);
        self.rebuild_fold_projection();
    }

    /// Ensure text is prepared (initializes wrapper if needed)
    pub fn ensure_text_prepared(&mut self, text: &Rope, cx: &mut App) {
        let did_initialize = self.wrap_map.ensure_text_prepared(text, cx);
        if did_initialize {
            self.rebuild_fold_projection();
        }
    }

    /// Initialize with text
    pub fn set_text(&mut self, text: &Rope, cx: &mut App) {
        self.wrap_map.set_text(text, cx);
        self.rebuild_fold_projection();
    }

    // ==================== Internal Helpers ====================

    /// Rebuild fold projection after wrap_map or fold state changes
    /// Only rebuilds if there are actually folded ranges
    fn rebuild_fold_projection(&mut self) {
        if !self.fold_map.folded_ranges().is_empty() {
            self.fold_map.rebuild(&self.wrap_map);
        } else {
            // No active folds: identity mapping (wrap_row == display_row).
            // Just update cached count so query methods work without Vec allocation.
            self.fold_map
                .mark_dirty_with_wrap_count(self.wrap_map.wrap_row_count());
        }
    }

    // ==================== Wrap Display Point Operations ====================

    /// Convert byte offset to wrap display point (with soft wrap info).
    #[inline]
    pub(crate) fn offset_to_wrap_display_point(&self, offset: usize) -> WrapDisplayPoint {
        self.wrap_map.wrapper().offset_to_display_point(offset)
    }

    /// Convert wrap display point to byte offset.
    #[inline]
    pub(crate) fn wrap_display_point_to_offset(&self, point: WrapDisplayPoint) -> usize {
        self.wrap_map.wrapper().display_point_to_offset(point)
    }

    /// Convert wrap display point to TreeSitterPoint (buffer line/col).
    #[inline]
    pub(crate) fn wrap_display_point_to_point(&self, point: WrapDisplayPoint) -> TreeSitterPoint {
        self.wrap_map.wrapper().display_point_to_point(point)
    }

    /// Convert a wrap row to a display row (skipping folded rows).
    /// Returns None if the wrap row is folded.
    #[inline]
    pub fn wrap_row_to_display_row(&self, wrap_row: usize) -> Option<usize> {
        self.fold_map.wrap_row_to_display_row(wrap_row)
    }

    /// Find the nearest visible display row for a given wrap row.
    #[inline]
    pub fn nearest_visible_display_row(&self, wrap_row: usize) -> usize {
        self.fold_map.nearest_visible_display_row(wrap_row)
    }

    /// Convert a display row to a wrap row.
    #[inline]
    pub fn display_row_to_wrap_row(&self, display_row: usize) -> Option<usize> {
        self.fold_map.display_row_to_wrap_row(display_row)
    }

    /// Get the longest row index (by byte length).
    #[inline]
    pub(crate) fn longest_row(&self) -> usize {
        self.wrap_map.wrapper().longest_row()
    }

    // ==================== Access Methods ====================

    /// Get the line item by buffer row index.
    #[inline]
    pub(crate) fn line(&self, row: usize) -> Option<&LineItem> {
        self.wrap_map.line(row)
    }

    /// Get the rope text
    #[inline]
    pub fn text(&self) -> &Rope {
        self.wrap_map.text()
    }

    /// Calculate how many wrap rows of a buffer line are visible (not folded)
    #[inline]
    pub fn visible_wrap_row_count_for_buffer_line(&self, line: usize) -> usize {
        self.wrap_map
            .visible_wrap_row_count_for_line(line, &self.fold_map)
    }

    /// Get the wrap row count (before folding)
    #[inline]
    pub fn wrap_row_count(&self) -> usize {
        self.wrap_map.wrap_row_count()
    }

    /// Get the buffer line count (logical lines)
    #[inline]
    pub fn buffer_line_count(&self) -> usize {
        self.wrap_map.buffer_line_count()
    }
}
