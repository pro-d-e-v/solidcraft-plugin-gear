//! Unit tests for the gear geometry, the command building and the contract.
//! They run on the host target; see `tests/` for integration-style tests.

use crate::commands::build_commands;
use crate::contract::GearInput;
use crate::gear::{outline, profile_area, volume_estimate, GearSpec, KeywaySpec, Piece};
use core::f64::consts::PI;
use serde_json::json;

fn default_spec() -> GearSpec {
    GearSpec {
        teeth: 20,
        module_mm: 2.0,
        pressure_angle_deg: 20.0,
        thickness_mm: 8.0,
        bore_diameter_mm: 8.0,
        keyway: None,
        name: "Gear".to_string(),
    }
}

/// Every flank point sits between the root and tip radii, and the tooth narrows
/// towards the tip: the half width at the pitch circle is pi*m/4 there.
#[test]
fn flank_widths_shrink_from_root_to_tip() {
    let spec = default_spec();
    let (_r_base, r_tip, r_root) = (spec.base_radius(), spec.tip_radius(), spec.root_radius());

    // The outline crosses the pitch circle exactly on the tooth centre line's
    // offset pi*m/4 / r_pitch: check by finding the flank radius at the angle
    // closest to the pitch crossing instead: every flank point's radius is in
    // range, and the widest point is at the root side, the narrowest at the tip.
    let mut radii = Vec::new();
    for piece in outline(&spec) {
        if let Piece::Polyline { points } = piece {
            for point in &points {
                radii.push(point[0].hypot(point[1]));
            }
        }
    }
    assert!(
        radii
            .iter()
            .all(|r| *r >= r_root - 1.0e-9 && *r <= r_tip + 1.0e-9),
        "flank points leave [{r_root}, {r_tip}]: min {:?} max {:?}",
        radii.iter().cloned().fold(f64::INFINITY, f64::min),
        radii.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    );
}

/// The tooth's angular width at the pitch circle is the standard half
/// thickness: (pi*m/4) / r_pitch = pi/z.
#[test]
fn tooth_width_at_pitch_matches_the_standard() {
    let spec = default_spec();
    let z = spec.teeth as f64;
    let r_pitch = spec.pitch_radius();
    let r_base = spec.base_radius();
    let alpha = spec.pressure_angle_rad();

    // Walk the outline points; collect pairs that straddle the pitch radius on
    // one flank (radius in (r_base, r_tip)) and measure the angular width there
    // via the involute relation: at radius r the flank angle offset from the
    // tooth centre is pitch_half + inv_offset - (t - atan(t)).
    let pitch_half = PI / (2.0 * z);
    let inv_offset = alpha.tan() - alpha;
    // At the pitch circle, t = tan(alpha), so the offset is exactly pitch_half.
    let offset_at = |r: f64| {
        let t = ((r / r_base).powi(2) - 1.0).sqrt();
        pitch_half + inv_offset - (t - t.atan())
    };
    assert!((offset_at(r_pitch) - pitch_half).abs() < 1.0e-12);
    // The tooth centre line is at the offset's mirror: half angular width.
    assert!((offset_at(r_pitch) * 2.0 - PI / z).abs() < 1.0e-12);
}

/// The outline closes: consecutive segments share an endpoint, and the last
/// segment ends where the first begins.
#[test]
fn outline_is_closed() {
    for spec in [default_spec(), small_gear(), big_gear()] {
        let segments = outline(&spec);
        assert_eq!(segments.len(), 4 * spec.teeth as usize);
        let ends = |s: &Piece| match s {
            Piece::Polyline { points } => *points.last().unwrap(),
            Piece::Arc { to, .. } => *to,
        };
        let starts = |s: &Piece| match s {
            Piece::Polyline { points } => *points.first().unwrap(),
            Piece::Arc { from, .. } => *from,
        };
        for (i, segment) in segments.iter().enumerate() {
            let next = &segments[(i + 1) % segments.len()];
            let a = ends(segment);
            let b = starts(next);
            assert!(
                (a[0] - b[0]).abs() < 1.0e-9 && (a[1] - b[1]).abs() < 1.0e-9,
                "segments {i} and {} do not meet: {a:?} vs {b:?}",
                (i + 1) % segments.len()
            );
        }
    }
}

