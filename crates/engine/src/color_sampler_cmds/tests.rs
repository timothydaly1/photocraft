use super::*;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({"width": 40, "height": 30, "background": "white"})).unwrap();
    s
}

fn points(s: &Session) -> Vec<[f64; 2]> {
    s.active().unwrap().doc.measurement.color_samplers.clone()
}

#[test]
fn samplers_snap_to_pixel_centres_and_read_the_composite() {
    let mut s = session();
    let r = s.execute("colorSampler.add", json!({"x": 3.9, "y": 7.2})).unwrap();
    assert_eq!(r["number"], 1);
    s.execute("colorSampler.add", json!({"at": [20, 10]})).unwrap();
    assert_eq!(points(&s), vec![[3.5, 7.5], [20.5, 10.5]]);
    let l = s.execute("colorSampler.list", json!({})).unwrap();
    assert_eq!(l["max"], 10);
    assert_eq!(l["samplers"][1]["number"], 2);
    assert_eq!(l["samplers"][0]["color"], json!([1.0, 1.0, 1.0, 1.0]), "white background");

    // The readout follows edits at the same pixel: fill the left half red.
    s.execute("select.rect", json!({"x": 0, "y": 0, "width": 10, "height": 30})).unwrap();
    s.execute("edit.fill", json!({"use": "color", "color": "#ff0000"})).unwrap();
    let l = s.execute("colorSampler.list", json!({})).unwrap();
    assert_eq!(l["samplers"][0]["color"], json!([1.0, 0.0, 0.0, 1.0]), "#1 sees the fill");
    assert_eq!(l["samplers"][1]["color"], json!([1.0, 1.0, 1.0, 1.0]), "#2 is outside it");
}

#[test]
fn move_remove_and_clear_renumber_without_history_steps() {
    let mut s = session();
    for x in [1, 2, 3] {
        s.execute("colorSampler.add", json!({"x": x, "y": 1})).unwrap();
    }
    s.execute("colorSampler.move", json!({"index": 0, "to": [30.2, 25.8]})).unwrap();
    assert_eq!(points(&s)[0], [30.5, 25.5]);
    let l = s.execute("colorSampler.remove", json!({"index": 1})).unwrap();
    assert_eq!(points(&s), vec![[30.5, 25.5], [3.5, 1.5]]);
    assert_eq!(l["samplers"][1]["number"], 2, "the later samplers move up a number");
    let st = s.active().unwrap();
    assert_eq!(st.history.past_len(), 0, "not history steps, as in Photoshop");
    assert_eq!(st.saved_revision, st.revision, "a clean document stays clean");
    s.execute("colorSampler.clear", json!({})).unwrap();
    assert!(points(&s).is_empty());
    assert!(!s.is_enabled("colorSampler.clear"), "nothing left to clear");
    assert!(!s.is_enabled("colorSampler.move"));
}

#[test]
fn at_most_ten_samplers() {
    let mut s = session();
    for i in 0..MAX_COLOR_SAMPLERS {
        s.execute("colorSampler.add", json!({"x": i, "y": 0})).unwrap();
    }
    let err = s.execute("colorSampler.add", json!({"x": 20, "y": 20})).unwrap_err();
    assert!(err.to_string().contains("at most 10"), "{err}");
    assert_eq!(points(&s).len(), MAX_COLOR_SAMPLERS);
}

#[test]
fn bad_params_are_errors_not_panics() {
    let mut s = Session::new();
    assert!(s.execute("colorSampler.add", json!({"x": 1, "y": 1})).is_err(), "no document");
    assert!(s.execute("colorSampler.list", json!({})).is_err(), "no document");
    let mut s = session();
    for p in [
        json!({}),
        json!({"x": "a", "y": 1}),
        json!({"x": -1, "y": 1}),
        json!({"x": 40, "y": 1}),
        json!({"x": 1, "y": 30}),
        json!({"x": 1e308, "y": 1}),
        json!({"at": [1]}),
        json!({"at": null}),
    ] {
        assert!(s.execute("colorSampler.add", p.clone()).is_err(), "{p}");
    }
    assert!(points(&s).is_empty());
    s.execute("colorSampler.add", json!({"x": 1, "y": 1})).unwrap();
    for p in [json!({}), json!({"index": 1, "to": [1, 1]}), json!({"index": -1, "to": [1, 1]}), json!({"index": 0}), json!({"index": 0, "to": [99, 1]})] {
        assert!(s.execute("colorSampler.move", p.clone()).is_err(), "{p}");
    }
    for p in [json!({}), json!({"index": 1}), json!({"index": u64::MAX}), json!({"index": "0"})] {
        assert!(s.execute("colorSampler.remove", p.clone()).is_err(), "{p}");
    }
    assert!(s.execute("colorSampler.list", json!({"size": 4})).is_err());
    assert!(s.execute("colorSampler.list", json!({"sampleLayer": "nope"})).is_err());
    assert_eq!(points(&s), vec![[1.5, 1.5]]);
}
