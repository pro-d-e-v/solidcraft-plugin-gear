//! Involute spur gear geometry.
//!
//! Everything here is plain mathematics with no host access, so it is unit
//! tested on the host target. The outline is a closed counter-clockwise loop
//! of line segments (approximated involute flanks, radial root flanks when the
//! root circle sits below the base circle) and arcs (tooth tips and roots on
//! the tip and root circles, both centred on the gear axis).

use alloc::{string::String, vec::Vec};
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
    /// Name of the sketch, the extrude and the body.
    pub name: String,
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

/// One piece of the gear outline. Arcs are centred on the gear axis.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    /// A straight-sided piece: the polyline runs through `points` in order.
    Polyline { points: Vec<[f64; 2]> },
    /// An arc about `center`, counter-clockwise from `from` to `to`
    /// (the radius is `|from - center|`; `to` gives only the direction).
    Arc {
        center: [f64; 2],
        from: [f64; 2],
        to: [f64; 2],
    },
}

/// How many straight pieces approximate one involute flank. Ten keeps the
/// profile area within about 0.1 % of the true involute area, which is well
/// under the tolerance a solid modeller reports anyway.
const FLANK_SEGMENTS: usize = 12;

/// The closed gear outline, counter-clockwise, tooth 0 centred on +X.
///
/// Per tooth, in order: the root arc (from the previous tooth's right flank
/// root to this tooth's left flank root), the left flank polyline up to the
/// tip, the tip arc, and the right flank polyline back down to the root.
pub fn outline(spec: &GearSpec) -> Vec<Piece> {
    let z = spec.teeth;
    let alpha = spec.pressure_angle_rad();
    let r_base = spec.base_radius();
    let r_tip = spec.tip_radius();
    let r_root = spec.root_radius();

    // Polar angle of the involute point at radius r: tan(alpha) - alpha.
    let inv_offset = alpha.tan() - alpha;
    // Half the tooth's angular width at the pitch circle: s/2 = pi*m/4 there.
    let pitch_half = PI / (2.0 * z as f64);
    // Involute roll parameter at a radius: t = sqrt((r / r_b)^2 - 1).
    let roll_at = |radius: f64| ((radius / r_base).powi(2) - 1.0).sqrt();

    // Below the base circle the involute does not exist; the flank there is a
    // straight radial line (the standard simplified profile, no root fillet).
    let radial_root = r_root < r_base;
    let roll_lo = if radial_root { 0.0 } else { roll_at(r_root) };
    let roll_hi = roll_at(r_tip);

    // The involute of the base circle, unrolled from the point (r_b, 0):
    // radius r_b*sqrt(1 + t^2), polar angle t - atan(t).
    let involute = |t: f64| {
        (
            r_base * (t.cos() + t * t.sin()),
            r_base * (t.sin() - t * t.cos()),
        )
    };

    // Rotate [x, y] by `angle`.
    let rot = |p: [f64; 2], angle: f64| -> [f64; 2] {
        let (s, c) = angle.sin_cos();
        [p[0] * c - p[1] * s, p[0] * s + p[1] * c]
    };

    // Right flank (the counter-clockwise side of the tooth), root to tip:
    // the mirrored involute, so the flank leans in as the radius grows.
    // Root points sit at angle theta_k + pitch_half + inv_offset (radial part,
    // relative angle 0), tip points at theta_k + pitch_half + inv_offset -
    // (t_hi - atan(t_hi)).
    let right_flank = |theta_k: f64| -> Vec<[f64; 2]> {
        let rho = theta_k + pitch_half + inv_offset;
        let mut points = Vec::with_capacity(2 + FLANK_SEGMENTS);
        if radial_root {
            points.push(rot([r_root, 0.0], rho));
            points.push(rot([r_base, 0.0], rho));
        }
        for i in 1..=FLANK_SEGMENTS {
            let t = roll_lo + (roll_hi - roll_lo) * i as f64 / FLANK_SEGMENTS as f64;
            let (x, y) = involute(t);
            points.push(rot([x, -y], rho));
        }
        points
    };

    // Left flank = the right flank mirrored about the tooth centre line.
    let mirror = |points: &[[f64; 2]], theta_k: f64| -> Vec<[f64; 2]> {
        points
            .iter()
            .map(|p| {
                let angle = p[1].atan2(p[0]);
                let radius = p[0].hypot(p[1]);
                [
                    radius * (2.0 * theta_k - angle).cos(),
                    radius * (2.0 * theta_k - angle).sin(),
                ]
            })
            .collect()
    };

    let pitch = 2.0 * PI / z as f64;
    let teeth = z as usize;

    // Every tooth's right flank (root to tip), and its mirror.
    let rights: Vec<Vec<[f64; 2]>> = (0..teeth).map(|k| right_flank(k as f64 * pitch)).collect();
    let lefts: Vec<Vec<[f64; 2]>> = (0..teeth)
        .map(|k| mirror(&rights[k], k as f64 * pitch))
        .collect();

    let centre = [0.0, 0.0];
    let mut pieces = Vec::with_capacity(4 * z as usize);
    for k in 0..teeth {
        let left = &lefts[k];
        let right = &rights[k];

        // Root arc: counter-clockwise from the previous tooth's right flank
        // root to this tooth's left flank root.
        pieces.push(Piece::Arc {
            center: centre,
            from: if k == 0 {
                rights[teeth - 1][0]
            } else {
                rights[k - 1][0]
            },
            to: left[0],
        });
        // Up the left flank.
        pieces.push(Piece::Polyline {
            points: left.clone(),
        });
        // Tip arc: counter-clockwise from the left tip to the right tip.
        pieces.push(Piece::Arc {
            center: centre,
            from: *left.last().unwrap(),
            to: *right.last().unwrap(),
        });
        // Down the right flank.
        let mut down = right.clone();
        down.reverse();
        pieces.push(Piece::Polyline { points: down });
    }
    pieces
}

