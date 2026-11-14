//! Unit tests for the gear geometry, the command building and the contract.
//! They run on the host target; see `tests/` for integration-style tests.

use crate::commands::build_commands;
use crate::contract::GearInput;
use crate::gear::{outline, volume_estimate, GearSpec, KeywaySpec};
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

/// The standard radii: pitch m·z/2, tip +m, root −1.25m.
#[test]
fn diameters_are_standard() {
    let spec = default_spec();
    assert!((spec.pitch_radius() - 20.0).abs() < 1.0e-12);
    assert!((spec.tip_radius() - 22.0).abs() < 1.0e-12);
    assert!((spec.root_radius() - 17.5).abs() < 1.0e-12);
    assert!((spec.base_radius() - 20.0 * 20.0f64.to_radians().cos()).abs() < 1.0e-12);
}

/// Every outline point sits between the root and tip radii, and the point
/// count follows the adaptive schedule: 30 per tooth up to 90 teeth, 20 up to
/// 150 and 16 above that.
#[test]
fn outline_stays_between_root_and_tip() {
    for (teeth, per_tooth) in [
        (6u32, 30usize),
        (20, 30),
        (90, 30),
        (91, 20),
        (150, 20),
        (151, 16),
        (200, 16),
    ] {
        let spec = GearSpec {
            teeth,
            ..default_spec()
        };
        let points = outline(&spec).unwrap();
        assert_eq!(points.len(), teeth as usize * per_tooth);
        assert!(points.len() <= 10_000, "z={teeth}: {} points", points.len());
        for point in &points {
            let radius = point[0].hypot(point[1]);
            assert!(
                radius >= spec.root_radius() - 1.0e-9 && radius <= spec.tip_radius() + 1.0e-9,
                "z={teeth}: radius {radius}"
            );
            assert!(point[0].is_finite() && point[1].is_finite());
        }
    }
}

/// The whole outline is one closed loop, so the command list does not grow
/// with the tooth count.
#[test]
fn one_line_command_holds_the_whole_outline() {
    for teeth in [6u32, 20, 100, 200] {
        let spec = GearSpec {
            teeth,
            ..default_spec()
        };
        let commands = build_commands(&spec).unwrap();
        assert_eq!(commands.len(), 5, "z={teeth}");
        assert_eq!(commands[1]["command"], "sketch.line");
        assert_eq!(commands[1]["params"]["closed"], true);
        assert_eq!(commands[1]["params"]["connect"], false);
        let points = commands[1]["params"]["points"].as_array().unwrap();
        assert_eq!(points.len(), outline(&spec).unwrap().len());
        assert!(points.len() <= 10_000);
    }
}

/// Volume estimate against the values the real engine reported for this exact
/// gear: 9403 mm^3 for 20 teeth, module 2, thickness 8 and an 8 mm bore.
#[test]
fn volume_estimate_matches_the_engine_measured_gear() {
    let estimated = volume_estimate(&default_spec());
    assert!(
        (estimated - 9403.0).abs() / 9403.0 < 0.01,
        "estimate {estimated}"
    );
}

/// A large gear where the root circle is above the base circle and the flanks
/// are involutes the whole way. The engine measured 45058.09 mm^3 for this
/// gear; the polyline estimate stays within a per cent.
#[test]
fn volume_estimate_matches_the_engine_measured_big_gear() {
    let spec = big_gear();
    assert!(spec.root_radius() > spec.base_radius());
    let estimated = volume_estimate(&spec);
    assert!(
        (estimated - 45058.09).abs() / 45058.09 < 0.01,
        "estimate {estimated}"
    );
}

/// The widest gear the schema allows: 200 teeth cost one line command and the
/// estimate matches the engine's 1,003,594 mm^3.
#[test]
fn volume_estimate_matches_the_engine_measured_200_tooth_gear() {
    let spec = GearSpec {
        teeth: 200,
        ..default_spec()
    };
    let estimated = volume_estimate(&spec);
    assert!(
        (estimated - 1_003_594.0).abs() / 1_003_594.0 < 0.01,
        "estimate {estimated}"
    );
}

