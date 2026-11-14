//! Involute spur gear geometry.
//!
//! Everything here is plain mathematics with no host access, so it is unit
//! tested on the host target. The outline is one closed counter-clockwise
//! polyline: approximated involute flanks, radial root flanks when the root
//! circle sits below the base circle, and short chords along the tip and root
//! circles.
//!
//! The whole outline goes to the engine as one `sketch.line`, so the host call
//! count does not grow with the tooth count. The number of chords per tooth
//! falls as the tooth count rises: the host's sketch work grows faster than
//! linearly with the point count, and a fine 200-tooth outline would pass the
//! plugin's deadline. Small gears keep the fine profile; on a many-toothed
//! gear the teeth are small anyway.

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::f64::consts::PI;

/// A validated gear, ready to be turned into engine commands.
#[derive(Clone, Debug, PartialEq)]
pub struct GearSpec {
    /// Number of teeth.
    pub teeth: u32,
    /// Module in mm (pitch diameter = module * teeth).
    pub module_mm: f64,
    /// Pressure angle in degrees at the pitch circle.
    pub pressure_angle_deg: f64,
    /// Face width (extrude distance) in mm.
    pub thickness_mm: f64,
    /// Bore diameter in mm; 0 means no bore.
    pub bore_diameter_mm: f64,
    /// The keyway slot, when asked for.
    pub keyway: Option<KeywaySpec>,
    /// Name of the sketch, the extrude and the body.
    pub name: String,
}

/// A keyway slot in the bore: a rectangle from the bore's centre out to
/// `depth_mm` past the bore radius, `width_mm` wide, pointing at
/// `angle_deg` (0 is +X). Cut through the whole face width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeywaySpec {
    /// The slot's width in mm.
    pub width_mm: f64,
    /// How far past the bore radius the slot reaches, in mm.
    pub depth_mm: f64,
    /// Where the slot points, in degrees; 0 is +X.
    pub angle_deg: f64,
}

impl KeywaySpec {
    /// The area the slot cuts beyond the bore circle: the part of the slot
    /// rectangle that lies outside the bore. Inside the bore there is nothing
    /// to cut, so only this matters to the volume.
    ///
    /// The slot spans `0 ≤ x ≤ r_bore + d` with `|y| ≤ w/2` (rotating it keeps
    /// the area). Over `[0, x_c]` with `x_c = sqrt(r² − (w/2)²)` the rectangle
    /// stays inside the circle and cuts nothing; past `x_c` up to the circle's
    /// edge the removed strip is `w − 2·sqrt(r² − x²)` wide; past `r` it is
    /// `w`. The strip integral has a closed form through
    /// `∫ sqrt(r² − x²) dx = x·sqrt(r² − x²)/2 + r²·asin(x/r)/2`.
    pub fn area_outside_bore(&self, bore_diameter_mm: f64) -> f64 {
        let r = bore_diameter_mm / 2.0;
        let half_w = self.width_mm / 2.0;
        if half_w >= r {
            // The slot is wider than the bore: everything from the centre out
            // is material once past the circle. Not a valid keyway, so this
            // only keeps the estimate honest if one slips through.
            return self.width_mm * (r + self.depth_mm);
        }
        let x_c = (r * r - half_w * half_w).sqrt();
        let f = |x: f64| x * (r * r - x * x).sqrt() / 2.0 + r * r * (x / r).asin() / 2.0;
        // From x_c to r: w·(r − x_c) minus twice the circle area the strip
        // covers; from r to r + d: the plain w·d rectangle.
        self.width_mm * (r - x_c) - 2.0 * (f(r) - f(x_c)) + self.width_mm * self.depth_mm
    }
}

impl GearSpec {
    /// Pitch radius: r = m * z / 2.
    pub fn pitch_radius(&self) -> f64 {
        self.module_mm * self.teeth as f64 / 2.0
    }

    /// Base radius: r_b = r * cos(alpha), where the involute starts.
    pub fn base_radius(&self) -> f64 {
        self.pitch_radius() * self.pressure_angle_rad().cos()
    }

    /// Tip (outside) radius: r_a = r + m.
    pub fn tip_radius(&self) -> f64 {
        self.pitch_radius() + self.module_mm
    }

    /// Root radius with the standard 1.25 m dedendum: r_f = r - 1.25 m.
    pub fn root_radius(&self) -> f64 {
        self.pitch_radius() - 1.25 * self.module_mm
    }

    /// Pressure angle in radians.
    pub fn pressure_angle_rad(&self) -> f64 {
        self.pressure_angle_deg.to_radians()
    }
}

fn inv(a: f64) -> f64 {
    a.tan() - a
}

fn polar(radius: f64, angle: f64) -> [f64; 2] {
    [radius * angle.cos(), radius * angle.sin()]
}

/// The tooth flank shape, in radii and angles.
struct Shape {
    base: f64,
    tip: f64,
    root: f64,
    /// Half the angular width of a tooth at the pitch circle, plus the
    /// involute function of the pressure angle: the flank's angular offset at
    /// the base circle.
    half_pitch: f64,
    inv_alpha: f64,
}

