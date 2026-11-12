//! The plugin's contract: the JSON `manifest()` export, the input schema and
//! the parameter validation. Plain Rust, tested on the host target.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::gear::GearSpec;

/// The plugin id. The folder under `{data}/plugins/`, `plugin.json` on disk and
/// the `manifest()` export all carry it.
pub const PLUGIN_ID: &str = "gear-generator";

/// The `manifest()` export: the whole contract as JSON.
///
/// Field types follow the engine command contracts' field types (`integer`,
/// `length`, `angle`, `string`) so the host can draw the input form from this
/// one declaration (RFC 0002, section 2, "Entry inputs should be schemas").
pub fn manifest_json() -> Value {
    json!({
        "id": PLUGIN_ID,
        "api_version": "1.0.0",
        "name": "Gear generator",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Creates a parametric involute spur gear at the origin: a native sketch and extrude, so the design opens without the plugin.",
        "kind": "command",
        // Phase 1 grants `run`, `log` and `progress`; nothing else is asked for.
        "permissions": [],
        // Every command the plugin emits is a built-in engine command, so a
        // design made by the plugin opens without it (RFC 0002, section 4).
        "fallback": "recorded_script",
        "entries": [
            {
                "id": "gear",
                "label": "Create gear",
                "effect": "edit",
                "doc": "Builds a spur gear centred on the origin on the XY plane, extruded towards +Z.",
                "parameters": [
                    { "name": "teeth", "type": "integer", "min": 6, "max": 200, "default": 20, "doc": "Number of teeth." },
                    { "name": "module", "type": "length", "min": 0.5, "max": 20.0, "default": 2.0, "unit": "mm", "doc": "Module: the pitch diameter is module * teeth." },
                    { "name": "pressure_angle", "type": "angle", "min": 10.0, "max": 30.0, "default": 20.0, "unit": "deg", "doc": "Pressure angle at the pitch circle. The common values are 14.5, 20 and 25." },
                    { "name": "thickness", "type": "length", "min": 1.0, "max": 300.0, "default": 8.0, "unit": "mm", "doc": "Face width: how far the gear is extruded." },
                    { "name": "bore_diameter", "type": "length", "min": 0.0, "max": 400.0, "default": 8.0, "unit": "mm", "doc": "Bore diameter, 0 for a gear without a bore." },
                    { "name": "name", "type": "string", "default": "Gear", "doc": "Name of the sketch, the extrude and the body." }
                ]
            }
        ]
    })
}

/// The entry's input, as the host passes it to `execute`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GearInput {
    #[serde(default = "default_teeth")]
    pub teeth: u32,
    #[serde(default = "default_module")]
    pub module: f64,
    #[serde(rename = "pressure_angle", default = "default_pressure_angle")]
    pub pressure_angle_deg: f64,
    #[serde(default = "default_thickness")]
    pub thickness: f64,
    #[serde(rename = "bore_diameter", default = "default_bore")]
    pub bore_diameter: f64,
    #[serde(default = "default_name")]
    pub name: String,
}

fn default_teeth() -> u32 {
    20
}
fn default_module() -> f64 {
    2.0
}
fn default_pressure_angle() -> f64 {
    20.0
}
fn default_thickness() -> f64 {
    8.0
}
fn default_bore() -> f64 {
    8.0
}
fn default_name() -> String {
    "Gear".to_string()
}

impl GearInput {
    /// Validate the input against the manifest's ranges and turn it into a
    /// [`GearSpec`]. The error text is a complete, readable sentence: it is
    /// what the user sees.
    pub fn validate(&self) -> Result<GearSpec, String> {
        let errors = [
            check("teeth", self.teeth as f64, 6.0, 200.0, |_| false, "number"),
            check("module", self.module, 0.5, 20.0, |_| false, "number"),
            check(
                "pressure_angle",
                self.pressure_angle_deg,
                10.0,
                30.0,
                |_| false,
                "number",
            ),
            check("thickness", self.thickness, 1.0, 300.0, |_| false, "number"),
            check(
                "bore_diameter",
                self.bore_diameter,
                0.0,
                400.0,
                |_| false,
                "number",
            ),
        ];
        if let Some(message) = errors.into_iter().flatten().next() {
            return Err(message);
        }

        let spec = GearSpec {
            teeth: self.teeth,
            module_mm: self.module,
            pressure_angle_deg: self.pressure_angle_deg,
            thickness_mm: self.thickness,
            bore_diameter_mm: self.bore_diameter,
            name: self.name.trim().to_string(),
        };
        if spec.name.is_empty() || spec.name.len() > 64 {
            return Err("the name must be 1 to 64 characters".to_string());
        }
        if !spec
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ' ')
        {
            return Err(
                "the name may hold only letters, digits, spaces, underscores and hyphens"
                    .to_string(),
            );
        }
        // The bore must leave a rim: its radius stays under the root radius
        // minus half a module.
        let rim = spec.root_radius() - spec.module_mm / 2.0;
        if spec.bore_diameter_mm / 2.0 >= rim {
            return Err(format!(
                "the bore diameter {} mm leaves no rim: with {} teeth and module {} mm it must stay under {} mm",
                spec.bore_diameter_mm, spec.teeth, spec.module_mm, rim * 2.0
            ));
        }
        // Every radius must stay in the range the engine commands accept.
        if spec.tip_radius() >= 1.0e9 || spec.root_radius() <= 0.0 {
            return Err("the gear's radii leave the range the engine commands accept".to_string());
        }
        Ok(spec)
    }
}

/// One range check: `None` when the value is fine, `Some(message)` when not.
fn check(
    name: &str,
    value: f64,
    min: f64,
    max: f64,
    extra: fn(f64) -> bool,
    extra_name: &str,
) -> Option<String> {
    if !value.is_finite() {
        return Some(format!("`{name}` must be a finite number"));
    }
    if value < min || value > max {
        return Some(format!("`{name}` must be between {min} and {max}"));
    }
    if extra(value) {
        return Some(format!("`{name}` must be a {extra_name}"));
    }
    None
}
