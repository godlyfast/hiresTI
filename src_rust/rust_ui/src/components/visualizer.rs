//! Spectrum visualizer. A thin GtkDrawingArea that renders the
//! current `viz_core::VizStateEngine` smoothed bars in one of several
//! modes (bars / line / spiral). State is driven externally —
//! AppController polls `Engine::copy_spectrum_mono` on a 33ms tick
//! and feeds frames in via `BarsVisualizerInput::SetFrame`. Each
//! tick advances the EMA + spiral phase + queues a redraw, so the
//! visualization stays animated even between fresh audio frames
//! (state machine "settles" during silence).
//!
//! The draw closure can't borrow component state, so the model
//! mirrors the smoothed values into an `Rc<RefCell<PaintData>>` the
//! draw closure clones at attach time.

use std::cell::RefCell;
use std::rc::Rc;

use relm4::gtk::cairo::Context as CairoContext;
use relm4::gtk::{prelude::*, DrawingArea};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use viz_core::{
    build_line_points_rs, build_spiral_points_rs, map_log_spectrum, VizStateEngine,
};

/// Render mode. Mirrors the `viz_mode` setting in `settings.extra`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VizMode {
    Bars,
    Line,
    Spiral,
}

impl VizMode {
    pub fn from_id(id: &str) -> Self {
        match id {
            "line" => Self::Line,
            "spiral" => Self::Spiral,
            _ => Self::Bars,
        }
    }
}

impl Default for VizMode {
    fn default() -> Self {
        Self::Bars
    }
}

/// Number of rendered bars. 64 is a sweet spot — fine enough to look
/// dense, coarse enough to read at small heights (mini-player strip).
const BAR_COUNT: usize = 64;
/// Spectrum normalization range. Matches the Python defaults.
const DB_MIN: f32 = -80.0;
const DB_RANGE: f32 = 80.0;

#[derive(Debug, Clone, Copy, Default)]
struct ColorRgba {
    r: f64,
    g: f64,
    b: f64,
    a: f64,
}

#[derive(Default)]
struct PaintData {
    bars: Vec<f32>,
    peaks: Vec<f32>,
    /// Phase accumulator for the spiral mode. Bumped each Tick.
    phase: f32,
    mode: VizMode,
    /// Disabled state renders a flat gradient placeholder so the
    /// strip doesn't suddenly disappear when the engine isn't pushing
    /// frames.
    active: bool,
}

pub struct BarsVisualizerModel {
    state: VizStateEngine,
    last_seq: u64,
    /// Scratch buffer for the bar-mapped spectrum (raw FFT → log-spaced
    /// bar bins). Reused between frames to avoid alloc churn.
    bar_input: Vec<f32>,
    paint: Rc<RefCell<PaintData>>,
    active: bool,
    mode: VizMode,
    phase: f32,
}

#[derive(Debug, Clone)]
pub enum BarsVisualizerInput {
    /// New raw spectrum frame from the engine. `seq` is bumped per
    /// frame; we drop the input when seq hasn't changed.
    SetFrame { seq: u64, values: Vec<f32> },
    /// Advance the state EMA by one step + queue redraw. Fires on the
    /// 33ms timer regardless of whether a new frame arrived, so the
    /// bars decay smoothly during silence.
    Tick,
    /// Suspend / resume rendering. Disabled state idles + draws a
    /// flat baseline.
    SetActive(bool),
    /// Switch to a different render mode. Cheap — just updates the
    /// PaintData mode tag; the next redraw picks the new path.
    SetMode(VizMode),
}

impl SimpleComponent for BarsVisualizerModel {
    type Init = ();
    type Input = BarsVisualizerInput;
    type Output = ();
    type Root = DrawingArea;
    type Widgets = DrawingArea;

