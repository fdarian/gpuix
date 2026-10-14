//! The CSS Color 4 spaces `color()` and `color-mix()` name, as conversions
//! to and from gamma-encoded sRGB.
//!
//! Components are kept in `f64` and are never clamped: a Display P3 red is
//! outside 0..1 in sRGB, and `color-mix()` must interpolate it as such. The
//! caller clips once, at the end. A hue that is powerless (a grey has no hue)
//! comes back as NaN, which is how the mixer knows to borrow the other colour's.

pub(crate) type Vec3 = [f64; 3];
type Mat3 = [[f64; 3]; 3];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Space {
    Srgb,
    SrgbLinear,
    DisplayP3,
    A98Rgb,
    ProphotoRgb,
    Rec2020,
    XyzD65,
    XyzD50,
    Lab,
    Lch,
    Oklab,
    Oklch,
    Hsl,
    Hwb,
}

impl Space {
    /// The names `color()` accepts.
    pub(crate) fn from_color_function(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "srgb" => Self::Srgb,
            "srgb-linear" => Self::SrgbLinear,
            "display-p3" => Self::DisplayP3,
            "a98-rgb" => Self::A98Rgb,
            "prophoto-rgb" => Self::ProphotoRgb,
            "rec2020" => Self::Rec2020,
            "xyz" | "xyz-d65" => Self::XyzD65,
            "xyz-d50" => Self::XyzD50,
            _ => return None,
        })
    }

    /// The names `color-mix(in <space>)` accepts: everything `color()` does,
    /// plus the non-RGB spaces.
    pub(crate) fn from_interpolation(name: &str) -> Option<Self> {
        Self::from_color_function(name).or_else(|| {
            Some(match name.to_ascii_lowercase().as_str() {
                "lab" => Self::Lab,
                "lch" => Self::Lch,
                "oklab" => Self::Oklab,
                "oklch" => Self::Oklch,
                "hsl" => Self::Hsl,
                "hwb" => Self::Hwb,
                _ => return None,
            })
        })
    }

    /// Which component is the hue angle in degrees, for the polar spaces.
    pub(crate) fn hue_index(self) -> Option<usize> {
        match self {
            Self::Hsl | Self::Hwb => Some(0),
            Self::Lch | Self::Oklch => Some(2),
            _ => None,
        }
    }

    pub(crate) fn from_srgb(self, rgb: Vec3) -> Vec3 {
        match self {
            Self::Srgb => rgb,
            Self::SrgbLinear => rgb.map(srgb_to_linear),
            Self::Hsl => rgb_to_hsl(rgb),
            Self::Hwb => rgb_to_hwb(rgb),
            Self::Oklab => linear_srgb_to_oklab(rgb.map(srgb_to_linear)),
            Self::Oklch => to_polar(linear_srgb_to_oklab(rgb.map(srgb_to_linear)), OKLCH_POWERLESS),
            _ => self.from_xyz_d65(srgb_to_xyz_d65(rgb)),
        }
    }

    pub(crate) fn to_srgb(self, components: Vec3) -> Vec3 {
        match self {
            Self::Srgb => components,
            Self::SrgbLinear => components.map(linear_to_srgb),
            Self::Hsl => hsl_to_rgb(components),
            Self::Hwb => hwb_to_rgb(components),
            Self::Oklab => oklab_to_linear_srgb(components).map(linear_to_srgb),
            Self::Oklch => oklab_to_linear_srgb(from_polar(components)).map(linear_to_srgb),
            _ => xyz_d65_to_srgb(self.to_xyz_d65(components)),
        }
    }

    fn to_xyz_d65(self, c: Vec3) -> Vec3 {
        match self {
            Self::XyzD65 => c,
            Self::XyzD50 => mul(&d50_to_d65(), c),
            Self::DisplayP3 => mul(&p3_to_xyz(), c.map(srgb_to_linear)),
            Self::A98Rgb => mul(&a98_to_xyz(), c.map(a98_to_linear)),
            Self::ProphotoRgb => mul(&d50_to_d65(), mul(&prophoto_to_xyz_d50(), c.map(prophoto_to_linear))),
            Self::Rec2020 => mul(&rec2020_to_xyz(), c.map(rec2020_to_linear)),
            Self::Lab => mul(&d50_to_d65(), lab_to_xyz_d50(c)),
            Self::Lch => mul(&d50_to_d65(), lab_to_xyz_d50(from_polar(c))),
            _ => unreachable!("{self:?} converts through sRGB, not XYZ"),
        }
    }

    fn from_xyz_d65(self, xyz: Vec3) -> Vec3 {
        match self {
            Self::XyzD65 => xyz,
            Self::XyzD50 => mul(&d65_to_d50(), xyz),
            Self::DisplayP3 => mul(&inverse(&p3_to_xyz()), xyz).map(linear_to_srgb),
            Self::A98Rgb => mul(&inverse(&a98_to_xyz()), xyz).map(linear_to_a98),
            Self::ProphotoRgb => {
                mul(&inverse(&prophoto_to_xyz_d50()), mul(&d65_to_d50(), xyz)).map(linear_to_prophoto)
            }
            Self::Rec2020 => mul(&inverse(&rec2020_to_xyz()), xyz).map(linear_to_rec2020),
            Self::Lab => xyz_d50_to_lab(mul(&d65_to_d50(), xyz)),
            Self::Lch => to_polar(xyz_d50_to_lab(mul(&d65_to_d50(), xyz)), LCH_POWERLESS),
            _ => unreachable!("{self:?} converts through sRGB, not XYZ"),
        }
    }
}

