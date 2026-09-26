//! 按住右键拖动缩放模型窗口的便携部分。
//!
//! 旧版（`pre-refactor` 分支的 `src/pages/main/index.vue`）在模型窗口上实现了
//! 同一个交互：指针移动时把 `(movementX + movementY) * 0.5` 加到窗口的缩放
//! 百分比上，再把窗口尺寸设为 `模型尺寸 * 缩放`。这个模块保留同一套位移到缩放的
//! 映射，只把结果收敛到 `next` 的配置契约：范围 `25–400%`，且不再要求按住 Shift。
//!
//! 交互与「右键单击弹菜单」共用同一个按键，因此一次拖动只有在指针移动超过
//! [`RESIZE_DRAG_THRESHOLD`] 之后才算缩放；没有越过阈值就松开的右键仍然是菜单。
//! 平台适配层负责自己那份无法便携的部分：读取指针位置、改原生窗口尺寸、以及在
//! 拖动结束时把最终缩放报回配置。
use crate::cover_window_dimension;

/// 指针位移超过它才算缩放，而不是一次右键单击。
///
/// 阈值按欧氏距离算，并且始终相对按下点测量，因此一次缓慢拖动不会因为单帧位移
/// 很小而被误判成单击，一次手抖也不会因为累计位移而被误判成拖动。
pub(crate) const RESIZE_DRAG_THRESHOLD: f64 = 3.0;

/// 每个指针像素对应的缩放百分比。
///
/// 沿用旧版 `(movementX + movementY) * 0.5` 的系数：向右下拖动放大，向左上
/// 拖动缩小，向右上或左下拖动时两个分量相互抵消。
pub(crate) const RESIZE_DRAG_PERCENT_PER_PIXEL: f64 = 0.5;

/// 拖动可以到达的缩放范围。
///
/// 与配置契约（`OverlayConfig::scale_percent`、`OverlaySettings::is_valid`）
/// 一致，因此拖动产生的结果总能被配置接受，设置页的滑块也总有一个可显示的值。
pub(crate) const MINIMUM_RESIZE_DRAG_SCALE_PERCENT: u16 = 25;
pub(crate) const MAXIMUM_RESIZE_DRAG_SCALE_PERCENT: u16 = 400;

/// `100%` 缩放时窗口的基准尺寸，与窗口 bounds 使用同一坐标单位。
///
/// Windows 传 DPI 换算后的物理像素，macOS 传点，因此拖动状态机本身不需要知道
/// DPI，只需要保证 `base` 与它输出的尺寸同单位。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResizeBase {
    width: f64,
    height: f64,
}

impl ResizeBase {
    /// 只在两个维度都是有限正数时返回一个基准尺寸。
    pub(crate) fn new(width: f64, height: f64) -> Option<Self> {
        (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
            .then_some(Self { width, height })
    }

    /// 这个基准在某个缩放百分比下对应的窗口尺寸。
    pub(crate) fn dimensions(self, scale_percent: u16) -> (u32, u32) {
        let factor = f64::from(scale_percent) / 100.0;
        (
            cover_window_dimension(self.width * factor),
            cover_window_dimension(self.height * factor),
        )
    }

    /// 一个窗口宽度对应的缩放百分比。
    ///
    /// 拖动从窗口**实际**尺寸开始，而不是从配置里的值开始：窗口几何与
    /// `overlay.scale_percent` 并不总是同步（用户拖动过一次、显示器 DPI 变了、
    /// 或者手工编辑过配置），用配置值当起点会让第一次指针移动就把窗口跳到另一个
    /// 尺寸。反算出来的值同样收敛到配置契约，因此拖动结束时写回的是一个合法值。
    pub(crate) fn scale_percent_for_width(self, width: u32) -> u16 {
        let requested = f64::from(width) / self.width * 100.0;
        requested.round().clamp(
            f64::from(MINIMUM_RESIZE_DRAG_SCALE_PERCENT),
            f64::from(MAXIMUM_RESIZE_DRAG_SCALE_PERCENT),
        ) as u16
    }
}

/// 一次拖动应该应用的窗口尺寸。
///
/// 窗口原点不在这里：拖动缩放沿用旧版 `setSize` 的行为，左上角保持不动。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ResizeOutcome {
    pub(crate) scale_percent: u16,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// 一次进行中的右键拖动。
#[derive(Clone, Copy, Debug)]
pub(crate) struct ResizeDrag {
    origin: (f64, f64),
    base: ResizeBase,
    start_scale: u16,
    scale: u16,
    dragging: bool,
}

impl ResizeDrag {
    /// 记录一次右键按下。`pointer` 与 [`Self::observe`] 使用同一坐标空间。
    pub(crate) fn begin(pointer: (f64, f64), base: ResizeBase, scale_percent: u16) -> Self {
        Self {
            origin: pointer,
            base,
            start_scale: scale_percent,
            scale: scale_percent,
            dragging: false,
        }
    }