    fn init_root() -> Self::Root {
        DrawingArea::builder()
            .height_request(56)
            .hexpand(true)
            .css_classes(["bars-visualizer"])
            .build()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let paint: Rc<RefCell<PaintData>> = Rc::new(RefCell::new(PaintData {
            bars: vec![0.0; BAR_COUNT],
            peaks: vec![0.0; BAR_COUNT],
            phase: 0.0,
            mode: VizMode::Bars,
            active: false,
        }));
        let paint_for_draw = Rc::clone(&paint);
        root.set_draw_func(move |_area, cr, w, h| {
            draw(cr, w, h, &paint_for_draw.borrow());
        });

        let widgets = root.clone();
        let model = Self {
            state: VizStateEngine::new(BAR_COUNT, 0.45, 0.85, 18, 0.012, 0.18),
            last_seq: 0,
            bar_input: vec![0.0; BAR_COUNT],
            paint,
            active: false,
            mode: VizMode::Bars,
            phase: 0.0,
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            BarsVisualizerInput::SetFrame { seq, values } => {
                if seq == self.last_seq {
                    return;
                }
                self.last_seq = seq;
                map_log_spectrum(&values, &mut self.bar_input, DB_MIN, DB_RANGE);
                self.state.set_target_from_slice(&self.bar_input);
            }
            BarsVisualizerInput::Tick => {
                self.state.tick();
                // Phase advances at ~30Hz; 0.04 rad/tick gives a
                // ~1.2 Hz spiral rotation rate, which reads as
                // "subtly drifting" without being dizzying.
                self.phase += 0.04;
                if self.phase > std::f32::consts::TAU * 32.0 {
                    self.phase -= std::f32::consts::TAU * 32.0;
                }
            }
            BarsVisualizerInput::SetActive(active) => {
                self.active = active;
                if !active {
                    self.state.reset();
                }
            }
            BarsVisualizerInput::SetMode(m) => {
                self.mode = m;
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        // Mirror state into the shared paint buffer first; the borrow
        // must end before queue_draw runs so the draw closure can
        // re-borrow when GTK schedules the repaint.
        {
            let mut p = self.paint.borrow_mut();
            p.active = self.active;
            p.mode = self.mode;
            p.phase = self.phase;
            let cur = self.state.current();
            let peak = self.state.peak();
            for i in 0..BAR_COUNT {
                p.bars[i] = cur.get(i).copied().unwrap_or(0.0);
                p.peaks[i] = peak.get(i).copied().unwrap_or(0.0);
            }
        }
        widgets.queue_draw();
    }
}

fn draw(cr: &CairoContext, w: i32, h: i32, paint: &PaintData) {
    let width = w.max(1) as f64;
    let height = h.max(1) as f64;
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    cr.rectangle(0.0, 0.0, width, height);
    let _ = cr.fill();

    if paint.bars.is_empty() {
        return;
    }
    match paint.mode {
        VizMode::Bars => draw_bars(cr, width, height, paint),
        VizMode::Line => draw_line(cr, width, height, paint),
        VizMode::Spiral => draw_spiral(cr, width, height, paint),
    }
}

fn draw_bars(cr: &CairoContext, width: f64, height: f64, paint: &PaintData) {
    let n = paint.bars.len();
    let gap_px: f64 = 2.0;
    let total_gap = gap_px * (n.saturating_sub(1)) as f64;
    let bar_w = ((width - total_gap) / n as f64).max(1.0);

    for i in 0..n {
        let x = i as f64 * (bar_w + gap_px);
        let val = paint.bars[i].clamp(0.0, 1.0) as f64;
        let bar_h = val * height;
        let y_top = height - bar_h;

        let color = bar_color(i, n, val, paint.active);
        cr.set_source_rgba(color.r, color.g, color.b, color.a);
        cr.rectangle(x, y_top, bar_w, bar_h);
        let _ = cr.fill();

        // Peak indicator: 2px tall slice at the peak height.
        let peak = paint.peaks[i].clamp(0.0, 1.0) as f64;
        if peak > val + 0.01 {
            let py = (height - peak * height).max(0.0);
            cr.set_source_rgba(color.r, color.g, color.b, (color.a * 1.4).min(1.0));
            cr.rectangle(x, py, bar_w, 2.0);
            let _ = cr.fill();
        }
    }
}

fn draw_line(cr: &CairoContext, width: f64, height: f64, paint: &PaintData) {
    let pts = build_line_points_rs(&paint.bars, width as f32, height as f32, 1.0);
    if pts.len() < 2 {
        return;
    }
    let alpha = if paint.active { 0.95 } else { 0.45 };
    cr.set_line_width(2.4);
    cr.set_source_rgba(0.40, 0.85, 1.0, alpha);
    cr.move_to(pts[0].0 as f64, pts[0].1 as f64);
    for &(x, y) in pts.iter().skip(1) {
        cr.line_to(x as f64, y as f64);
    }
    let _ = cr.stroke();

    // Subtle fill underneath the curve so the strip reads as
    // amplitude-over-frequency rather than just a wiggly line.
    cr.move_to(pts[0].0 as f64, height);
    for &(x, y) in pts.iter() {
        cr.line_to(x as f64, y as f64);
    }
    cr.line_to(pts[pts.len() - 1].0 as f64, height);
    cr.close_path();
    cr.set_source_rgba(0.40, 0.85, 1.0, alpha * 0.18);
    let _ = cr.fill();
}

fn draw_spiral(cr: &CairoContext, width: f64, height: f64, paint: &PaintData) {
    let pts = build_spiral_points_rs(
        &paint.bars,
        width as f32,
        height as f32,
        paint.phase,
        1.0,
    );
    if pts.is_empty() {
        return;
    }
    let alpha_floor = if paint.active { 0.30 } else { 0.10 };
    for (x, y, lvl, t) in pts {
        let val = lvl as f64;
        let r = 0.40 + 0.55 * t as f64;
        let g = 0.20 + 0.55 * (1.0 - t as f64) + 0.20 * val;
        let b = 0.95 - 0.50 * t as f64 + 0.05 * val;
        let a = alpha_floor + 0.65 * val;
        let dot_r = (1.4 + 3.6 * val).max(0.6);
        cr.set_source_rgba(r, g, b, a);
        cr.arc(x as f64, y as f64, dot_r, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }
}

fn bar_color(i: usize, n: usize, val: f64, active: bool) -> ColorRgba {
    let t = if n <= 1 { 0.0 } else { i as f64 / (n - 1) as f64 };
    // Cool-blue → magenta gradient with a small green-to-red hot spot
    // at the very top of each bar so transients pop visually.
    let r = 0.10 + 0.85 * t;
    let g = 0.55 - 0.30 * t + 0.25 * val;
    let b = 1.00 - 0.65 * t;
    let alpha_floor = if active { 0.22 } else { 0.10 };
    let a = alpha_floor + 0.60 * val;
    ColorRgba { r, g, b, a }
}