/// CSS Color 4 treats chroma below these as "powerless": the hue is missing.
const LCH_POWERLESS: f64 = 0.0015;
const OKLCH_POWERLESS: f64 = 0.000004;

// ── Matrices ────────────────────────────────────────────────────────────

fn mul(m: &Mat3, v: Vec3) -> Vec3 {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn mat_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            out[row][column] = (0..3).map(|k| a[row][k] * b[k][column]).sum();
        }
    }
    out
}

fn inverse(m: &Mat3) -> Mat3 {
    let cofactor = |r0: usize, c0: usize, r1: usize, c1: usize| {
        m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]
    };
    let c00 = cofactor(1, 1, 2, 2);
    let c01 = -cofactor(1, 0, 2, 2);
    let c02 = cofactor(1, 0, 2, 1);
    let det = m[0][0] * c00 + m[0][1] * c01 + m[0][2] * c02;
    let inv_det = 1.0 / det;
    [
        [
            c00 * inv_det,
            -cofactor(0, 1, 2, 2) * inv_det,
            cofactor(0, 1, 1, 2) * inv_det,
        ],
        [
            c01 * inv_det,
            cofactor(0, 0, 2, 2) * inv_det,
            -cofactor(0, 0, 1, 2) * inv_det,
        ],
        [
            c02 * inv_det,
            -cofactor(0, 0, 2, 1) * inv_det,
            cofactor(0, 0, 1, 1) * inv_det,
        ],
    ]
}

fn xy_to_xyz(x: f64, y: f64) -> Vec3 {
    [x / y, 1.0, (1.0 - x - y) / y]
}

const WHITE_D65: (f64, f64) = (0.3127, 0.3290);
const WHITE_D50: (f64, f64) = (0.3457, 0.3585);

/// Linear RGB to XYZ for a space given by its primaries and white point.
fn rgb_to_xyz(primaries: [(f64, f64); 3], white: (f64, f64)) -> Mat3 {
    let [r, g, b] = primaries.map(|(x, y)| xy_to_xyz(x, y));
    let columns = [
        [r[0], g[0], b[0]],
        [r[1], g[1], b[1]],
        [r[2], g[2], b[2]],
    ];
    let scale = mul(&inverse(&columns), xy_to_xyz(white.0, white.1));
    [
        [columns[0][0] * scale[0], columns[0][1] * scale[1], columns[0][2] * scale[2]],
        [columns[1][0] * scale[0], columns[1][1] * scale[1], columns[1][2] * scale[2]],
        [columns[2][0] * scale[0], columns[2][1] * scale[1], columns[2][2] * scale[2]],
    ]
}

fn srgb_to_xyz_matrix() -> Mat3 {
    rgb_to_xyz([(0.64, 0.33), (0.30, 0.60), (0.15, 0.06)], WHITE_D65)
}

