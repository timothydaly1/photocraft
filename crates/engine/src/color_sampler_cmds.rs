//! The Color Sampler tool (eyedropper group, #1046): up to ten fixed points per document whose
//! colours the Info panel reads out as #1, #2, …, kept while the image is edited so a correction
//! can be watched at the same pixels.
//!
//! The points are document data ([`photocraft_doc::Measurement::color_samplers`], saved in
//! `.pcraft`). As in Photoshop, placing, moving or deleting a sampler is not a history step; like
//! the Ruler line, a change leaves a clean document clean.

use photocraft_doc::{Document, MAX_COLOR_SAMPLERS};
use serde_json::{Value, json};

use crate::analysis_cmds::set_quiet;
use crate::commands::CommandSpec;
use crate::sample_cmds::{layers_param, sample_color, size_param};
use crate::{EngineError, Result, Session};

type Run = fn(&mut Session, &Value) -> Result<Value>;
type Enabled = fn(&Session) -> std::result::Result<(), String>;

fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: msg.into() }
}

fn has_doc(s: &Session) -> std::result::Result<(), String> {
    s.active().map(|_| ()).ok_or_else(|| "no document open".into())
}

fn samplers(s: &Session) -> &[[f64; 2]] {
    s.active().map_or(&[], |d| d.doc.measurement.color_samplers.as_slice())
}

fn has_samplers(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if samplers(s).is_empty() { Err("no color samplers".into()) } else { Ok(()) }
}

/// `[x, y]` from `key` (an `[x, y]` array) or from the top-level `x` and `y`.
fn point(p: &Value, key: &str) -> Option<[f64; 2]> {
    let from_array = || {
        let a = p.get(key)?.as_array()?;
        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?])
    };
    from_array().or_else(|| Some([p.get("x")?.as_f64()?, p.get("y")?.as_f64()?]))
}

/// The centre of the canvas pixel under `at`: where a sampler sits, as in Photoshop, which reads
/// whole pixels. Errors off the canvas or for non-finite coordinates.
fn pixel_centre(cmd: &str, d: &Document, at: [f64; 2]) -> Result<[f64; 2]> {
    let (w, h) = (f64::from(d.size.width), f64::from(d.size.height));
    if !at[0].is_finite() || !at[1].is_finite() {
        return Err(bad(cmd, "coordinates must be finite"));
    }
    if at[0] < 0.0 || at[1] < 0.0 || at[0] >= w || at[1] >= h {
        return Err(bad(cmd, format!("({}, {}) is off the {}×{} canvas", at[0], at[1], d.size.width, d.size.height)));
    }
    Ok([at[0].floor() + 0.5, at[1].floor() + 0.5])
}

/// The `index` param: a 0-based sampler that exists.
fn index(cmd: &str, s: &Session, p: &Value) -> Result<usize> {
    let n = samplers(s).len();
    let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "pass `index` (0-based)"))?;
    usize::try_from(i).ok().filter(|i| *i < n).ok_or_else(|| bad(cmd, format!("no sampler {i} ({n} placed)")))
}

/// Every sampler with its colour: `{samplers:[{index, number, x, y, color:[r,g,b,a]}], max}`.
/// `size` and `sampleLayer` are the Eyedropper's (`document.sampleColor`).
fn list(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "colorSampler.list";
    let size = size_param(CMD, p)?;
    let which = layers_param(CMD, p)?;
    let st = s.active().ok_or(EngineError::NoDocument)?;
    let rows: Vec<Value> = st
        .doc
        .measurement
        .color_samplers
        .iter()
        .enumerate()
        .map(|(i, at)| {
            let c = sample_color(&st.doc, st.active_layer, at[0], at[1], size, which);
            json!({"index": i, "number": i + 1, "x": at[0], "y": at[1], "color": c})
        })
        .collect();
    Ok(json!({"samplers": rows, "max": MAX_COLOR_SAMPLERS}))
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "colorSampler.add";
    let at = point(p, "at").ok_or_else(|| bad(CMD, "pass `x` and `y` (document pixels)"))?;
    let d = s.active().ok_or(EngineError::NoDocument)?.doc.clone();
    let at = pixel_centre(CMD, &d, at)?;
    if d.measurement.color_samplers.len() >= MAX_COLOR_SAMPLERS {
        return Err(bad(CMD, format!("a document has at most {MAX_COLOR_SAMPLERS} color samplers; delete one first")));
    }
    set_quiet(s, |d| d.measurement.color_samplers.push(at))?;
    let mut r = list(s, &json!({}))?;
    r["number"] = json!(samplers(s).len());
    Ok(r)
}

fn move_to(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "colorSampler.move";
    let i = index(CMD, s, p)?;
    let to = p.get("to").and_then(Value::as_array).and_then(|a| Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?]));
    let to = to.ok_or_else(|| bad(CMD, "pass `to`: [x, y]"))?;
    let d = s.active().ok_or(EngineError::NoDocument)?.doc.clone();
    let to = pixel_centre(CMD, &d, to)?;
    set_quiet(s, |d| {
        if let Some(q) = d.measurement.color_samplers.get_mut(i) {
            *q = to;
        }
    })?;
    list(s, &json!({}))
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "colorSampler.remove";
    let i = index(CMD, s, p)?;
    set_quiet(s, |d| {
        if i < d.measurement.color_samplers.len() {
            d.measurement.color_samplers.remove(i);
        }
    })?;
    list(s, &json!({}))
}

fn clear(s: &mut Session, _p: &Value) -> Result<Value> {
    set_quiet(s, |d| d.measurement.color_samplers.clear())?;
    list(s, &json!({}))
}

fn spec(id: &'static str, label: &'static str, params: &'static str, enabled: Enabled, run: Run, journal: bool) -> CommandSpec {
    CommandSpec { id, label, menu: &[], shortcut: None, params, enabled, run, journal }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec(
            "colorSampler.list",
            "Color Samplers",
            r##"{"size":"point"|1|3|5|11|31|51|101=1,"sampleLayer":"current"|"currentAndBelow"|"all"|"allNoAdjustments"|"currentAndBelowNoAdjustments"="all"} → {"samplers":[{"index","number","x","y","color":[r,g,b,a]}],"max":10}"##,
            has_doc,
            list,
            false,
        ),
        spec(
            "colorSampler.add",
            "Add Color Sampler",
            r##"{"x":px,"y":px} | {"at":[x,y]} (snapped to the pixel centre; at most 10 per document) → the list plus the new sampler's "number""##,
            has_doc,
            add,
            true,
        ),
        spec("colorSampler.move", "Move Color Sampler", r##"{"index":n (0-based),"to":[x,y]}"##, has_samplers, move_to, true),
        spec("colorSampler.remove", "Delete Color Sampler", r##"{"index":n (0-based)}"##, has_samplers, remove, true),
        spec("colorSampler.clear", "Clear All Color Samplers", r##"{}"##, has_samplers, clear, true),
    ]
}

#[cfg(test)]
mod tests;