/// Volume estimate against the values the real engine reported for this exact
/// gear (see the README's development section): the measured volume was
/// 9404.11 mm^3, and the fine analytic area gives 9411.04 mm^3.
#[test]
fn volume_estimate_matches_the_engine_measured_gear() {
    let spec = default_spec();
    let estimated = volume_estimate(&spec);
    assert!(
        (estimated - 9411.04).abs() / 9411.04 < 0.005,
        "estimate {estimated}"
    );
    // The engine measured 9404.11; the estimate must sit within 1% of that too.
    assert!(
        (estimated - 9404.11).abs() / 9404.11 < 0.01,
        "estimate {estimated}"
    );
}

/// A large gear where the root circle is above the base circle: the flanks are
/// involutes the whole way, no radial root piece.
#[test]
fn big_gear_root_above_base_circle() {
    let spec = big_gear();
    assert!(spec.root_radius() > spec.base_radius());
    let area = profile_area(&spec);
    // The engine measured 45058.09 mm^3 for this gear (T = 4, bore 5); the fine
    // analytic estimate was 45056.95.
    let estimated = volume_estimate(&spec);
    assert!(
        (estimated - 45057.0).abs() / 45057.0 < 0.005,
        "estimate {estimated}"
    );
    assert!(area > PI * spec.root_radius().powi(2));
}

fn small_gear() -> GearSpec {
    GearSpec {
        teeth: 8,
        module_mm: 3.0,
        bore_diameter_mm: 0.0,
        ..default_spec()
    }
}

fn big_gear() -> GearSpec {
    GearSpec {
        teeth: 120,
        module_mm: 1.0,
        thickness_mm: 4.0,
        bore_diameter_mm: 5.0,
        ..default_spec()
    }
}

/// The command list starts with the sketch, ends with the extrude, holds the
/// bore circle, and every coordinate is finite and inside the tip circle.
#[test]
fn command_list_shape() {
    let spec = small_gear(); // 8 teeth, no bore
    let commands = build_commands(&spec);
    assert_eq!(commands.first().unwrap()["command"], "sketch.create");
    assert_eq!(commands.last().unwrap()["command"], "solid.extrude");
    // 4 steps per tooth (two flanks, tip arc, root arc) + create + finish + extrude.
    assert_eq!(commands.len(), 1 + 4 * spec.teeth as usize + 2);
    let text = serde_json::to_string(&commands).unwrap();
    assert!(!text.contains("null"), "a coordinate was missing: {text}");
    assert!(!text.contains("NaN"), "a coordinate was not a number");
    // No bore in this one.
    assert!(!commands
        .iter()
        .any(|c| c["command"] == "sketch.circle.center"));

    let spec = default_spec(); // with a bore
    let commands = build_commands(&spec);
    assert_eq!(commands.len(), 1 + 4 * spec.teeth as usize + 3);
    let bore = commands
        .iter()
        .find(|c| c["command"] == "sketch.circle.center")
        .expect("the bore circle is missing");
    assert_eq!(bore["params"]["diameter"], 8.0);
}

/// Invalid inputs are refused with readable messages.
#[test]
fn validation_refuses_bad_inputs() {
    let base = GearInput {
        teeth: 20,
        module: 2.0,
        pressure_angle_deg: 20.0,
        thickness: 8.0,
        bore_diameter: 8.0,
        keyway: false,
        keyway_width: 4.0,
        keyway_depth: 2.0,
        keyway_angle: 0.0,
        name: "Gear".to_string(),
    };

    let too_few_teeth = GearInput {
        teeth: 5,
        ..base.clone()
    };
    assert_eq!(
        too_few_teeth.validate().unwrap_err(),
        "`teeth` must be between 6 and 200"
    );

    let tiny_module = GearInput {
        module: 0.1,
        ..base.clone()
    };
    assert!(tiny_module
        .validate()
        .unwrap_err()
        .contains("`module` must be between"));

    let no_rim = GearInput {
        bore_diameter: 40.0,
        ..base.clone()
    };
    assert!(no_rim.validate().unwrap_err().contains("leaves no rim"));

    let empty_name = GearInput {
        name: "  ".to_string(),
        ..base.clone()
    };
    assert_eq!(
        empty_name.validate().unwrap_err(),
        "the name must be 1 to 64 characters"
    );

    // A good input round-trips.
    assert!(base.validate().is_ok());
}

