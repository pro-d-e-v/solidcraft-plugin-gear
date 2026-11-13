//! The plugin's input schema lives in the static `plugin.json` next to the
//! component; this module keeps the parameter validation that schema's fields
//! feed. Plain Rust, tested on the host target.

// The no_std wasm build needs `format!` and `ToString` from alloc; on the
// host they come from std's prelude, so the import is allowed to be unused.
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_imports))]
use alloc::{format, string::String, string::ToString};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::gear::{GearSpec, KeywaySpec};

/// The plugin id. The folder under `{data}/plugins/` and `plugin.json` carry it.
pub const PLUGIN_ID: &str = "gear-generator";

/// The one entry `plugin.json` declares; the host calls `execute` with its name.
pub const ENTRY_NAME: &str = "gear";

/// The entry's input schema, as `plugin.json` declares it: the same fields,
/// ranges, labels and defaults [`GearInput`] validates. Kept here so a test
/// can check the static manifest against the validator.
pub fn input_schema_json() -> Value {
    json!({
        "type": "object",
        "required": ["teeth", "module", "pressure_angle", "thickness", "bore_diameter", "keyway", "keyway_width", "keyway_depth", "keyway_angle", "name"],
        "properties": {
            "teeth": { "type": "integer", "minimum": 6, "maximum": 200, "default": 20, "title": "Число зубьев", "description": "Сколько зубьев на шестерне." },
            "module": { "type": "number", "minimum": 0.5, "maximum": 20, "default": 2, "unit": "мм", "title": "Модуль", "description": "Модуль: делительный диаметр равен модуль × число зубьев." },
            "pressure_angle": { "type": "number", "minimum": 10, "maximum": 30, "default": 20, "unit": "°", "title": "Угол зацепления", "description": "Угол зацепления на делительной окружности. Обычные значения: 14.5, 20 и 25." },
            "thickness": { "type": "number", "minimum": 1, "maximum": 300, "default": 8, "unit": "мм", "title": "Толщина венца", "description": "Ширина венца: на сколько выдавливается шестерня." },
            "bore_diameter": { "type": "number", "minimum": 0, "maximum": 400, "default": 8, "unit": "мм", "title": "Диаметр отверстия", "description": "Диаметр центрального отверстия, 0 — без отверстия." },
            "keyway": { "type": "boolean", "default": false, "title": "Шпоночный паз", "description": "Прорезать паз под шпонку в отверстии (нужно отверстие)." },
            "keyway_width": { "type": "number", "minimum": 0.5, "maximum": 50, "default": 4, "unit": "мм", "title": "Ширина шпонки", "description": "Ширина паза; должна быть меньше диаметра отверстия." },
            "keyway_depth": { "type": "number", "minimum": 0.5, "maximum": 100, "default": 2, "unit": "мм", "title": "Глубина паза", "description": "Насколько паз уходит в материал от края отверстия; не должен прорезать зубья." },
            "keyway_angle": { "type": "number", "minimum": -360, "maximum": 360, "default": 0, "unit": "°", "title": "Положение паза", "description": "Угол, куда смотрит паз (0 — вдоль +X)." },
            "name": { "type": "string", "default": "Gear", "minLength": 1, "maxLength": 64, "title": "Имя", "description": "Имя эскиза, выдавливания и тела." }
        }
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
    #[serde(default)]
    pub keyway: bool,
    #[serde(default = "default_keyway_width")]
    pub keyway_width: f64,
    #[serde(default = "default_keyway_depth")]
    pub keyway_depth: f64,
    #[serde(default = "default_keyway_angle")]
    pub keyway_angle: f64,
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
fn default_keyway_width() -> f64 {
    4.0
}
fn default_keyway_depth() -> f64 {
    2.0
}
fn default_keyway_angle() -> f64 {
    0.0
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
            keyway: if self.keyway {
                Some(KeywaySpec {
                    width_mm: self.keyway_width,
                    depth_mm: self.keyway_depth,
                    angle_deg: self.keyway_angle,
                })
            } else {
                None
            },
            name: self.name.trim().to_string(),
        };
        if let Some(keyway) = &spec.keyway {
            // A keyway needs a bore to sit in, must be narrower than it, and
            // must not punch through the teeth: the slot's far corners stay
            // under the root radius.
            if spec.bore_diameter_mm <= 0.0 {
                return Err(
                    "the keyway needs a bore: set a bore diameter or turn the keyway off"
                        .to_string(),
                );
            }
            if keyway.width_mm.is_finite()
                && (keyway.width_mm < 0.5 || keyway.width_mm >= spec.bore_diameter_mm)
            {
                return Err(format!(
                    "the keyway width must be from 0.5 mm and under the bore diameter {} mm",
                    spec.bore_diameter_mm
                ));
            }
            let r_bore = spec.bore_diameter_mm / 2.0;
            let corner = (r_bore + keyway.depth_mm).hypot(keyway.width_mm / 2.0);
            if keyway.depth_mm.is_finite()
                && (keyway.depth_mm < 0.5
                    || keyway.depth_mm >= r_bore
                    || corner >= spec.root_radius())
            {
                return Err(format!(
                    "the keyway depth {} mm cuts through the gear: it must stay under the bore radius {} mm and keep the slot's corners under the root radius {} mm",
                    keyway.depth_mm,
                    r_bore,
                    spec.root_radius()
                ));
            }
            if !keyway.angle_deg.is_finite() || !(-360.0..=360.0).contains(&keyway.angle_deg) {
                return Err("`keyway_angle` must be between -360 and 360 degrees".to_string());
            }
        }
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
