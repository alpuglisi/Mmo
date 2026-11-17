//! Devlog charts from a [`Timeline`]: population over time and gene drift,
//! as SVG (crisp, editable) and PNG (drawn with the film's rasterizer).
//! Clean style: white ground, hairline grid, direct labels, no chart junk.

use std::fmt::Write as _;

use super::raster::{text_width, Canvas, Rgb};
use super::timeline::Timeline;

const INK: Rgb = [33, 37, 41];
const MUTED: Rgb = [110, 118, 126];
const GRID: Rgb = [228, 231, 235];
const BAND: Rgb = [214, 69, 65];
const PAPER: Rgb = [255, 255, 255];

pub struct Series {
    pub name: String,
    pub color: Rgb,
    pub points: Vec<(f64, f64)>,
}

/// One panel: a title, lines on shared axes, shaded day ranges.
pub struct Panel {
    pub title: String,
    pub series: Vec<Series>,
    pub y_max: f64,
    pub x_max: f64,
    /// Shaded `(from, to, label)` ranges on the x axis (catastrophes).
    pub bands: Vec<(f64, f64, String)>,
    /// Label the line ends with their names.
    pub labels: bool,
}

/// Lineages worth showing: the `n` with the highest peak population.
fn top_lineages(t: &Timeline, n: usize) -> Vec<usize> {
    let mut peaks: Vec<(u32, usize)> = (0..t.lineages.len())
        .map(|l| (t.days.iter().map(|d| d.population.get(l).copied().unwrap_or(0)).max().unwrap_or(0), l))
        .filter(|p| p.0 > 0)
        .collect();
    peaks.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    peaks.into_iter().take(n).map(|p| p.1).collect()
}

fn years(t: &Timeline, day: u64) -> f64 {
    day as f64 / t.days_per_year.max(1) as f64
}

pub fn population_panel(t: &Timeline) -> Panel {
    let x_max = t.days.last().map_or(1.0, |d| years(t, d.day)).max(0.1);
    let mut series: Vec<Series> = top_lineages(t, 8)
        .into_iter()
        .map(|l| Series {
            name: t.lineages[l].name.clone(),
            color: t.lineages[l].color,
            points: t
                .days
                .iter()
                .map(|d| (years(t, d.day), d.population.get(l).copied().unwrap_or(0) as f64))
                .collect(),
        })
        .collect();
    let y_max = series.iter().flat_map(|s| s.points.iter().map(|p| p.1)).fold(1.0, f64::max);
    series.reverse(); // biggest drawn last, on top
    Panel {
        title: "Population by lineage".into(),
        series,
        y_max: axis_max(y_max),
        x_max,
        bands: t.catastrophes.iter().map(|c| (years(t, c.start_day), years(t, c.end_day), c.kind.clone())).collect(),
        labels: true,
    }
}

/// One panel per followed gene: each big lineage's mean over time.
pub fn gene_panels(t: &Timeline) -> Vec<Panel> {
    let x_max = t.days.last().map_or(1.0, |d| years(t, d.day)).max(0.1);
    let lineages = top_lineages(t, 6);
    (0..t.genes.len())
        .map(|g| Panel {
            title: t.genes[g].clone(),
            series: lineages
                .iter()
                .rev()
                .map(|&l| Series {
                    name: t.lineages[l].name.clone(),
                    color: t.lineages[l].color,
                    points: t
                        .days
                        .iter()
                        .filter(|d| d.population.get(l).copied().unwrap_or(0) >= 3)
                        .filter_map(|d| Some((years(t, d.day), d.genes.get(l)?.get(g).copied().flatten()? as f64)))
                        .collect(),
                })
                .collect(),
            y_max: 1.0,
            x_max,
            bands: Vec::new(),
            labels: false,
        })
        .collect()
}

/// 1, 2, 5 × 10^k at or above `v`.
fn nice(v: f64) -> f64 {
    let p = 10f64.powf(v.max(1e-9).log10().floor());
    [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * p).find(|&x| x >= v).unwrap_or(10.0 * p)
}

/// The axis top: `v` rounded up to a multiple of a nice tick step.
fn axis_max(v: f64) -> f64 {
    let step = nice(v / 4.0);
    (v / step).ceil() * step
}

fn ticks(max: f64, n: usize) -> Vec<f64> {
    let step = nice(max / n as f64);
    (0..).map(|k| k as f64 * step).take_while(|&v| v <= max + 1e-9).collect()
}