fn p3_to_xyz() -> Mat3 {
    rgb_to_xyz([(0.680, 0.320), (0.265, 0.690), (0.150, 0.060)], WHITE_D65)
}

fn a98_to_xyz() -> Mat3 {
    rgb_to_xyz([(0.64, 0.33), (0.21, 0.71), (0.15, 0.06)], WHITE_D65)
}

fn rec2020_to_xyz() -> Mat3 {
    rgb_to_xyz([(0.708, 0.292), (0.170, 0.797), (0.131, 0.046)], WHITE_D65)
}

fn prophoto_to_xyz_d50() -> Mat3 {
    rgb_to_xyz([(0.7347, 0.2653), (0.1596, 0.8404), (0.0366, 0.0001)], WHITE_D50)
}

/// Bradford chromatic adaptation between the two white points CSS uses.
fn adapt(from: (f64, f64), to: (f64, f64)) -> Mat3 {
    const BRADFORD: Mat3 = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let source = mul(&BRADFORD, xy_to_xyz(from.0, from.1));
    let target = mul(&BRADFORD, xy_to_xyz(to.0, to.1));
    let scale = [
        [target[0] / source[0], 0.0, 0.0],
        [0.0, target[1] / source[1], 0.0],
        [0.0, 0.0, target[2] / source[2]],
    ];
    mat_mul(&inverse(&BRADFORD), &mat_mul(&scale, &BRADFORD))
}

fn d65_to_d50() -> Mat3 {
    adapt(WHITE_D65, WHITE_D50)
}

fn d50_to_d65() -> Mat3 {
    adapt(WHITE_D50, WHITE_D65)
}

fn srgb_to_xyz_d65(rgb: Vec3) -> Vec3 {
    mul(&srgb_to_xyz_matrix(), rgb.map(srgb_to_linear))
}

fn xyz_d65_to_srgb(xyz: Vec3) -> Vec3 {
    mul(&inverse(&srgb_to_xyz_matrix()), xyz).map(linear_to_srgb)
}

// ── Transfer functions, sign-preserving so out-of-gamut values survive ──

fn srgb_to_linear(value: f64) -> f64 {
    let magnitude = value.abs();
    let linear = if magnitude <= 0.04045 {
        magnitude / 12.92
    } else {
        ((magnitude + 0.055) / 1.055).powf(2.4)
    };
    linear.copysign(value)
}

fn linear_to_srgb(value: f64) -> f64 {
    let magnitude = value.abs();
    let encoded = if magnitude <= 0.0031308 {
        magnitude * 12.92
    } else {
        1.055 * magnitude.powf(1.0 / 2.4) - 0.055
    };
    encoded.copysign(value)
}

fn a98_to_linear(value: f64) -> f64 {
    value.abs().powf(563.0 / 256.0).copysign(value)
}

fn linear_to_a98(value: f64) -> f64 {
    value.abs().powf(256.0 / 563.0).copysign(value)
}

fn prophoto_to_linear(value: f64) -> f64 {
    let magnitude = value.abs();
    let linear = if magnitude <= 16.0 / 512.0 {
        magnitude / 16.0
    } else {
        magnitude.powf(1.8)
    };
    linear.copysign(value)
}

fn linear_to_prophoto(value: f64) -> f64 {
    let magnitude = value.abs();
    let encoded = if magnitude < 1.0 / 512.0 {
        magnitude * 16.0
    } else {
        magnitude.powf(1.0 / 1.8)
    };
    encoded.copysign(value)
}

const REC2020_ALPHA: f64 = 1.09929682680944;
const REC2020_BETA: f64 = 0.018053968510807;

fn rec2020_to_linear(value: f64) -> f64 {
    let magnitude = value.abs();
    let linear = if magnitude < REC2020_BETA * 4.5 {
        magnitude / 4.5
    } else {
        ((magnitude + REC2020_ALPHA - 1.0) / REC2020_ALPHA).powf(1.0 / 0.45)
    };
    linear.copysign(value)
}

fn linear_to_rec2020(value: f64) -> f64 {
    let magnitude = value.abs();
    let encoded = if magnitude < REC2020_BETA {
        magnitude * 4.5
    } else {
        REC2020_ALPHA * magnitude.powf(0.45) - (REC2020_ALPHA - 1.0)
    };
    encoded.copysign(value)
}