    /// 喂入一次指针位置，返回需要应用的窗口尺寸。
    ///
    /// 指针还没越过阈值、或者缩放百分比没变（尺寸因而也没变）时返回 `None`，
    /// 调用方因此不会为一次纯粹的抖动去改原生窗口。
    pub(crate) fn observe(&mut self, pointer: (f64, f64)) -> Option<ResizeOutcome> {
        let dx = pointer.0 - self.origin.0;
        let dy = pointer.1 - self.origin.1;
        if !dx.is_finite() || !dy.is_finite() {
            return None;
        }
        if !self.dragging && (dx * dx + dy * dy).sqrt() <= RESIZE_DRAG_THRESHOLD {
            return None;
        }
        self.dragging = true;
        let requested = f64::from(self.start_scale) + (dx + dy) * RESIZE_DRAG_PERCENT_PER_PIXEL;
        let scale = requested.round().clamp(
            f64::from(MINIMUM_RESIZE_DRAG_SCALE_PERCENT),
            f64::from(MAXIMUM_RESIZE_DRAG_SCALE_PERCENT),
        ) as u16;
        if scale == self.scale {
            return None;
        }
        self.scale = scale;
        let (width, height) = self.dimensions();
        Some(ResizeOutcome {
            scale_percent: scale,
            width,
            height,
        })
    }

    /// 这次拖动是否已经越过阈值。
    ///
    /// 平台层用它决定松手时该弹菜单还是该结束缩放：越过阈值的右键即使最终没有
    /// 改变缩放，也不再是一次菜单点击。
    pub(crate) const fn dragging(&self) -> bool {
        self.dragging
    }

    /// 结束拖动，返回需要写回配置的缩放百分比。
    ///
    /// 没有越过阈值、或者越过阈值但缩放没有实际变化时返回 `None`：前者是菜单
    /// 点击，后者没有需要持久化的新值。
    pub(crate) const fn finish(self) -> Option<u16> {
        if self.scale == self.start_scale {
            None
        } else {
            Some(self.scale)
        }
    }

