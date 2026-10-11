//! The Art History Brush tool: a canvas stroke dispatches `paint.artHistoryBrush` with the options
//! bar's Mode, Style, Area, Tolerance and Opacity, one History step painted from the Open state; Y
//! cycles it with the History Brush; `ui.set`, the options bar and the brush gestures.

use egui::{Key, Modifiers, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::PhotocraftApp;
use crate::canvas::{ToolEvent, tool_event};
use crate::state::Tool;

/// An 80×40 document opened red on the left and blue on the right, then painted black, with the
/// Art History Brush picked (4 px, hard).
fn app() -> PhotocraftApp {
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
    app.run("file.new", json!({"width": 80, "height": 40, "background": "#ff0000"})).unwrap();
    app.run("select.rect", json!({"x": 40, "y": 0, "width": 40, "height": 40})).unwrap();
    app.run("edit.fill", json!({"color": "#0000ff"})).unwrap();
    app.run("select.all", json!({})).unwrap();
    app.run("edit.fill", json!({"color": "#000000"})).unwrap();
    app.run("select.deselect", json!({})).unwrap();
    app.ui.tool = Tool::ArtHistoryBrush;
    crate::paint_mouse::sync_tool_brush(&mut app);
    app.run("tools.setBrush", json!({"brush": {"size": 4, "hardness": 1.0, "pressureSize": false, "smoothing": {"amount": 0.0}}})).unwrap();
    app
}

fn rgba(app: &PhotocraftApp, x: i32, y: i32) -> [f32; 4] {
    let st = app.session.active().unwrap();
    st.doc.layer(st.active_layer.unwrap()).unwrap().surface().unwrap().rgba(x, y)
}

fn drag(app: &mut PhotocraftApp, pts: &[(f64, f64)]) {
    let m = Modifiers::NONE;
    let (x, y) = pts[0];
    tool_event(app, ToolEvent::Down { x, y, pressure: 1.0 }, m);
    for &(x, y) in &pts[1..] {
        tool_event(app, ToolEvent::Move { x, y, pressure: 1.0 }, m);
    }
    let &(x, y) = pts.last().unwrap();
    tool_event(app, ToolEvent::Up { x, y }, m);
    assert!(!app.ui.status_error, "{}", app.ui.status);
}

/// The source is the History Brush's: the oldest held state (the white-filled document as it was
/// created, before the red/blue fills). Strokes carry the options bar's settings.
#[test]
fn a_stroke_paints_from_history_with_the_options_bar() {
    let mut a = app();
    let o = &mut a.ui.tool_options;
    (o.ah_style, o.ah_area, o.ah_tolerance, o.ah_mode) = ("tightMedium".into(), 20.0, 0.0, "Lighten".into());
    let steps = a.session.active().unwrap().history.past_len();
    drag(&mut a, &[(20.0, 20.0), (40.0, 20.0), (60.0, 20.0)]);
    let (id, p) = a.session.journal.last().cloned().unwrap();
    assert_eq!(id, "paint.artHistoryBrush");
    for (k, v) in [("style", json!("tightMedium")), ("area", json!(20.0)), ("tolerance", json!(0.0)), ("mode", json!("Lighten")), ("opacity", json!(100.0))] {
        assert_eq!(p[k], v, "{k}: {p}");
    }
    assert_eq!(a.session.active().unwrap().history.past_len(), steps + 1, "one History step");
    // The opening state was red (the background colour): strokes brought red back over the black.
    let painted = (0..80).flat_map(|x| (0..40).map(move |y| (x, y))).filter(|&(x, y)| rgba(&a, x, y)[0] > 0.5).count();
    assert!(painted > 20, "{painted} pixels painted");
    assert_eq!(rgba(&a, 2, 2), [0.0, 0.0, 0.0, 1.0], "away from the stroke");
}

#[test]
fn y_cycles_history_brush_and_art_history_brush_and_ui_set_picks_it() {
    let mut h = Harness::builder().with_size(vec2(1280.0, 800.0)).with_max_steps(64).build_eframe(|cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
        app()
    });
    h.state_mut().ui.tool = Tool::Move;
    h.run_steps(4);
    let press = |h: &mut Harness<'_, PhotocraftApp>| {
        h.event(egui::Event::Key { key: Key::Y, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        h.event(egui::Event::Key { key: Key::Y, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
        h.run_steps(2);
    };
    // Y returns to the slot's last tool (the Art History Brush, picked in `app`), then cycles.
    press(&mut h);
    assert_eq!(h.state().ui.tool, Tool::ArtHistoryBrush);
    press(&mut h);
    assert_eq!(h.state().ui.tool, Tool::HistoryBrush);
    press(&mut h);
    assert_eq!(h.state().ui.tool, Tool::ArtHistoryBrush);
    // Its options bar: Opacity (no Flow), Mode, Style, Area and Tolerance.
    for label in ["Style:", "Area:", "Tolerance:"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    assert!(h.query_by_label("Flow").is_none(), "no Flow, as in Photoshop");
    h.state_mut().ui.tool = Tool::Move;
    let ctx = egui::Context::default();
    let (req, _rx) = crate::control::ControlRequest::new("ui.set", json!({"tool": "artHistoryBrush"}));
    let _ = crate::control::handle(h.state_mut(), &ctx, &req);
    assert_eq!(h.state().ui.tool, Tool::ArtHistoryBrush);
    assert_eq!(crate::control::inspect(h.state(), &ctx)["tool"], "ArtHistoryBrush");
}

#[test]
fn it_paints_like_the_brush_tools() {
    assert!(Tool::ArtHistoryBrush.is_brushlike(), "brush cursor, brush chip, [ ] and ⇧[ ⇧]");
    assert!(crate::brush_resize::applies(Tool::ArtHistoryBrush));
    assert!(crate::canvas::freehand_tool(Tool::ArtHistoryBrush));
    let mut a = app();
    let size = a.session.tools.brush.size;
    a.run("tools.increaseBrushSize", json!({})).unwrap();
    assert!(a.session.tools.brush.size > size);
    // Reset Tool puts Photoshop's defaults back.
    let o = &mut a.ui.tool_options;
    (o.ah_style, o.ah_area, o.ah_tolerance, o.ah_mode) = ("dab".into(), 3.0, 40.0, "Hue".into());
    crate::tool_reset::reset_options(&mut a.ui.tool_options, Tool::ArtHistoryBrush);
    let o = &a.ui.tool_options;
    assert_eq!((o.ah_style.as_str(), o.ah_area, o.ah_tolerance, o.ah_mode.as_str()), ("tightShort", 50.0, 0.0, "Normal"));
}