fn fmt_tick(v: f64) -> String {
    if v.fract().abs() < 1e-9 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Plot area of a panel in a `w × h` cell at the origin.
struct Frame {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Frame {
    fn new(w: f64, h: f64, labels: bool) -> Self {
        Frame { x0: 56.0, y0: 40.0, x1: w - if labels { 150.0 } else { 16.0 }, y1: h - 36.0 }
    }
    fn map(&self, p: &Panel, x: f64, y: f64) -> (f64, f64) {
        (self.x0 + (x / p.x_max) * (self.x1 - self.x0), self.y1 - (y / p.y_max).clamp(0.0, 1.0) * (self.y1 - self.y0))
    }
}

/// Direct labels at the line ends, nudged apart vertically.
fn end_labels(p: &Panel, f: &Frame, min_gap: f64) -> Vec<(f64, usize)> {
    let mut ends: Vec<(f64, usize)> =
        p.series.iter().enumerate().filter_map(|(i, s)| s.points.last().map(|&(x, y)| (f.map(p, x, y).1, i))).collect();
    ends.sort_by(|a, b| a.0.total_cmp(&b.0));
    for i in 1..ends.len() {
        if ends[i].0 - ends[i - 1].0 < min_gap {
            ends[i].0 = ends[i - 1].0 + min_gap;
        }
    }
    ends
}

fn panel_svg(svg: &mut String, p: &Panel, ox: f64, oy: f64, w: f64, h: f64) {
    let f = Frame::new(w, h, p.labels);
    let _ = writeln!(svg, r#"<g transform="translate({ox},{oy})">"#);
    let _ = writeln!(
        svg,
        r#"<text x="{}" y="22" font-size="15" font-weight="600" fill="{}">{}</text>"#,
        f.x0,
        hex(INK),
        esc(&p.title)
    );
    for (a, b, label) in &p.bands {
        let (xa, _) = f.map(p, *a, 0.0);
        let (xb, _) = f.map(p, *b, 0.0);
        let _ = writeln!(
            svg,
            r#"<rect x="{xa:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="{}" fill-opacity="0.08"><title>{}</title></rect>"#,
            f.y0,
            (xb - xa).max(1.0),
            f.y1 - f.y0,
            hex(BAND),
            esc(label)
        );
    }
    for v in ticks(p.y_max, 4) {
        let (_, y) = f.map(p, 0.0, v);
        let _ = writeln!(
            svg,
            r#"<line x1="{}" x2="{}" y1="{y:.1}" y2="{y:.1}" stroke="{}" stroke-width="1"/><text x="{}" y="{:.1}" font-size="11" text-anchor="end" fill="{}">{}</text>"#,
            f.x0,
            f.x1,
            hex(GRID),
            f.x0 - 8.0,
            y + 4.0,
            hex(MUTED),
            fmt_tick(v)
        );
    }
    for v in ticks(p.x_max, 6) {
        let (x, _) = f.map(p, v, 0.0);
        let _ = writeln!(
            svg,
            r#"<text x="{x:.1}" y="{:.1}" font-size="11" text-anchor="middle" fill="{}">{}</text>"#,
            f.y1 + 18.0,
            hex(MUTED),
            if v == 0.0 { "year 0".to_string() } else { fmt_tick(v) }
        );
    }
    let _ = writeln!(
        svg,
        r#"<line x1="{}" x2="{}" y1="{}" y2="{}" stroke="{}" stroke-width="1"/>"#,
        f.x0,
        f.x1,
        f.y1,
        f.y1,
        hex(MUTED)
    );
    for s in &p.series {
        if s.points.len() < 2 {
            continue;
        }
        let mut d = String::new();
        for (i, &(x, y)) in s.points.iter().enumerate() {
            let (px, py) = f.map(p, x, y);
            let _ = write!(d, "{}{px:.1},{py:.1}", if i == 0 { "M" } else { "L" });
        }
        let _ = writeln!(
            svg,
            r#"<path d="{d}" fill="none" stroke="{}" stroke-width="2" stroke-linejoin="round"><title>{}</title></path>"#,
            hex(s.color),
            esc(&s.name)
        );
    }
    if p.labels {
        for (y, i) in end_labels(p, &f, 14.0) {
            let s = &p.series[i];
            let _ = writeln!(
                svg,
                r#"<text x="{:.1}" y="{:.1}" font-size="12" fill="{}">{}</text>"#,
                f.x1 + 8.0,
                y + 4.0,
                hex(s.color),
                esc(&s.name)
            );
        }
    }
    let _ = writeln!(svg, "</g>");
}

/// Panels in a grid of `cols` columns, each `cw × ch`.
pub fn svg(title: &str, panels: &[Panel], cols: usize, cw: f64, ch: f64) -> String {
    let rows = panels.len().div_ceil(cols);
    let (w, h) = (cw * cols as f64, ch * rows as f64 + 44.0);
    let mut s = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="Inter, system-ui, -apple-system, Segoe UI, sans-serif">
<rect width="100%" height="100%" fill="{}"/>
<text x="20" y="30" font-size="20" font-weight="700" fill="{}">{}</text>
"#,
        hex(PAPER),
        hex(INK),
        esc(title)
    );
    for (i, p) in panels.iter().enumerate() {
        panel_svg(&mut s, p, (i % cols) as f64 * cw, 44.0 + (i / cols) as f64 * ch, cw, ch);
    }
    s.push_str("</svg>\n");
    s
}

fn panel_png(c: &mut Canvas, p: &Panel, ox: f64, oy: f64, w: f64, h: f64) {
    let f = Frame::new(w, h, p.labels);
    let at = |x: f64, y: f64| (ox + x, oy + y);
    let (tx, ty) = at(f.x0, 10.0);
    c.text(tx as i64, ty as i64, &p.title, 2, INK, 1.0);
    for (a, b, _) in &p.bands {
        let (xa, _) = f.map(p, *a, 0.0);
        let (xb, _) = f.map(p, *b, 0.0);
        c.fill_rect((ox + xa) as i64, (oy + f.y0) as i64, (xb - xa).max(1.0) as i64, (f.y1 - f.y0) as i64, BAND, 0.08);
    }
    for v in ticks(p.y_max, 4) {
        let (_, y) = f.map(p, 0.0, v);
        c.fill_rect((ox + f.x0) as i64, (oy + y) as i64, (f.x1 - f.x0) as i64, 1, GRID, 1.0);
        let label = fmt_tick(v);
        c.text((ox + f.x0 - 8.0) as i64 - text_width(&label, 1), (oy + y - 3.0) as i64, &label, 1, MUTED, 1.0);
    }
    for v in ticks(p.x_max, 6) {
        let (x, _) = f.map(p, v, 0.0);
        let label = if v == 0.0 { "year 0".to_string() } else { fmt_tick(v) };
        c.text((ox + x) as i64 - text_width(&label, 1) / 2, (oy + f.y1 + 10.0) as i64, &label, 1, MUTED, 1.0);
    }
    c.fill_rect((ox + f.x0) as i64, (oy + f.y1) as i64, (f.x1 - f.x0) as i64, 1, MUTED, 1.0);
    for s in &p.series {
        let pts: Vec<(f32, f32)> = s
            .points
            .iter()
            .map(|&(x, y)| {
                let (px, py) = f.map(p, x, y);
                ((ox + px) as f32, (oy + py) as f32)
            })
            .collect();
        c.polyline(&pts, 2.0, s.color, 1.0);
    }
    if p.labels {
        for (y, i) in end_labels(p, &f, 14.0) {
            let s = &p.series[i];
            c.text((ox + f.x1 + 8.0) as i64, (oy + y - 3.0) as i64, &s.name, 1, s.color, 1.0);
        }
    }
}

pub fn png(title: &str, panels: &[Panel], cols: usize, cw: f64, ch: f64) -> Canvas {
    let rows = panels.len().div_ceil(cols);
    let mut c = Canvas::new((cw * cols as f64) as usize, (ch * rows as f64 + 44.0) as usize, PAPER);
    c.text(20, 14, title, 3, INK, 1.0);
    for (i, p) in panels.iter().enumerate() {
        panel_png(&mut c, p, (i % cols) as f64 * cw, 44.0 + (i / cols) as f64 * ch, cw, ch);
    }
    c
}

/// Write population.{svg,png} and genes.{svg,png} into `dir`.
/// A chart written by [`write_all`]: its name, the files, and the PNG's size.
pub struct Written {
    pub name: &'static str,
    pub svg: std::path::PathBuf,
    pub png: std::path::PathBuf,
    pub width: usize,
    pub height: usize,
}

pub fn write_all(t: &Timeline, dir: &std::path::Path) -> std::io::Result<Vec<Written>> {
    let title = format!("Seed {} · {}x{} map", t.seed, t.size, t.size);
    let pop = [population_panel(t)];
    let genes = gene_panels(t);
    let mut written = Vec::new();
    let mut save = |name: &'static str, svg: String, png: Canvas| -> std::io::Result<()> {
        let svg_path = dir.join(format!("{name}.svg"));
        std::fs::write(&svg_path, svg)?;
        let png_path = dir.join(format!("{name}.png"));
        png.save_png(&png_path)?;
        written.push(Written { name, svg: svg_path, png: png_path, width: png.w, height: png.h });
        Ok(())
    };
    save("population", svg(&title, &pop, 1, 1200.0, 560.0), png(&title, &pop, 1, 1200.0, 560.0))?;
    let gt = format!("Gene drift (lineage means) · {title}");
    save("genes", svg(&gt, &genes, 5, 300.0, 220.0), png(&gt, &genes, 5, 300.0, 220.0))?;
    Ok(written)
}