    fn dimensions(&self) -> (u32, u32) {
        self.base.dimensions(self.scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ResizeBase {
        ResizeBase::new(350.0, 350.0).expect("square base")
    }

    fn begin() -> ResizeDrag {
        ResizeDrag::begin((500.0, 400.0), base(), 100)
    }

    #[test]
    fn a_window_width_maps_back_to_the_scale_it_was_drawn_at() {
        let base = base();
        assert_eq!(base.scale_percent_for_width(350), 100);
        assert_eq!(base.scale_percent_for_width(700), 200);
        assert_eq!(base.scale_percent_for_width(175), 50);

        // Rounding is what keeps a box restored from state (which stores whole
        // pixels) from drifting by one percent on every drag.
        assert_eq!(base.scale_percent_for_width(357), 102);

        // A box that is not on the contract is still reported on it, so the
        // value a drag ends up writing back is always acceptable.
        assert_eq!(
            base.scale_percent_for_width(1),
            MINIMUM_RESIZE_DRAG_SCALE_PERCENT
        );
        assert_eq!(
            base.scale_percent_for_width(100_000),
            MAXIMUM_RESIZE_DRAG_SCALE_PERCENT
        );
    }

    #[test]
    fn a_drag_started_from_a_drifted_window_does_not_jump() {
        // The window was restored at 420px wide while the configuration still
        // says 100% (350px). A drag must start from 420, not from 350.
        let mut drag = ResizeDrag::begin((0.0, 0.0), base(), base().scale_percent_for_width(420));
        assert_eq!(drag.observe((0.0, 0.0)), None, "不动就不产生新尺寸");
        assert_eq!(drag.finish(), None);

        // The first pointer move changes the scale relative to the real box.
        let mut drag = ResizeDrag::begin((0.0, 0.0), base(), base().scale_percent_for_width(420));
        let outcome = drag.observe((20.0, 20.0)).expect("dragged");
        assert_eq!(outcome.scale_percent, 140);
        assert_eq!(outcome.width, 490);
    }

    #[test]
    fn a_base_needs_two_positive_finite_dimensions() {
        assert!(ResizeBase::new(350.0, 200.0).is_some());
        assert!(ResizeBase::new(0.0, 200.0).is_none());
        assert!(ResizeBase::new(350.0, -1.0).is_none());
        assert!(ResizeBase::new(f64::NAN, 200.0).is_none());
        assert!(ResizeBase::new(350.0, f64::INFINITY).is_none());
    }

    #[test]
    fn a_right_click_that_does_not_move_never_resizes() {
        let mut drag = begin();
        assert_eq!(drag.observe((500.0, 400.0)), None);
        assert_eq!(drag.observe((501.0, 401.0)), None);
        assert!(!drag.dragging());
        assert_eq!(drag.finish(), None);
    }

    #[test]
    fn the_threshold_is_measured_from_the_press_point() {
        let mut drag = begin();
        // 单帧位移很小，但累计位移越过了阈值，因此仍然是一次拖动。
        assert_eq!(drag.observe((502.0, 400.0)), None);
        assert!(!drag.dragging());
        let outcome = drag.observe((504.0, 400.0)).expect("past the threshold");
        assert!(drag.dragging());
        assert_eq!(outcome.scale_percent, 102);
        assert_eq!(outcome.width, 357);
        assert_eq!(outcome.height, 357);
    }

    #[test]
    fn dragging_down_right_grows_and_up_left_shrinks() {
        let mut grow = begin();
        let grown = grow.observe((600.0, 500.0)).expect("dragged");
        assert_eq!(grown.scale_percent, 200);
        assert_eq!((grown.width, grown.height), (700, 700));
        assert_eq!(grow.finish(), Some(200));

        let mut shrink = begin();
        let shrunk = shrink.observe((450.0, 350.0)).expect("dragged");
        assert_eq!(shrunk.scale_percent, 50);
        assert_eq!((shrunk.width, shrunk.height), (175, 175));
        assert_eq!(shrink.finish(), Some(50));

        let mut floor = begin();
        let floored = floor.observe((300.0, 300.0)).expect("dragged");
        assert_eq!(floored.scale_percent, MINIMUM_RESIZE_DRAG_SCALE_PERCENT);
        assert_eq!((floored.width, floored.height), (88, 88));
    }

    #[test]
    fn the_scale_is_clamped_to_the_configuration_contract() {
        let mut huge = begin();
        let outcome = huge.observe((5_000.0, 5_000.0)).expect("dragged");
        assert_eq!(outcome.scale_percent, MAXIMUM_RESIZE_DRAG_SCALE_PERCENT);
        assert_eq!(huge.finish(), Some(MAXIMUM_RESIZE_DRAG_SCALE_PERCENT));

        let mut tiny = begin();
        let outcome = tiny.observe((-5_000.0, -5_000.0)).expect("dragged");
        assert_eq!(outcome.scale_percent, MINIMUM_RESIZE_DRAG_SCALE_PERCENT);
        assert_eq!(tiny.finish(), Some(MINIMUM_RESIZE_DRAG_SCALE_PERCENT));
    }

    #[test]
    fn a_scale_that_does_not_change_reports_nothing() {
        let mut drag = begin();
        // 越过阈值，但两个方向的位移相互抵消，缩放没有变化。
        assert_eq!(drag.observe((504.0, 396.0)), None);
        assert!(drag.dragging(), "越过阈值的右键不再是菜单点击");
        assert_eq!(drag.finish(), None, "没有新值需要写回配置");
    }

    #[test]
    fn repeated_observations_at_one_position_apply_once() {
        let mut drag = begin();
        let first = drag.observe((600.0, 500.0)).expect("dragged");
        assert_eq!(drag.observe((600.0, 500.0)), None);
        assert_eq!(drag.finish(), Some(first.scale_percent));
    }

    #[test]
    fn diagonal_offsets_that_cancel_leave_the_scale_alone() {
        let mut drag = begin();
        assert_eq!(drag.observe((600.0, 300.0)), None);
        assert!(drag.dragging());
        assert_eq!(drag.finish(), None);
    }

    #[test]
    fn a_non_finite_pointer_position_is_ignored() {
        let mut drag = begin();
        assert_eq!(drag.observe((f64::NAN, 400.0)), None);
        assert_eq!(drag.observe((600.0, f64::INFINITY)), None);
        assert!(!drag.dragging());
    }

    #[test]
    fn an_extreme_aspect_ratio_still_produces_a_configurable_window() {
        let wide = ResizeBase::new(350.0, 64.0).expect("wide base");
        let mut drag = ResizeDrag::begin((0.0, 0.0), wide, 100);
        let outcome = drag.observe((-1_000.0, -1_000.0)).expect("dragged");
        assert_eq!(outcome.scale_percent, MINIMUM_RESIZE_DRAG_SCALE_PERCENT);
        assert_eq!(outcome.width, 88);
        assert_eq!(outcome.height, 64, "尺寸下限优先于算出来的 16 像素");
        assert!(
            crate::OverlayWindowBounds::new(0, 0, outcome.width, outcome.height)
                .validate()
                .is_ok()
        );
    }
}
