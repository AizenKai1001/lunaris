//! `UiPlot`: a live graph of a resource, as bars made of ordinary `Ui` rectangles (so both renderers draw it with no new code, and the editor and
//! the tools see it like any other interface). One entity with a `UiPlot` and a `Ui` (the frame and background) is all a scene has to say: the first
//! tick the system makes the bars. See `docs/UI.md`.
use crate::components::{Ui, UiPlot, UiPlotBar, PLOT_MAX};
use lunaris_ecs::{Entity, World};
use std::collections::BTreeMap;

/// Where each bar goes and how tall it is, and the plots' samples, once per tick (after the layout, so a plot inside a container follows it).
pub fn update(world: &mut World) {
    let mut plots: Vec<(u64, Entity, UiPlot, Ui)> = Vec::new();
    let _ = world.for_each_uid::<(&UiPlot, &Ui), _>(|e, uid, (p, u)| plots.push((uid, e, *p, *u)));
    if plots.is_empty() {
        return;
    }
    let mut new_bars: Vec<(u64, u32, Ui)> = Vec::new();
    let mut views: BTreeMap<u64, [f32; PLOT_MAX]> = BTreeMap::new();
    for (uid, e, mut p, ui) in plots {
        let n = (p.samples as usize).clamp(1, PLOT_MAX);
        // the bars, once
        if !p.built {
            for i in 0..n {
                let bar = Ui { pos: ui.pos, size: [0.0, 0.0], color: p.bar_color, back_color: [0.0; 4], fill: 1.0, border: 0.0, layer: ui.layer + 1, visible: false, ..Ui::default() };
                new_bars.push((uid, i as u32, bar));
            }
            p.built = true;
        }
        // a new sample every `every` ticks
        p.timer += 1;
        if p.timer >= p.every.max(1) {
            p.timer = 0;
            let name = world.strings().get(p.source.0).to_string();
            let raw = if name.is_empty() { 0.0 } else { world.resource(&name).unwrap_or(0.0) } as f32;
            let value = if p.log { raw.max(1e-9).log10() } else { raw };
            let slot = (p.head as usize) % PLOT_MAX;
            p.data[slot] = if value.is_finite() { value } else { 0.0 };
            p.head = ((slot + 1) % PLOT_MAX) as u32;
            p.count = (p.count + 1).min(PLOT_MAX as u32);
        }
        // the scale: the plot's own, or from the samples that are shown
        let shown = (p.count as usize).min(n);
        let at = |i: usize| p.data[(p.head as usize + PLOT_MAX - shown + i) % PLOT_MAX];
        let (lo, hi) = if p.max > p.min {
            (p.min, p.max)
        } else if shown == 0 {
            (0.0, 1.0)
        } else {
            let (mut mn, mut mx) = (f32::MAX, f32::MIN);
            for i in 0..shown {
                mn = mn.min(at(i));
                mx = mx.max(at(i));
            }
            // a graph of counts starts at zero; a graph of logarithms starts at its lowest sample
            let lo = if p.log { mn } else { mn.min(0.0) };
            (lo, if mx > lo { mx } else { lo + 1.0 })
        };
        p.scale_lo = lo;
        p.scale_hi = hi;
        let mut view = [0.0f32; PLOT_MAX];
        for (i, slot) in view.iter_mut().enumerate().take(n) {
            // bar `i` shows the sample `n - 1 - i` places before the newest; bars before the first sample stay empty
            *slot = if i + shown >= n { ((at(i + shown - n) - lo) / (hi - lo)).clamp(0.0, 1.0) } else { -1.0 };
        }
        views.insert(uid, view);
        if let Some(w) = world.get_mut::<UiPlot>(e) {
            *w = p;
        }
    }
    for (plot, index, bar) in new_bars {
        let _ = world.spawn_bundle((bar, UiPlotBar { plot: lunaris_ecs::Uid(plot), index }));
    }
    // the bars follow their plot's rectangle, layer and colour
    struct Frame {
        pos: [f32; 2],
        size: [f32; 2],
        gap: f32,
        n: u32,
        shown: bool,
        layer: u32,
        color: [f32; 4],
    }
    let mut frames: BTreeMap<u64, Frame> = BTreeMap::new();
    let _ = world.for_each_uid::<(&UiPlot, &Ui), _>(|_, uid, (p, u)| {
        frames.insert(uid, Frame { pos: u.pos, size: u.size, gap: p.gap, n: p.samples.clamp(1, PLOT_MAX as u32), shown: u.visible, layer: u.layer + 1, color: p.bar_color });
    });
    let mut moves: Vec<(Entity, Ui)> = Vec::new();
    let _ = world.for_each::<(&UiPlotBar, &Ui), _>(|e, (b, u)| {
        if let (Some(f), Some(view)) = (frames.get(&b.plot.0), views.get(&b.plot.0)) {
            let i = b.index as usize;
            let h = if b.index < f.n { view[i] } else { -1.0 };
            let slot = f.size[0] / f.n as f32;
            let w = slot * (1.0 - f.gap.clamp(0.0, 0.9));
            let x = f.pos[0] + slot * i as f32 + (slot - w) * 0.5;
            let bar_h = f.size[1] * h.max(0.0);
            moves.push((e, Ui { pos: [x, f.pos[1] + f.size[1] - bar_h], size: [w, bar_h], color: f.color, layer: f.layer, visible: f.shown && h > 0.0, ..*u }));
        }
    });
    for (e, ui) in moves {
        if let Some(u) = world.get_mut::<Ui>(e) {
            *u = ui;
        }
    }
}
