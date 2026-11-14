//! Turning a validated gear into the engine commands the host runs.

// The no_std wasm build needs `format!` and `ToString` from alloc; on the
// host they come from std's prelude, so the import is allowed to be unused.
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_imports))]
use alloc::{format, string::String, vec, vec::Vec};
use serde_json::{json, Value};

use crate::gear::{outline, GearSpec, KeywaySpec};

/// Round to 6 decimals: exact to a nanometre, and keeps the JSON small.
fn r6(x: f64) -> f64 {
    (x * 1.0e6).round() / 1.0e6
}

/// The commands the plugin emits, in order. Every one is a built-in engine
/// command, so the finished design opens without the plugin (the recorded
/// script fallback in RFC 0002, section 4).
///
/// The whole outline is one closed `sketch.line`, the bore is one circle in the
/// same sketch, and one extrude makes the body: five calls for any tooth count,
/// so a 200-tooth gear costs no more host calls than a 6-tooth one. The extrude
/// names no profiles, so the circle inside the outline is a hole in it.
pub fn build_commands(spec: &GearSpec) -> Result<Vec<Value>, String> {
    let points: Vec<[f64; 2]> = outline(spec)?
        .into_iter()
        .map(|p| [r6(p[0]), r6(p[1])])
        .collect();

    let mut commands = Vec::with_capacity(9);
    commands.push(json!({
        "command": "sketch.create",
        "params": { "plane": "XY", "name": spec.name }
    }));
    commands.push(json!({
        "command": "sketch.line",
        "params": { "points": points, "closed": true, "connect": false }
    }));

    if spec.bore_diameter_mm > 0.0 {
        // A region inside another becomes a hole in it on the extrude, so the
        // bore is just a circle inside the outline.
        commands.push(json!({
            "command": "sketch.circle.center",
            "params": { "center": [0.0, 0.0], "diameter": r6(spec.bore_diameter_mm) }
        }));
    }

    commands.push(json!({ "command": "sketch.finish", "params": {} }));
    commands.push(json!({
        "command": "solid.extrude",
        "params": { "distance": spec.thickness_mm, "name": spec.name, "body_name": spec.name }
    }));

    if let Some(keyway) = &spec.keyway {
        // The keyway slot: a separate sketch on the same plane, then a cut
        // through everything. The slot runs from the bore's centre out past
        // the bore radius, so its visible part is exactly the keyway.
        let slot = keyway_rectangle(spec, keyway);
        commands.push(json!({
            "command": "sketch.create",
            "params": { "plane": "XY", "name": format!("{} keyway", spec.name) }
        }));
        let points: Vec<[f64; 2]> = slot.iter().map(|p| [r6(p[0]), r6(p[1])]).collect();
        commands.push(json!({
            "command": "sketch.line",
            "params": { "points": points, "closed": true, "connect": false }
        }));
        commands.push(json!({ "command": "sketch.finish", "params": {} }));
        // The cut names its target: only the gear body is cut, never the rest
        // of a design the plugin ran into.
        commands.push(json!({
            "command": "solid.extrude",
            "params": { "operation": "cut", "through_all": true, "name": "Keyway", "targets": [spec.name] }
        }));
    }
    Ok(commands)
}

/// The keyway slot rectangle's four corners, counter-clockwise, starting below
/// the centre line. Rotation by the slot's angle keeps the area and the depths.
fn keyway_rectangle(spec: &GearSpec, keyway: &KeywaySpec) -> Vec<[f64; 2]> {
    let r_bore = spec.bore_diameter_mm / 2.0;
    let half_w = keyway.width_mm / 2.0;
    let far = r_bore + keyway.depth_mm;
    let angle = keyway.angle_deg.to_radians();
    let rot = |x: f64, y: f64| -> [f64; 2] {
        let (s, c) = angle.sin_cos();
        [x * c - y * s, x * s + y * c]
    };
    vec![
        rot(0.0, -half_w),
        rot(far, -half_w),
        rot(far, half_w),
        rot(0.0, half_w),
    ]
}

/// The JSON summary `execute` returns after every command ran.
pub fn summary_json(spec: &GearSpec, command_count: usize) -> Value {
    json!({
        "ok": true,
        "plugin": crate::contract::PLUGIN_ID,
        "command": "gear",
        "commands_run": command_count,
        "teeth": spec.teeth,
        "module_mm": spec.module_mm,
        "pressure_angle_deg": spec.pressure_angle_deg,
        "thickness_mm": spec.thickness_mm,
        "bore_diameter_mm": spec.bore_diameter_mm,
        "pitch_diameter_mm": spec.pitch_radius() * 2.0,
        "base_diameter_mm": spec.base_radius() * 2.0,
        "tip_diameter_mm": spec.tip_radius() * 2.0,
        "root_diameter_mm": spec.root_radius() * 2.0,
        "keyway": match &spec.keyway {
            None => serde_json::Value::Null,
            Some(keyway) => json!({
                "width_mm": keyway.width_mm,
                "depth_mm": keyway.depth_mm,
                "angle_deg": keyway.angle_deg
            }),
        },
        "volume_estimate_mm3": crate::gear::volume_estimate(spec),
        "body": spec.name,
        "bbox_mm": {
            "min": [-r6(spec.tip_radius()), -r6(spec.tip_radius()), 0.0],
            "max": [r6(spec.tip_radius()), r6(spec.tip_radius()), r6(spec.thickness_mm)]
        }
    })
}
