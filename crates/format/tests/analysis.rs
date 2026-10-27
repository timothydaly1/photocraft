//! Image › Analysis data and notes survive a `.pcraft` round trip; old manifests default them.

use photocraft_color::{Color, ColorMode, SampleType};
use photocraft_doc::{CountGroup, Document, MeasurementScale, Note, Ruler, Size};
use photocraft_format::*;

#[test]
fn measurement_and_notes_roundtrip_and_default_when_absent() {
    let mut doc = Document::new("a", Size::new(40, 30), ColorMode::Rgb, SampleType::U16);
    doc.measurement.scale = MeasurementScale { pixel_length: 72.0, logical_length: 1.0, units: "inches".into() };
    doc.measurement.count_groups = vec![CountGroup {
        name: "Cells".into(),
        color: Color::rgb(0.0, 0.5, 1.0),
        marker_size: 3,
        label_size: 12,
        visible: true,
        points: vec![[1.0, 2.0], [5.5, 6.5]],
    }];
    doc.measurement.ruler = Some(Ruler { start: [1.0, 1.0], end: [30.0, 20.0], protractor: Some([1.0, 25.0]) });
    doc.measurement.color_samplers = vec![[3.5, 4.5], [39.5, 29.5]];
    doc.notes = vec![Note {
        author: "A".into(),
        text: "Fix the sky\nplease".into(),
        position: [10.0, 12.0],
        popup: [10.0, 30.0, 200.0, 150.0],
        open: true,
        ..Default::default()
    }];
    let bytes = save_to_bytes(&doc, &SaveOptions::default()).unwrap();
    let back = load_from_bytes(&bytes).unwrap();
    assert_eq!(back.measurement, doc.measurement);
    assert_eq!(back.notes, doc.notes);

    let m = read_manifest(&bytes).unwrap();
    let mut v = serde_json::to_value(&m.document).unwrap();
    v.as_object_mut().unwrap().remove("measurement");
    v.as_object_mut().unwrap().remove("notes");
    let old: photocraft_format::manifest::DocM = serde_json::from_value(v).unwrap();
    assert!(old.measurement.is_empty() && old.notes.is_empty());
}

#[test]
fn a_manifest_without_color_samplers_still_loads() {
    let mut doc = Document::new("a", Size::new(8, 8), ColorMode::Rgb, SampleType::U8);
    doc.measurement.color_samplers = vec![[1.5, 1.5]];
    let m = read_manifest(&save_to_bytes(&doc, &SaveOptions::default()).unwrap()).unwrap();
    let mut v = serde_json::to_value(&m.document).unwrap();
    // A file from before the Color Sampler tool: its measurement has no `color_samplers`.
    let measurement = v["measurement"].as_object_mut().unwrap();
    assert!(measurement.remove("color_samplers").is_some(), "{measurement:?}");
    let old: photocraft_format::manifest::DocM = serde_json::from_value(v).unwrap();
    assert!(old.measurement.color_samplers.is_empty());
}
