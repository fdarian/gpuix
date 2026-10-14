//! SVG path data and shape geometry: absolute normalisation for `d` morphs,
//! and the real length of a shape for `pathLength` dashes.
//!
//! The rasteriser (resvg) ignores the `pathLength` attribute, so dash values
//! that depend on it have to be turned into absolute user units here.

use kurbo::{BezPath, Ellipse, Shape};
use svgtypes::{PathParser, PathSegment, PointsParser};

/// Arc length tolerance in user units. Dashes are drawn at pixel scale, so a
/// thousandth of a unit is far below anything visible.
const ACCURACY: f64 = 1e-3;

/// A path command in its absolute form. `H`, `V`, `S`, `T` and `A` stay what
/// they are, so two paths morph only when they were written with the same
/// commands, as CSS requires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    MoveTo,
    LineTo,
    Horizontal,
    Vertical,
    Cubic,
    SmoothCubic,
    Quadratic,
    SmoothQuadratic,
    Arc,
    Close,
}

impl Command {
    fn letter(self) -> char {
        match self {
            Self::MoveTo => 'M',
            Self::LineTo => 'L',
            Self::Horizontal => 'H',
            Self::Vertical => 'V',
            Self::Cubic => 'C',
            Self::SmoothCubic => 'S',
            Self::Quadratic => 'Q',
            Self::SmoothQuadratic => 'T',
            Self::Arc => 'A',
            Self::Close => 'Z',
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Segment {
    command: Command,
    /// Arc flags are not numbers to blend, so they live apart and take part in
    /// the structure comparison instead.
    flags: Option<(bool, bool)>,
    /// For an arc: `rx ry x-axis-rotation x y`.
    numbers: Vec<f64>,
}

impl Segment {
    fn same_structure(&self, other: &Self) -> bool {
        self.command == other.command && self.flags == other.flags
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PathData {
    segments: Vec<Segment>,
}

impl PathData {
    /// Parses `d` and rewrites every relative command as an absolute one.
    pub(crate) fn parse(data: &str) -> Result<Self, String> {
        let mut segments = Vec::new();
        let mut current = (0.0, 0.0);
        let mut subpath_start = (0.0, 0.0);
        let absolute = |abs: bool, origin: f64, value: f64| if abs { value } else { origin + value };

        for parsed in PathParser::from(data) {
            let segment = parsed.map_err(|error| format!("invalid path data {data:?}: {error}"))?;
            let (x0, y0) = current;
            let (command, flags, numbers) = match segment {
                PathSegment::MoveTo { abs, x, y } => {
                    let end = (absolute(abs, x0, x), absolute(abs, y0, y));
                    subpath_start = end;
                    current = end;
                    (Command::MoveTo, None, vec![end.0, end.1])
                }
                PathSegment::LineTo { abs, x, y } => {
                    let end = (absolute(abs, x0, x), absolute(abs, y0, y));
                    current = end;
                    (Command::LineTo, None, vec![end.0, end.1])
                }
                PathSegment::HorizontalLineTo { abs, x } => {
                    let x = absolute(abs, x0, x);
                    current.0 = x;
                    (Command::Horizontal, None, vec![x])
                }
                PathSegment::VerticalLineTo { abs, y } => {
                    let y = absolute(abs, y0, y);
                    current.1 = y;
                    (Command::Vertical, None, vec![y])
                }
                PathSegment::CurveTo {
                    abs,
                    x1,
                    y1,
                    x2,
                    y2,
                    x,
                    y,
                } => {
                    let numbers = vec![
                        absolute(abs, x0, x1),
                        absolute(abs, y0, y1),
                        absolute(abs, x0, x2),
                        absolute(abs, y0, y2),
                        absolute(abs, x0, x),
                        absolute(abs, y0, y),
                    ];
                    current = (numbers[4], numbers[5]);
                    (Command::Cubic, None, numbers)
                }
                PathSegment::SmoothCurveTo { abs, x2, y2, x, y } => {
                    let numbers = vec![
                        absolute(abs, x0, x2),
                        absolute(abs, y0, y2),
                        absolute(abs, x0, x),
                        absolute(abs, y0, y),
                    ];
                    current = (numbers[2], numbers[3]);
                    (Command::SmoothCubic, None, numbers)
                }
                PathSegment::Quadratic { abs, x1, y1, x, y } => {
                    let numbers = vec![
                        absolute(abs, x0, x1),
                        absolute(abs, y0, y1),
                        absolute(abs, x0, x),
                        absolute(abs, y0, y),
                    ];
                    current = (numbers[2], numbers[3]);
                    (Command::Quadratic, None, numbers)
                }
                PathSegment::SmoothQuadratic { abs, x, y } => {
                    let end = (absolute(abs, x0, x), absolute(abs, y0, y));
                    current = end;
                    (Command::SmoothQuadratic, None, vec![end.0, end.1])
                }
                PathSegment::EllipticalArc {
                    abs,
                    rx,
                    ry,
                    x_axis_rotation,
                    large_arc,
                    sweep,
                    x,
                    y,
                } => {
                    let end = (absolute(abs, x0, x), absolute(abs, y0, y));
                    current = end;
                    (
                        Command::Arc,
                        Some((large_arc, sweep)),
                        vec![rx, ry, x_axis_rotation, end.0, end.1],
                    )
                }
                PathSegment::ClosePath { .. } => {
                    current = subpath_start;
                    (Command::Close, None, Vec::new())
                }
            };
            if numbers.iter().any(|number| !number.is_finite()) {
                return Err(format!("path data {data:?} has a number that is not finite"));
            }
            segments.push(Segment {
                command,
                flags,
                numbers,
            });
        }
        Ok(Self { segments })
    }

    /// Whether the two paths have the same commands (and arc flags), so every
    /// number can be blended pairwise.
    pub(crate) fn same_structure(&self, other: &Self) -> bool {
        self.segments.len() == other.segments.len()
            && self
                .segments
                .iter()
                .zip(&other.segments)
                .all(|(left, right)| left.same_structure(right))
    }

    pub(crate) fn numbers(&self) -> Vec<f64> {
        self.segments
            .iter()
            .flat_map(|segment| segment.numbers.iter().copied())
            .collect()
    }

    /// The same commands with `numbers` in place of this path's, in the order
    /// [`numbers`](Self::numbers) lists them.
    pub(crate) fn with_numbers(&self, numbers: &[f64]) -> Self {
        let mut remaining = numbers.iter().copied();
        Self {
            segments: self
                .segments
                .iter()
                .map(|segment| Segment {
                    numbers: segment
                        .numbers
                        .iter()
                        .map(|_| remaining.next().unwrap_or_default())
                        .collect(),
                    ..segment.clone()
                })
                .collect(),
        }
    }

    pub(crate) fn to_data(&self) -> String {
        let mut data = String::new();
        for segment in &self.segments {
            if !data.is_empty() {
                data.push(' ');
            }
            data.push(segment.command.letter());
            for (index, number) in segment.numbers.iter().enumerate() {
                // The flags sit between the rotation and the end point.
                if let (3, Some((large_arc, sweep))) = (index, segment.flags) {
                    data.push_str(&format!(" {} {}", u8::from(large_arc), u8::from(sweep)));
                }
                data.push(' ');
                data.push_str(&format_number(*number));
            }
        }
        data
    }

    pub(crate) fn length(&self) -> Result<f64, String> {
        let path = BezPath::from_svg(&self.to_data())
            .map_err(|error| format!("cannot measure path: {error}"))?;
        Ok(path.perimeter(ACCURACY))
    }
}

/// Shortest decimal that round-trips at four places, without a trailing `.0`.
pub(crate) fn format_number(value: f64) -> String {
    let rounded = (value * 1e4).round() / 1e4;
    if rounded == 0.0 {
        return "0".to_string();
    }
    format!("{rounded}")
}

pub(crate) fn circle_length(radius: f64) -> f64 {
    std::f64::consts::TAU * radius.max(0.0)
}

pub(crate) fn ellipse_length(rx: f64, ry: f64) -> f64 {
    if rx <= 0.0 || ry <= 0.0 {
        return 0.0;
    }
    Ellipse::new((0.0, 0.0), (rx, ry), 0.0).perimeter(ACCURACY)
}

/// The perimeter of a `<rect>`. A missing radius takes the other one, and each
/// is held to half the side, as SVG specifies.
pub(crate) fn rect_length(width: f64, height: f64, rx: Option<f64>, ry: Option<f64>) -> f64 {
    if width <= 0.0 || height <= 0.0 {
        return 0.0;
    }
    let (rx, ry) = match (rx, ry) {
        (None, None) => (0.0, 0.0),
        (Some(rx), None) => (rx, rx),
        (None, Some(ry)) => (ry, ry),
        (Some(rx), Some(ry)) => (rx, ry),
    };
    let rx = rx.clamp(0.0, width / 2.0);
    let ry = ry.clamp(0.0, height / 2.0);
    2.0 * (width - 2.0 * rx) + 2.0 * (height - 2.0 * ry) + ellipse_length(rx, ry)
}

pub(crate) fn line_length(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    (x2 - x1).hypot(y2 - y1)
}

pub(crate) fn polyline_length(points: &str, close: bool) -> f64 {
    let points: Vec<(f64, f64)> = PointsParser::from(points).collect();
    let mut length: f64 = points
        .windows(2)
        .map(|pair| line_length(pair[0].0, pair[0].1, pair[1].0, pair[1].1))
        .sum();
    if close && points.len() > 2 {
        let (first, last) = (points[0], points[points.len() - 1]);
        length += line_length(last.0, last.1, first.0, first.1);
    }
    length
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(data: &str) -> PathData {
        PathData::parse(data).unwrap()
    }

    #[test]
    fn relative_commands_become_absolute() {
        assert_eq!(
            parse("m10 10 l5 0 h5 v5 c1 1 2 2 3 3 z").to_data(),
            "M 10 10 L 15 10 H 20 V 15 C 21 16 22 17 23 18 Z"
        );
    }

    #[test]
    fn a_relative_move_after_close_starts_from_the_subpath_start() {
        assert_eq!(
            parse("M10 10 L20 10 L20 20 z m5 5 l1 1").to_data(),
            "M 10 10 L 20 10 L 20 20 Z M 15 15 L 16 16"
        );
    }

    #[test]
    fn implicit_repeats_are_written_out() {
        assert_eq!(
            parse("M0 0 10 10 20 0").to_data(),
            "M 0 0 L 10 10 L 20 0"
        );
    }

    #[test]
    fn arc_flags_stay_between_the_radii_and_the_end_point() {
        let arc = parse("M0 0 a5 5 0 1 1 10 0");
        assert_eq!(arc.to_data(), "M 0 0 A 5 5 0 1 1 10 0");
        assert_eq!(arc.numbers(), vec![0.0, 0.0, 5.0, 5.0, 0.0, 10.0, 0.0]);
    }

    #[test]
    fn same_commands_have_the_same_structure_whatever_their_case() {
        assert!(parse("M0 0 L10 10").same_structure(&parse("m5 5 l2 2")));
        assert!(!parse("M0 0 L10 10").same_structure(&parse("M0 0 H10")));
        assert!(!parse("M0 0 A5 5 0 0 1 10 0").same_structure(&parse("M0 0 A5 5 0 1 1 10 0")));
        assert!(!parse("M0 0 L10 10").same_structure(&parse("M0 0 L10 10 L20 0")));
    }

    #[test]
    fn numbers_round_trip_through_a_template() {
        let path = parse("M0 0 C1 2 3 4 5 6 Z");
        let halved: Vec<f64> = path.numbers().iter().map(|number| number / 2.0).collect();
        assert_eq!(
            path.with_numbers(&halved).to_data(),
            "M 0 0 C 0.5 1 1.5 2 2.5 3 Z"
        );
    }

    #[test]
    fn rejects_malformed_data() {
        assert!(PathData::parse("L10 10").is_err());
        assert!(PathData::parse("M0 0 L10").is_err());
        assert!(PathData::parse("M0 0 X").is_err());
        assert!(PathData::parse("").unwrap().segments.is_empty());
    }

    #[test]
    fn measures_path_length() {
        assert!((parse("M0 0 L30 40").length().unwrap() - 50.0).abs() < 1e-9);
        assert!((parse("M0 0 H10 V10 H0 Z").length().unwrap() - 40.0).abs() < 1e-6);
        let half_circle = parse("M0 0 A5 5 0 0 1 10 0").length().unwrap();
        assert!((half_circle - std::f64::consts::PI * 5.0).abs() < 1e-2, "{half_circle}");
    }

    #[test]
    fn measures_shapes() {
        assert!((circle_length(10.0) - 62.831853).abs() < 1e-5);
        assert!((ellipse_length(5.0, 5.0) - 31.415927).abs() < 1e-2);
        assert!((rect_length(10.0, 20.0, None, None) - 60.0).abs() < 1e-9);
        // A rounded corner shortens the straight sides by 2r and adds a quarter
        // circle each: the perimeter is 2(w-2r) + 2(h-2r) + 2*pi*r.
        let rounded = rect_length(10.0, 20.0, Some(2.0), None);
        assert!((rounded - (2.0 * 6.0 + 2.0 * 16.0 + std::f64::consts::TAU * 2.0)).abs() < 1e-2);
        assert!((line_length(0.0, 0.0, 3.0, 4.0) - 5.0).abs() < 1e-12);
        assert!((polyline_length("0,0 10,0 10,10", false) - 20.0).abs() < 1e-12);
        assert!((polyline_length("0,0 10,0 10,10", true) - (20.0 + 200f64.sqrt())).abs() < 1e-9);
    }

    #[test]
    fn numbers_are_written_without_noise() {
        assert_eq!(format_number(1.0), "1");
        assert_eq!(format_number(0.333333333), "0.3333");
        assert_eq!(format_number(-0.00001), "0");
        assert_eq!(format_number(12.5), "12.5");
    }
}
