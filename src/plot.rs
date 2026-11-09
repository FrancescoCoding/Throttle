//! Rate plots: the shared plot builder, line style, axis grid and navigation.

use eframe::egui;
use egui_plot::{Legend, Line, Plot, PlotPoints};

use crate::settings::View;
use crate::units::{GB, KB, MB, TB, axis_rate};

/// Fill opacity under the plot lines when "Filled area" is on.
const PLOT_FILL_ALPHA: f32 = 0.15;

/// The plot both rate graphs share: no x axis labels, binary rate grid and
/// labels on y, time axis zoom and pan only (the rate axis keeps fitting
/// the data), a legend, and zero always at the bottom.
pub fn rate_plot(id: &'static str) -> Plot<'static> {
    // An explicit id, so `reset_view` can find the plot's memory.
    Plot::new(id)
        .id(egui::Id::new(id))
        .show_x(false)
        .y_axis_formatter(rate_axis_label)
        .y_grid_spacer(rate_grid)
        .allow_scroll(false)
        .allow_zoom([true, false])
        .allow_boxed_zoom(false)
        .allow_drag([true, false])
        .legend(Legend::default())
        .include_y(0.0)
        .include_y(1.0)
}

/// Put the plot built by `rate_plot(id)` back on auto bounds, as a double
/// click on it would. Used by the plots' context menus.
pub fn reset_view(ctx: &egui::Context, id: &str) {
    let id = egui::Id::new(id);
    if let Some(mut memory) = egui_plot::PlotMemory::load(ctx, id) {
        memory.auto_bounds = egui::Vec2b::TRUE;
        memory.store(ctx, id);
    }
}

/// A plot line in the style the View menu asks for. Samples are one second
/// apart, plotted against their index from zero.
pub fn plot_line(view: &View, name: &str, ys: &[f64], colour: egui::Color32) -> Line<'static> {
    let mut line = Line::new(name, PlotPoints::from_ys_f64(ys)).color(colour);
    // The default width keeps egui_plot's own stroke (1.5), so the default
    // plot looks as before; only a thicker choice sets the width.
    if view.plot_line_width > View::default().plot_line_width {
        line = line.width(view.plot_line_width);
    }
    if view.plot_fill {
        line = line.fill(0.0).fill_alpha(PLOT_FILL_ALPHA);
    }
    line
}

/// Grid steps for a rate axis: 1, 2 and 5 of a binary unit (bytes, KB, MB,
/// GB, TB) and their tens and hundreds, so the axis reads "512 KB/s, 1 MB/s,
/// 2 MB/s" rather than the powers of ten egui_plot would pick ("976.6 KB/s"),
/// and a labelled step always fits the plot height: with powers of ten a
/// short plot whose range passes 3 MB/s lost every label but zero. Halves of
/// the step are minor lines, labelled too when there is room.
fn rate_grid(input: egui_plot::GridInput) -> Vec<egui_plot::GridMark> {
    let (min, max) = input.bounds;
    // egui_plot labels a step once it spans about 20 points and its base
    // step is 8 points, so three base steps leave room for a label.
    let wanted = input.base_step_size * 3.0;
    if !(wanted.is_finite() && wanted > 0.0 && max > min) {
        return Vec::new();
    }
    let major = nice_rate_step(wanted);
    let minor = major / 2.0;
    let first = (min / minor).floor() as i64;
    let last = (max / minor).ceil() as i64;
    if last - first > 10_000 {
        return Vec::new();
    }
    // A label is centred on its mark, so one within half a text height of
    // the top edge would spill out of the plot over whatever sits above it.
    // The base step spans 8 points, which gives the distance in points.
    let points_from_top = |value: f64| (max - value) / input.base_step_size * 8.0;
    (first..=last)
        .map(|i| egui_plot::GridMark {
            value: i as f64 * minor,
            step_size: if i % 2 == 0 { major } else { minor },
        })
        .filter(|mark| points_from_top(mark.value) >= 8.0)
        .collect()
}

/// The smallest of 1, 2, 5, 10, 20, 50, 100, 200 or 500 bytes, KB, MB, GB
/// or TB per second that is at least `wanted` bytes per second.
fn nice_rate_step(wanted: f64) -> f64 {
    for unit in [1.0, KB, MB, GB, TB] {
        for scale in [1.0, 10.0, 100.0] {
            for m in [1.0, 2.0, 5.0] {
                let step = m * scale * unit;
                if step >= wanted {
                    return step;
                }
            }
        }
    }
    wanted
}

/// Y axis formatter for the rate plots.
fn rate_axis_label(mark: egui_plot::GridMark, _range: &std::ops::RangeInclusive<f64>) -> String {
    axis_rate(mark.value)
}