/// The area the outline encloses, by the shoelace formula over a fine sampling
/// of the arcs. Used for tests and for the reported volume estimate.
pub fn profile_area(spec: &GearSpec) -> f64 {
    let sampled = sample_outline(spec, 64);
    let mut twice_area = 0.0;
    for i in 0..sampled.len() {
        let a = sampled[i];
        let b = sampled[(i + 1) % sampled.len()];
        twice_area += a[0] * b[1] - b[0] * a[1];
    }
    (twice_area / 2.0).abs()
}

/// The outline as a fine polygon: `arcs_per_arc` chords per arc. The flanks are
/// already polylines and pass through unchanged.
fn sample_outline(spec: &GearSpec, arcs_per_arc: usize) -> Vec<[f64; 2]> {
    let mut points = Vec::new();
    for piece in outline(spec) {
        match piece {
            Piece::Polyline { points: polyline } => {
                points.extend(polyline.iter().skip(1));
            }
            Piece::Arc { center, from, to } => {
                let radius = (from[0] - center[0]).hypot(from[1] - center[1]);
                let start = (from[1] - center[1]).atan2(from[0] - center[0]);
                let end = (to[1] - center[1]).atan2(to[0] - center[0]);
                let mut sweep = end - start;
                while sweep <= 0.0 {
                    sweep += 2.0 * PI;
                }
                for i in 1..arcs_per_arc {
                    let angle = start + sweep * i as f64 / arcs_per_arc as f64;
                    points.push([
                        center[0] + radius * angle.cos(),
                        center[1] + radius * angle.sin(),
                    ]);
                }
            }
        }
    }
    points
}

/// The extruded volume the plugin's commands should produce: the profile minus
/// the bore, times the face width. An estimate: the flanks are polylines.
pub fn volume_estimate(spec: &GearSpec) -> f64 {
    let bore_area = if spec.bore_diameter_mm > 0.0 {
        PI * (spec.bore_diameter_mm / 2.0).powi(2)
    } else {
        0.0
    };
    (profile_area(spec) - bore_area) * spec.thickness_mm
}