/// The command list starts with the sketch and ends with the extrude; the
/// line carries the bore, and the extrude names no profiles, so the circle
/// inside the outline is its hole.
#[test]
fn command_list_shape() {
    let spec = default_spec(); // with a bore
    let commands = build_commands(&spec).unwrap();
    assert_eq!(commands.len(), 5);
    assert_eq!(commands[0]["command"], "sketch.create");
    assert_eq!(commands[1]["command"], "sketch.line");
    assert_eq!(commands[2]["command"], "sketch.circle.center");
    assert_eq!(commands[2]["params"]["diameter"], 8.0);
    assert_eq!(commands[3]["command"], "sketch.finish");
    let extrude = &commands[4];
    assert_eq!(extrude["command"], "solid.extrude");
    assert!(
        extrude["params"].get("profiles").is_none(),
        "`profiles` must stay out so the bore is a hole: {}",
        extrude["params"]
    );

    let no_bore = GearSpec {
        bore_diameter_mm: 0.0,
        ..default_spec()
    };
    let commands = build_commands(&no_bore).unwrap();
    assert_eq!(commands.len(), 4);
    assert!(!commands
        .iter()
        .any(|c| c["command"] == "sketch.circle.center"));

    let text = serde_json::to_string(&commands).unwrap();
    assert!(!text.contains("null"), "a coordinate was missing: {text}");
    assert!(!text.contains("NaN"), "a coordinate was not a number");
}

/// The keyway's extra commands: a sketch, the closed slot rectangle, a finish
/// and a cut-through-all extrude naming only the gear body.
#[test]
fn keyway_commands_shape() {
    let mut spec = default_spec();
    spec.keyway = Some(KeywaySpec {
        width_mm: 4.0,
        depth_mm: 2.0,
        angle_deg: 30.0,
    });
    let commands = build_commands(&spec).unwrap();
    assert_eq!(commands.len(), 9);

    assert_eq!(commands[5]["command"], "sketch.create");
    let slot = &commands[6];
    assert_eq!(slot["command"], "sketch.line");
    assert_eq!(slot["params"]["closed"], true);
    let points = slot["params"]["points"].as_array().expect("points");
    assert_eq!(points.len(), 4);

    // The second corner sits at bore radius + depth, half a width below the
    // centre line, rotated 30°.
    let far = 4.0 + 2.0;
    let angle = 30.0f64.to_radians();
    let (s, c) = angle.sin_cos();
    let expected = [far * c - (-2.0) * s, far * s + (-2.0) * c];
    let second = points[1].as_array().unwrap();
    assert!((second[0].as_f64().unwrap() - expected[0]).abs() < 1.0e-5);
    assert!((second[1].as_f64().unwrap() - expected[1]).abs() < 1.0e-5);

    assert_eq!(commands[7]["command"], "sketch.finish");
    let cut = &commands[8];
    assert_eq!(cut["command"], "solid.extrude");
    assert_eq!(cut["params"]["operation"], "cut");
    assert_eq!(cut["params"]["through_all"], true);
    assert_eq!(cut["params"]["targets"], json!(["Gear"]));
    assert!(cut["params"].get("profiles").is_none());
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

    // A good input round-trips; Cyrillic names are allowed.
    assert!(base.validate().is_ok());
    let cyrillic = GearInput {
        name: "Шестерня 1".to_string(),
        ..base
    };
    assert_eq!(cyrillic.validate().unwrap().name, "Шестерня 1");
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

/// With the keyway on, the volume drops by the slot outside the bore; the
/// slot is about 4 mm wide and 2 mm deep past the circle edge, times the
/// 8 mm thickness: roughly 80 mm³, well under the gear's ~9400 mm³.
#[test]
fn keyway_removes_the_slot_volume() {
    let mut with = default_spec();
    with.keyway = Some(KeywaySpec {
        width_mm: 4.0,
        depth_mm: 2.0,
        angle_deg: 0.0,
    });
    let without = default_spec();
    let removed = volume_estimate(&without) - volume_estimate(&with);
    assert!(removed > 40.0 && removed < 120.0, "removed {removed}");
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
        ..base
    };
    assert!(bad_angle.validate().unwrap_err().contains("keyway_angle"));
}

/// The default gear's outline area is the pitch disc plus the teeth' extra
/// area minus the gaps: it is larger than the root disc and smaller than the
/// tip disc.
#[test]
fn profile_area_lies_between_the_root_and_tip_discs() {
    let spec = default_spec();
    let area = crate::gear::profile_area(&spec);
    assert!(area > PI * spec.root_radius().powi(2));
    assert!(area < PI * spec.tip_radius().powi(2));
}