/// Extra plot navigation on top of egui_plot's own (left drag pans the time
/// axis, double click resets): a plain mouse wheel zooms the time axis
/// around the pointer, like Ctrl + wheel does by default, and a middle
/// button drag pans it too. The wheel is swallowed so nothing else scrolls.
/// The rate axis keeps fitting the data, so zero stays at the bottom. The
/// plot's own scroll-to-pan is off (`allow_scroll(false)`).
pub fn plot_navigation(pui: &mut egui_plot::PlotUi<'_>) {
    let response = pui.response().clone();
    if response.dragged_by(egui::PointerButton::Middle) {
        // Screen pixels to plot units along x; dragging right moves the
        // view left, like the built in left button pan.
        let per_pixel = pui.transform().dvalue_dpos()[0];
        let dx = -f64::from(response.drag_delta().x) * per_pixel;
        pui.translate_bounds(egui::vec2(dx as f32, 0.0));
        pui.set_auto_bounds([false, true]);
    }

    if !response.contains_pointer() {
        return;
    }
    let ctx = pui.ctx().clone();
    let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
    if scroll == 0.0 {
        return;
    }
    // Same curve as egui's own wheel zoom: wheel up zooms in.
    let factor = (scroll / 200.0).exp();
    pui.zoom_bounds_around_hovered(egui::vec2(factor, 1.0));
    // A zoom switches auto bounds off on both axes; put the rate axis back.
    pui.set_auto_bounds([false, true]);
    ctx.input_mut(|i| {
        i.smooth_scroll_delta.y = 0.0;
    });
}

/// What a plot looked like last frame, to show a usage hint next to it.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlotHint {
    /// The pointer was over the plot.
    pub hovered: bool,
    /// The view is zoomed or panned (auto bounds are off on some axis).
    pub moved: bool,
}

impl PlotHint {
    /// The hint to show: how to use the plot while hovered, and the way back
    /// for as long as the view is zoomed or panned.
    pub fn text(self) -> Option<&'static str> {
        if self.moved {
            Some("Double click the plot to reset the view")
        } else if self.hovered {
            Some(
                "Scroll to zoom the time axis, drag to pan, double click to reset, right click for options",
            )
        } else {
            None
        }
    }
}

impl From<&egui_plot::PlotResponse<egui::Vec2b>> for PlotHint {
    /// `inner` is the plot's auto bounds flags.
    fn from(plot: &egui_plot::PlotResponse<egui::Vec2b>) -> Self {
        PlotHint {
            hovered: plot.response.hovered(),
            moved: !(plot.inner.x && plot.inner.y),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_grid_uses_binary_steps_that_fit_the_plot() {
        // A 62 point plot showing up to 2.1 MB/s: 8 points are 271 KB, so
        // the labelled step is 1 MB and the halves are minor lines.
        let marks = rate_grid(egui_plot::GridInput {
            bounds: (0.0, 2.1 * MB),
            base_step_size: 2.1 * MB / 62.0 * 8.0,
        });
        let major: Vec<f64> = marks
            .iter()
            .filter(|m| m.step_size == MB)
            .map(|m| m.value)
            .collect();
        // 2 MB/s is 3 points under the top edge, so it is dropped.
        assert_eq!(major, vec![0.0, MB]);
        assert_eq!(axis_rate(major[1]), "1 MB/s");
        let minor: Vec<String> = marks
            .iter()
            .filter(|m| m.step_size == MB / 2.0)
            .map(|m| axis_rate(m.value))
            .collect();
        assert_eq!(minor, vec!["512 KB/s", "1.5 MB/s"]);

        // Steps climb through 1, 2, 5 and their tens within a unit.
        assert_eq!(nice_rate_step(3.0 * KB), 5.0 * KB);
        assert_eq!(nice_rate_step(60.0 * KB), 100.0 * KB);
        assert_eq!(nice_rate_step(600.0 * KB), MB);
        assert_eq!(nice_rate_step(1.0), 1.0);
        assert!(
            rate_grid(egui_plot::GridInput {
                bounds: (0.0, 0.0),
                base_step_size: 0.0,
            })
            .is_empty()
        );

        // A mark right at the top edge is dropped so its label cannot spill
        // out of the plot: 2 MB/s sits 2 points under the top here.
        let marks = rate_grid(egui_plot::GridInput {
            bounds: (0.0, 2.05 * MB),
            base_step_size: 2.05 * MB / 62.0 * 8.0,
        });
        let top = marks.iter().map(|m| m.value).fold(0.0, f64::max);
        assert_eq!(top, 1.5 * MB);
    }
}
