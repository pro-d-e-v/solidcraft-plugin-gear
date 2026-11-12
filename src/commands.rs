//! Turning a validated gear into the engine commands the host runs.

use serde_json::{json, Value};

use crate::gear::{outline, GearSpec, Piece};

/// Round to 6 decimals: exact to a nanometre, and keeps the JSON small.
fn r6(x: f64) -> f64 {
    (x * 1.0e6).round() / 1.0e6
}

/// The commands the plugin emits, in order. Every one is a built-in engine
/// command, so the finished design opens without the plugin (the recorded
/// script fallback in RFC 0002, section 4).
pub fn build_commands(spec: &GearSpec) -> Vec<Value> {
    let mut commands = Vec::new();
    commands.push(json!({
        "command": "sketch.create",
        "params": { "plane": "XY", "name": spec.name }
    }));

    let pieces = outline(spec);
    for piece in &pieces {
        match piece {
            Piece::Polyline { points } => {
                let points: Vec<[f64; 2]> = points.iter().map(|p| [r6(p[0]), r6(p[1])]).collect();
                commands.push(json!({
                    "command": "sketch.line",
                    "params": { "points": points }
                }));
            }
            Piece::Arc { center, from, to } => {
                commands.push(json!({
                    "command": "sketch.arc.center_point",
                    "params": {
                        "center": [r6(center[0]), r6(center[1])],
                        "start": [r6(from[0]), r6(from[1])],
                        "end": [r6(to[0]), r6(to[1])]
                    }
                }));
            }
        }
    }

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
        "params": { "distance": spec.thickness_mm, "name": spec.name }
    }));
    commands
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
        "tip_diameter_mm": spec.tip_radius() * 2.0,
        "root_diameter_mm": spec.root_radius() * 2.0,
        "volume_estimate_mm3": crate::gear::volume_estimate(spec),
        "body": spec.name,
        "bbox_mm": {
            "min": [-r6(spec.tip_radius()), -r6(spec.tip_radius()), 0.0],
            "max": [r6(spec.tip_radius()), r6(spec.tip_radius()), r6(spec.thickness_mm)]
        }
    })
}