impl Shape {
    fn new(spec: &GearSpec) -> Self {
        let z = spec.teeth as f64;
        let alpha = spec.pressure_angle_rad();
        Shape {
            base: spec.base_radius(),
            tip: spec.tip_radius(),
            root: spec.root_radius(),
            half_pitch: PI / (2.0 * z),
            inv_alpha: inv(alpha),
        }
    }

    /// Half the angular thickness of a tooth at radius `rho`; radial below the
    /// base circle, where the true involute does not exist.
    fn psi(&self, rho: f64) -> f64 {
        if rho <= self.base {
            self.half_pitch + self.inv_alpha
        } else {
            let a = (self.base / rho).min(1.0).acos();
            self.half_pitch + self.inv_alpha - inv(a)
        }
    }
}

/// Chords per tooth: (flank segments, tip segments, root segments).
fn segments_per_tooth(teeth: u32) -> (usize, usize, usize) {
    if teeth <= 90 {
        (10, 4, 3) // 30 points per tooth
    } else if teeth <= 150 {
        (7, 2, 2) // 20 points per tooth
    } else {
        (5, 2, 2) // 16 points per tooth
    }
}

/// The closed outline of the whole gear, counter-clockwise, starting in the
/// first root gap.
///
/// Per tooth: a root arc up to the right flank, the right flank, the tip arc,
/// the left flank and a root arc towards the next gap. Consecutive teeth share
/// their gap-centre point, so the engine's `closed: true` joins the last point
/// back to the first without a duplicate.
pub fn outline(spec: &GearSpec) -> Result<Vec<[f64; 2]>, String> {
    let shape = Shape::new(spec);
    if shape.root <= 0.0 {
        return Err("the gear's root radius must be positive".to_string());
    }
    let z = spec.teeth as f64;
    let h = PI / z;
    let psi_root = shape.psi(shape.root);
    let psi_tip = shape.psi(shape.tip);
    if !(psi_tip.is_finite() && psi_tip > 0.0) {
        return Err("the tooth comes to a point; reduce the module or tooth count".to_string());
    }
    if psi_root >= h * 0.98 {
        return Err("the teeth overlap at the root; reduce the tooth count".to_string());
    }
    if psi_tip * shape.tip < 0.01 * spec.module_mm {
        return Err("the teeth are pointed at the tip; reduce the tooth count".to_string());
    }

    let (flank, tip, root) = segments_per_tooth(spec.teeth);
    let per_tooth = root + (flank + 1) + (tip - 1) + (flank + 1) + (root - 1);
    let mut points = Vec::with_capacity(spec.teeth as usize * per_tooth);

    for i in 0..spec.teeth {
        let centre = i as f64 * 2.0 * h;
        // Root arc from the gap centre up to the right flank's root.
        for k in 0..root {
            let t = k as f64 / root as f64;
            let angle = -h + (h - psi_root) * t;
            points.push(polar(shape.root, centre + angle));
        }
        // Right flank, root to tip.
        for k in 0..=flank {
            let rho = shape.root + (shape.tip - shape.root) * k as f64 / flank as f64;
            points.push(polar(rho, centre - shape.psi(rho)));
        }
        // Tip arc, without its end points (they are the flank ends).
        for k in 1..tip {
            let angle = -psi_tip + 2.0 * psi_tip * k as f64 / tip as f64;
            points.push(polar(shape.tip, centre + angle));
        }
        // Left flank, tip back to the root.
        for k in (0..=flank).rev() {
            let rho = shape.root + (shape.tip - shape.root) * k as f64 / flank as f64;
            points.push(polar(rho, centre + shape.psi(rho)));
        }
        // Root arc towards the next gap centre; its end point is the next
        // tooth's first point.
        for k in 1..root {
            let t = k as f64 / root as f64;
            let angle = psi_root + (h - psi_root) * t;
            points.push(polar(shape.root, centre + angle));
        }
    }
    Ok(points)
}

/// The area the outline encloses, by the shoelace formula.
pub fn profile_area(spec: &GearSpec) -> f64 {
    let Ok(points) = outline(spec) else {
        return 0.0;
    };
    let mut twice_area = 0.0;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        twice_area += a[0] * b[1] - b[0] * a[1];
    }
    (twice_area / 2.0).abs()
}

/// The extruded volume the plugin's commands should produce: the profile minus
/// the bore, times the face width, minus what the keyway slot cuts. An
/// estimate: the flanks are polylines.
pub fn volume_estimate(spec: &GearSpec) -> f64 {
    let bore_area = if spec.bore_diameter_mm > 0.0 {
        PI * (spec.bore_diameter_mm / 2.0).powi(2)
    } else {
        0.0
    };
    let keyway = spec
        .keyway
        .map(|k| k.area_outside_bore(spec.bore_diameter_mm))
        .unwrap_or(0.0);
    (profile_area(spec) - bore_area - keyway) * spec.thickness_mm
}