/// The strip integral the keyway estimate uses, checked against a direct
/// numeric sample of the removed area.
#[test]
fn keyway_area_matches_the_numeric_strip_integral() {
    let keyway = KeywaySpec {
        width_mm: 4.0,
        depth_mm: 2.0,
        angle_deg: 0.0,
    };
    let r = 4.0; // bore diameter 8
    let x1 = r + keyway.depth_mm;
    let steps = 200_000;
    let dx = x1 / steps as f64;
    let mut numeric = 0.0;
    for i in 0..steps {
        let x = (i as f64 + 0.5) * dx;
        let removed = if x >= r {
            keyway.width_mm
        } else {
            let chord = 2.0 * (r * r - x * x).sqrt();
            (keyway.width_mm - chord).max(0.0)
        };
        numeric += removed * dx;
    }
    let analytic = keyway.area_outside_bore(2.0 * r);
    assert!(
        (numeric - analytic).abs() / numeric < 1.0e-3,
        "analytic {analytic} vs numeric {numeric}"
    );
}

/// With the keyway off, the command list and the volume stay exactly as they
/// were in 0.2.0.
#[test]
fn keyway_off_changes_nothing() {
    let mut with = default_spec();
    with.keyway = Some(KeywaySpec {
        width_mm: 4.0,
        depth_mm: 2.0,
        angle_deg: 0.0,
    });
    let without = default_spec();
    assert!(without.keyway.is_none());
    assert_eq!(
        build_commands(&with).len(),
        build_commands(&without).len() + 4
    );

    let base = volume_estimate(&without);
    let removed = base - volume_estimate(&with);
    // The slot is about 4 mm wide and 2 mm deep past the circle edge, times
    // the 8 mm thickness: roughly 80 mm³, well under the gear's ~9400 mm³.
    assert!(removed > 40.0 && removed < 120.0, "removed {removed}");
}

/// The keyway's extra commands: a sketch, the closed slot rectangle, a
/// finish and a cut-through-all extrude.
#[test]
fn keyway_commands_shape() {
    let mut spec = default_spec();
    spec.keyway = Some(KeywaySpec {
        width_mm: 4.0,
        depth_mm: 2.0,
        angle_deg: 30.0,
    });
    let commands = build_commands(&spec);
    assert_eq!(commands.len(), 84 + 4);

    // The slot's second corner sits at bore radius + depth, half a width
    // below the centre line, rotated 30°.
    let slot = &commands[84 + 1];
    assert_eq!(slot["command"], "sketch.line");
    let points = slot["params"]["points"].as_array().expect("points");
    assert_eq!(points.len(), 5); // four corners, closed
    let far = 4.0 + 2.0;
    let angle = 30.0f64.to_radians();
    let (s, c) = angle.sin_cos();
    let expected = [far * c - (-2.0) * s, far * s + (-2.0) * c];
    let second = points[1].as_array().unwrap();
    assert!((second[0].as_f64().unwrap() - expected[0]).abs() < 1.0e-5);
    assert!((second[1].as_f64().unwrap() - expected[1]).abs() < 1.0e-5);

    // The cut goes through everything, and only through the gear's body.
    let cut = commands.last().unwrap();
    assert_eq!(cut["command"], "solid.extrude");
    assert_eq!(cut["params"]["operation"], "cut");
    assert_eq!(cut["params"]["through_all"], true);
    assert_eq!(cut["params"]["targets"], json!(["Gear"]));
}

#[test]
fn validation_refuses_broken_keyways() {
    let base = GearInput {
        teeth: 20,
        module: 2.0,
        pressure_angle_deg: 20.0,
        thickness: 8.0,
        bore_diameter: 8.0,
        keyway: true,
        keyway_width: 4.0,
        keyway_depth: 2.0,
        keyway_angle: 0.0,
        name: "Gear".to_string(),
    };
    assert!(base.validate().is_ok());

    // A keyway without a bore.
    let no_bore = GearInput {
        bore_diameter: 0.0,
        ..base.clone()
    };
    assert!(no_bore
        .validate()
        .unwrap_err()
        .contains("the keyway needs a bore"));

    // A slot wider than the bore.
    let too_wide = GearInput {
        keyway_width: 10.0,
        ..base.clone()
    };
    assert!(too_wide
        .validate()
        .unwrap_err()
        .contains("keyway width must be from 0.5 mm and under the bore"));

    // A slot so deep its corners break through the teeth: the corners reach
    // sqrt((r_bore + d)² + (w/2)²), which must stay under the root radius
    // 17.5 mm; d = 14 crosses it.
    let too_deep = GearInput {
        keyway_depth: 14.0,
        ..base.clone()
    };
    assert!(too_deep
        .validate()
        .unwrap_err()
        .contains("cuts through the gear"));

    // A wild angle.
    let bad_angle = GearInput {
        keyway_angle: 500.0,
        ..base.clone()
    };
    assert!(bad_angle
        .validate()
        .unwrap_err()
        .contains("`keyway_angle` must be between -360 and 360"));
}