// ── Lab, Oklab, and their polar forms ───────────────────────────────────

const LAB_EPSILON: f64 = 216.0 / 24389.0;
const LAB_KAPPA: f64 = 24389.0 / 27.0;

fn d50_white() -> Vec3 {
    xy_to_xyz(WHITE_D50.0, WHITE_D50.1)
}

fn xyz_d50_to_lab(xyz: Vec3) -> Vec3 {
    let white = d50_white();
    let f = [0, 1, 2].map(|index| {
        let t = xyz[index] / white[index];
        if t > LAB_EPSILON {
            t.cbrt()
        } else {
            (LAB_KAPPA * t + 16.0) / 116.0
        }
    });
    [116.0 * f[1] - 16.0, 500.0 * (f[0] - f[1]), 200.0 * (f[1] - f[2])]
}

fn lab_to_xyz_d50(lab: Vec3) -> Vec3 {
    let fy = (lab[0] + 16.0) / 116.0;
    let fx = lab[1] / 500.0 + fy;
    let fz = fy - lab[2] / 200.0;
    let cube = |f: f64| {
        let f3 = f * f * f;
        if f3 > LAB_EPSILON {
            f3
        } else {
            (116.0 * f - 16.0) / LAB_KAPPA
        }
    };
    let y = if lab[0] > LAB_KAPPA * LAB_EPSILON {
        fy * fy * fy
    } else {
        lab[0] / LAB_KAPPA
    };
    let white = d50_white();
    [cube(fx) * white[0], y * white[1], cube(fz) * white[2]]
}

fn linear_srgb_to_oklab(rgb: Vec3) -> Vec3 {
    let l = (0.4122214708 * rgb[0] + 0.5363325363 * rgb[1] + 0.0514459929 * rgb[2]).cbrt();
    let m = (0.2119034982 * rgb[0] + 0.6806995451 * rgb[1] + 0.1073969566 * rgb[2]).cbrt();
    let s = (0.0883024619 * rgb[0] + 0.2817188376 * rgb[1] + 0.6299787005 * rgb[2]).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

fn oklab_to_linear_srgb(lab: Vec3) -> Vec3 {
    let l = lab[0] + 0.3963377774 * lab[1] + 0.2158037573 * lab[2];
    let m = lab[0] - 0.1055613458 * lab[1] - 0.0638541728 * lab[2];
    let s = lab[0] - 0.0894841775 * lab[1] - 1.2914855480 * lab[2];
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    [
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    ]
}

/// `(L, a, b)` to `(L, chroma, hue)`. Hue is NaN under `powerless` chroma.
fn to_polar(lab: Vec3, powerless: f64) -> Vec3 {
    let chroma = lab[1].hypot(lab[2]);
    let hue = if chroma <= powerless {
        f64::NAN
    } else {
        lab[2].atan2(lab[1]).to_degrees().rem_euclid(360.0)
    };
    [lab[0], chroma, hue]
}

fn from_polar(lch: Vec3) -> Vec3 {
    let hue = if lch[2].is_nan() { 0.0 } else { lch[2].to_radians() };
    [lch[0], lch[1] * hue.cos(), lch[1] * hue.sin()]
}

// ── HSL and HWB ─────────────────────────────────────────────────────────

fn rgb_to_hsl(rgb: Vec3) -> Vec3 {
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    let min = rgb[0].min(rgb[1]).min(rgb[2]);
    let delta = max - min;
    let lightness = (max + min) / 2.0;
    if delta.abs() < 1e-9 {
        return [f64::NAN, 0.0, lightness];
    }
    let denominator = 1.0 - (2.0 * lightness - 1.0).abs();
    let saturation = if denominator.abs() < 1e-12 { 0.0 } else { delta / denominator };
    let hue = if max == rgb[0] {
        (rgb[1] - rgb[2]) / delta + if rgb[1] < rgb[2] { 6.0 } else { 0.0 }
    } else if max == rgb[1] {
        (rgb[2] - rgb[0]) / delta + 2.0
    } else {
        (rgb[0] - rgb[1]) / delta + 4.0
    };
    [(hue * 60.0).rem_euclid(360.0), saturation, lightness]
}

fn hsl_to_rgb(hsl: Vec3) -> Vec3 {
    let hue = if hsl[0].is_nan() { 0.0 } else { hsl[0].rem_euclid(360.0) };
    let (saturation, lightness) = (hsl[1], hsl[2]);
    let chroma = saturation * lightness.min(1.0 - lightness);
    let channel = |n: f64| {
        let k = (n + hue / 30.0).rem_euclid(12.0);
        lightness - chroma * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    [channel(0.0), channel(8.0), channel(4.0)]
}

fn rgb_to_hwb(rgb: Vec3) -> Vec3 {
    let hue = rgb_to_hsl(rgb)[0];
    let white = rgb[0].min(rgb[1]).min(rgb[2]);
    let black = 1.0 - rgb[0].max(rgb[1]).max(rgb[2]);
    [hue, white, black]
}

fn hwb_to_rgb(hwb: Vec3) -> Vec3 {
    let (white, black) = (hwb[1], hwb[2]);
    if white + black >= 1.0 {
        let grey = white / (white + black);
        return [grey; 3];
    }
    hsl_to_rgb([hwb[0], 1.0, 0.5]).map(|channel| channel * (1.0 - white - black) + white)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: Vec3, expected: Vec3, tolerance: f64) {
        for index in 0..3 {
            assert!(
                (actual[index] - expected[index]).abs() <= tolerance,
                "{actual:?} != {expected:?}"
            );
        }
    }

    const SAMPLES: [Vec3; 5] = [
        [1.0, 0.0, 0.0],
        [0.2, 0.5, 0.8],
        [0.73, 0.83, 0.33],
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
    ];

    #[test]
    fn every_space_round_trips_through_srgb() {
        for space in [
            Space::SrgbLinear,
            Space::DisplayP3,
            Space::A98Rgb,
            Space::ProphotoRgb,
            Space::Rec2020,
            Space::XyzD65,
            Space::XyzD50,
            Space::Lab,
            Space::Lch,
            Space::Oklab,
            Space::Oklch,
            Space::Hsl,
            Space::Hwb,
        ] {
            for sample in SAMPLES {
                let there = space.from_srgb(sample);
                close(space.to_srgb(there), sample, 1e-6);
            }
        }
    }

    #[test]
    fn matches_known_reference_values() {
        // CSS Color 4 and the csscolorparser tests: sRGB red.
        close(Space::Oklab.from_srgb([1.0, 0.0, 0.0]), [0.62796, 0.22486, 0.12585], 1e-4);
        close(Space::Oklch.from_srgb([1.0, 0.0, 0.0]), [0.62796, 0.25768, 29.2339], 2e-3);
        close(Space::Lab.from_srgb([1.0, 0.0, 0.0]), [54.2905, 80.8049, 69.8910], 2e-2);
        close(Space::DisplayP3.from_srgb([1.0, 0.0, 0.0]), [0.9175, 0.2003, 0.1386], 1e-3);
        close(Space::XyzD65.from_srgb([1.0, 1.0, 1.0]), [0.95046, 1.0, 1.08906], 1e-4);
        close(Space::XyzD50.from_srgb([1.0, 1.0, 1.0]), [0.96430, 1.0, 0.82511], 1e-4);
        close(Space::Hsl.from_srgb([1.0, 0.0, 0.0]), [0.0, 1.0, 0.5], 1e-9);
        close(Space::Hwb.from_srgb([1.0, 0.0, 0.0]), [0.0, 0.0, 0.0], 1e-9);
    }

    #[test]
    fn greys_have_a_powerless_hue() {
        for space in [Space::Hsl, Space::Hwb, Space::Lch, Space::Oklch] {
            let converted = space.from_srgb([0.5, 0.5, 0.5]);
            assert!(converted[space.hue_index().unwrap()].is_nan(), "{space:?}");
        }
    }

    #[test]
    fn wide_gamut_values_leave_the_unit_cube_unclamped() {
        let p3_red = Space::DisplayP3.to_srgb([1.0, 0.0, 0.0]);
        assert!(p3_red[0] > 1.0 && p3_red[1] < 0.0);
    }
}
