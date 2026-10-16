//! CSS grid syntax: track lists, line placement, `grid-template-areas`.
//!
//! Everything here parses once, when a style is deserialized, so the per-frame
//! `apply_styles` only moves already-built values into GPUI.

use crate::css::dimension::{
    parse_length_percentage, parse_linear_length, split_dimension, LengthPercentage,
};
use crate::css::length::LengthContext;
use gpui::{
    GridPlacement, GridRepetition, GridTemplateArea, GridTemplateAreas, GridTemplateEntry,
    GridTemplateTracks, GridTrack, GridTrackSize, SharedString,
};

struct Cursor<'a> {
    text: &'a str,
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, position: 0 }
    }

    fn rest(&self) -> &'a str {
        &self.text[self.position..]
    }

    fn skip_whitespace(&mut self) {
        let trimmed = self.rest().trim_start();
        self.position = self.text.len() - trimmed.len();
    }

    fn at_end(&mut self) -> bool {
        self.skip_whitespace();
        self.rest().is_empty()
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_whitespace();
        self.rest().chars().next()
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.position += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), String> {
        if self.eat(expected) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{expected}'")))
        }
    }

    fn error(&self, message: &str) -> String {
        format!("{message} at {:?}", self.rest())
    }

    /// A CSS identifier: letters, digits, `-` and `_`, not starting with a digit.
    fn ident(&mut self) -> Option<&'a str> {
        self.skip_whitespace();
        let rest = self.rest();
        let first = rest.chars().next()?;
        if !(first.is_alphabetic() || first == '_' || first == '-') {
            return None;
        }
        let length = rest
            .char_indices()
            .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '-'))
            .map_or(rest.len(), |(index, _)| index);
        self.position += length;
        Some(&rest[..length])
    }

    fn number(&mut self) -> Result<f32, String> {
        self.skip_whitespace();
        let rest = self.rest();
        let length = rest
            .char_indices()
            .find(|(index, c)| {
                !(c.is_ascii_digit() || *c == '.' || (*index == 0 && (*c == '-' || *c == '+')))
            })
            .map_or(rest.len(), |(index, _)| index);
        let value = rest[..length]
            .parse::<f32>()
            .map_err(|_| self.error("expected a number"))?;
        self.position += length;
        Ok(value)
    }

    fn unit(&mut self) -> &'a str {
        let rest = self.rest();
        let length = rest
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_alphabetic() || *c == '%'))
            .map_or(rest.len(), |(index, _)| index);
        self.position += length;
        &rest[..length]
    }

    /// One `<length>` as written: a number with its unit, or a `calc()` through its
    /// closing parenthesis.
    fn length_token(&mut self) -> Result<&'a str, String> {
        self.skip_whitespace();
        let start = self.position;
        if self.peek_ident_followed_by_paren() == Some("calc") {
            self.ident();
            let mut depth = 0usize;
            for (index, character) in self.rest().char_indices() {
                match character {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            self.position += index + 1;
                            return Ok(&self.text[start..self.position]);
                        }
                    }
                    _ => {}
                }
            }
            return Err(self.error("unclosed calc()"));
        }
        self.number()?;
        self.unit();
        Ok(&self.text[start..self.position])
    }

    fn peek_ident_followed_by_paren(&mut self) -> Option<&'a str> {
        let saved = self.position;
        let name = self.ident();
        let is_call = name.is_some() && self.rest().starts_with('(');
        self.position = saved;
        name.filter(|_| is_call)
    }
}

/// What the lengths of a track list resolve against, and whether any of them
/// needed it: a list that did not parses the same in every context.
pub struct TrackLengths<'a> {
    context: &'a LengthContext,
    relative: bool,
}

/// A parsed track list. `relative` says the list has a length whose px depend
/// on the element's font size or the window, so it has to be parsed again in
/// another context to change.
pub struct Parsed<T> {
    pub value: T,
    pub relative: bool,
}

impl TrackLengths<'_> {
    /// One `<length-percentage>` as a track size: px, a percentage, or the
    /// sum of the two that `calc()` makes.
    fn size(&mut self, token: &str) -> Result<GridTrackSize, String> {
        let linear = parse_linear_length(token)
            .ok_or_else(|| format!("invalid track size {token:?}, use a length, a percentage, calc() or fr"))?;
        if linear.is_negative() {
            return Err("a track size cannot be negative".to_string());
        }
        self.relative |= linear.is_relative();
        let resolved = parse_length_percentage(token, self.context)
            .ok_or_else(|| format!("invalid track size {token:?}"))?;
        Ok(match resolved {
            // A calc() clamps to the range of its property, as in CSS.
            LengthPercentage::Length(px) => GridTrackSize::Length(px.max(0.0) as f32),
            LengthPercentage::Percent(fraction) => GridTrackSize::Percent(fraction as f32),
            LengthPercentage::Calc { px, percent } => GridTrackSize::Calc {
                length: px as f32,
                fraction: percent as f32,
            },
        })
    }
}

/// A `<length-percentage>` or `<flex>`, as a track sizing function bound.
fn parse_dimension(cursor: &mut Cursor, lengths: &mut TrackLengths) -> Result<GridTrackSize, String> {
    let token = cursor.length_token()?;
    match split_dimension(token) {
        Some((value, unit)) if unit == "fr" => {
            if value < 0.0 {
                return Err("a flex factor cannot be negative".to_string());
            }
            Ok(GridTrackSize::Fr(value as f32))
        }
        // A number is px, which CSS only allows for 0.
        Some((value, unit)) if unit.is_empty() && value >= 0.0 => Ok(GridTrackSize::Length(value as f32)),
        Some((_, unit)) if unit.is_empty() => Err("a track size cannot be negative".to_string()),
        _ => lengths.size(token),
    }
}

fn starts_dimension(cursor: &mut Cursor) -> bool {
    cursor.peek().is_some_and(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
        || cursor.peek_ident_followed_by_paren() == Some("calc")
}

fn parse_breadth(cursor: &mut Cursor, lengths: &mut TrackLengths) -> Result<GridTrackSize, String> {
    if starts_dimension(cursor) {
        return parse_dimension(cursor, lengths);
    }
    match cursor.ident() {
        Some("auto") => Ok(GridTrackSize::Auto),
        Some("min-content") => Ok(GridTrackSize::MinContent),
        Some("max-content") => Ok(GridTrackSize::MaxContent),
        _ => Err(cursor.error("expected a track size")),
    }
}

fn parse_track(cursor: &mut Cursor, lengths: &mut TrackLengths) -> Result<GridTrack, String> {
    match cursor.peek_ident_followed_by_paren() {
        Some("minmax") => {
            cursor.ident();
            cursor.expect('(')?;
            let min = parse_breadth(cursor, lengths)?;
            if matches!(min, GridTrackSize::Fr(_)) {
                return Err("the minimum of minmax() cannot be a flex factor".to_string());
            }
            cursor.expect(',')?;
            let max = parse_breadth(cursor, lengths)?;
            cursor.expect(')')?;
            Ok(GridTrack { min, max })
        }
        Some("fit-content") => {
            cursor.ident();
            cursor.expect('(')?;
            let limit = match parse_dimension(cursor, lengths)? {
                GridTrackSize::Length(value) => GridTrackSize::FitContentLength(value),
                GridTrackSize::Percent(value) => GridTrackSize::FitContentPercent(value),
                GridTrackSize::Calc { .. } => {
                    return Err("fit-content() cannot take a length mixed with a percentage".to_string())
                }
                _ => return Err("fit-content() takes a length or a percentage".to_string()),
            };
            cursor.expect(')')?;
            Ok(GridTrack {
                min: GridTrackSize::Auto,
                max: limit,
            })
        }
        Some("calc") | None => {
            let size = parse_breadth(cursor, lengths)?;
            Ok(match size {
                GridTrackSize::Fr(_) => GridTrack {
                    min: GridTrackSize::Auto,
                    max: size,
                },
                size => GridTrack { min: size, max: size },
            })
        }
        Some(other) => Err(format!("unknown function {other}()")),
    }
}

type LineNames = Vec<Vec<SharedString>>;

fn parse_line_names(cursor: &mut Cursor, into: &mut Vec<SharedString>) -> Result<(), String> {
    while cursor.eat('[') {
        while !cursor.eat(']') {
            let name = cursor
                .ident()
                .ok_or_else(|| cursor.error("expected a line name"))?;
            into.push(SharedString::from(name.to_string()));
        }
    }
    Ok(())
}

fn parse_repeat(cursor: &mut Cursor, lengths: &mut TrackLengths) -> Result<GridTemplateEntry, String> {
    cursor.ident();
    cursor.expect('(')?;
    let repetition = match cursor.peek() {
        Some(c) if c.is_ascii_digit() => {
            let count = cursor.number()?;
            if count < 1.0 || count.fract() != 0.0 {
                return Err("repeat() needs a whole count of at least 1".to_string());
            }
            GridRepetition::Count(count.min(u16::MAX as f32) as u16)
        }
        _ => match cursor.ident() {
            Some("auto-fill") => GridRepetition::AutoFill,
            Some("auto-fit") => GridRepetition::AutoFit,
            _ => return Err(cursor.error("expected a repeat count, auto-fill or auto-fit")),
        },
    };
    cursor.expect(',')?;

    let mut tracks = Vec::new();
    let mut line_names: LineNames = Vec::new();
    let mut pending = Vec::new();
    loop {
        parse_line_names(cursor, &mut pending)?;
        if cursor.peek() == Some(')') {
            break;
        }
        if cursor.peek_ident_followed_by_paren() == Some("repeat") {
            return Err("repeat() cannot be nested".to_string());
        }
        tracks.push(parse_track(cursor, lengths)?);
        line_names.push(std::mem::take(&mut pending));
    }
    line_names.push(pending);
    cursor.expect(')')?;
    if tracks.is_empty() {
        return Err("repeat() needs at least one track".to_string());
    }
    Ok(GridTemplateEntry::Repeat {
        repetition,
        tracks,
        line_names,
    })
}

/// `grid-template-columns` / `grid-template-rows`: `none`, or a track list with
/// optional `[line names]` and `repeat()`.
pub(crate) fn parse_template(
    input: &str,
    context: &LengthContext,
) -> Result<Parsed<GridTemplateTracks>, String> {
    let mut cursor = Cursor::new(input);
    let mut lengths = TrackLengths { context, relative: false };
    let mut template = GridTemplateTracks::default();
    let mut pending = Vec::new();
    if cursor.peek_ident_followed_by_paren().is_none() {
        let saved = cursor.position;
        if cursor.ident() == Some("none") && cursor.at_end() {
            return Ok(Parsed { value: template, relative: false });
        }
        cursor.position = saved;
    }
    let mut auto_repeats = 0;
    loop {
        parse_line_names(&mut cursor, &mut pending)?;
        if cursor.at_end() {
            break;
        }
        let entry = if cursor.peek_ident_followed_by_paren() == Some("repeat") {
            let entry = parse_repeat(&mut cursor, &mut lengths)?;
            if let GridTemplateEntry::Repeat { repetition, .. } = &entry {
                if !matches!(repetition, GridRepetition::Count(_)) {
                    auto_repeats += 1;
                }
            }
            entry
        } else {
            GridTemplateEntry::Track(parse_track(&mut cursor, &mut lengths)?)
        };
        template.entries.push(entry);
        template.line_names.push(std::mem::take(&mut pending));
    }
    template.line_names.push(pending);
    if template.entries.is_empty() {
        return Err("expected at least one track".to_string());
    }
    if auto_repeats > 1 {
        return Err("only one auto-fill or auto-fit repeat is allowed".to_string());
    }
    Ok(Parsed { value: template, relative: lengths.relative })
}

/// `grid-auto-columns` / `grid-auto-rows`: a list of track sizes.
pub(crate) fn parse_auto_tracks(
    input: &str,
    context: &LengthContext,
) -> Result<Parsed<Vec<GridTrack>>, String> {
    let mut cursor = Cursor::new(input);
    let mut lengths = TrackLengths { context, relative: false };
    let mut tracks = Vec::new();
    while !cursor.at_end() {
        tracks.push(parse_track(&mut cursor, &mut lengths)?);
    }
    if tracks.is_empty() {
        return Err("expected at least one track size".to_string());
    }
    Ok(Parsed { value: tracks, relative: lengths.relative })
}

/// One `<grid-line>`: `auto`, `3`, `-1`, `span 2`, `name`, `name 2`, `span name`.
pub fn parse_line(input: &str) -> Result<GridPlacement, String> {
    let words: Vec<&str> = input.split_whitespace().collect();
    let invalid = || format!("invalid grid line {input:?}");
    let integer = |word: &str| word.parse::<i16>().ok();
    let ident = |word: &str| {
        let mut cursor = Cursor::new(word);
        let name = cursor.ident()?;
        (name.len() == word.len() && name != "span" && name != "auto")
            .then(|| SharedString::from(name.to_string()))
    };
    match words.as_slice() {
        ["auto"] => Ok(GridPlacement::Auto),
        ["span", value] => match (integer(value), ident(value)) {
            (Some(count), _) if count >= 1 => Ok(GridPlacement::Span(count as u16)),
            (_, Some(name)) => Ok(GridPlacement::NamedSpan(name, 1)),
            _ => Err(invalid()),
        },
        ["span", first, second] => match (integer(first), ident(second), ident(first), integer(second)) {
            (Some(count), Some(name), _, _) | (_, _, Some(name), Some(count)) if count >= 1 => {
                Ok(GridPlacement::NamedSpan(name, count as u16))
            }
            _ => Err(invalid()),
        },
        [value] => match (integer(value), ident(value)) {
            (Some(line), _) if line != 0 => Ok(GridPlacement::Line(line)),
            (_, Some(name)) => Ok(GridPlacement::NamedLine(name, 0)),
            _ => Err(invalid()),
        },
        [first, second] => match (integer(first), ident(second), ident(first), integer(second)) {
            (Some(line), Some(name), _, _) | (_, _, Some(name), Some(line)) if line != 0 => {
                Ok(GridPlacement::NamedLine(name, line))
            }
            _ => Err(invalid()),
        },
        _ => Err(invalid()),
    }
}

fn is_ident_only(placement: &GridPlacement) -> bool {
    matches!(placement, GridPlacement::NamedLine(_, 0))
}

/// `grid-column` / `grid-row`: `start`, or `start / end`.
pub fn parse_line_pair(input: &str) -> Result<(GridPlacement, GridPlacement), String> {
    let mut parts = input.split('/');
    let start = parse_line(parts.next().unwrap_or_default())?;
    let end = match parts.next() {
        Some(end) => parse_line(end)?,
        None if is_ident_only(&start) => start.clone(),
        None => GridPlacement::Auto,
    };
    if parts.next().is_some() {
        return Err(format!("invalid grid placement {input:?}"));
    }
    Ok((start, end))
}

/// `grid-area` as `(row, column)` pairs of `(start, end)`.
pub type GridAreaPlacement = [(GridPlacement, GridPlacement); 2];

/// `grid-area`: `name`, or up to four lines `row-start / column-start / row-end / column-end`.
pub fn parse_area(input: &str) -> Result<GridAreaPlacement, String> {
    let lines = input
        .split('/')
        .map(parse_line)
        .collect::<Result<Vec<_>, _>>()?;
    if lines.is_empty() || lines.len() > 4 {
        return Err(format!("invalid grid-area {input:?}"));
    }
    let omitted = |source: &GridPlacement| {
        if is_ident_only(source) {
            source.clone()
        } else {
            GridPlacement::Auto
        }
    };
    let row_start = lines[0].clone();
    let column_start = lines.get(1).cloned().unwrap_or_else(|| omitted(&row_start));
    let row_end = lines.get(2).cloned().unwrap_or_else(|| omitted(&row_start));
    let column_end = lines
        .get(3)
        .cloned()
        .unwrap_or_else(|| omitted(&column_start));
    Ok([(row_start, row_end), (column_start, column_end)])
}

/// `grid-template-areas`: quoted rows of cell names, `.` for an empty cell.
pub fn parse_template_areas(input: &str) -> Result<GridTemplateAreas, String> {
    let mut rows: Vec<Vec<&str>> = Vec::new();
    let mut rest = input.trim();
    if rest == "none" {
        return Ok(GridTemplateAreas::default());
    }
    while !rest.is_empty() {
        let quote = rest.chars().next().filter(|c| matches!(c, '"' | '\''));
        let Some(quote) = quote else {
            return Err(format!("grid-template-areas rows must be quoted, got {rest:?}"));
        };
        let body = &rest[1..];
        let end = body
            .find(quote)
            .ok_or_else(|| "unterminated grid-template-areas row".to_string())?;
        rows.push(body[..end].split_whitespace().collect());
        rest = body[end + 1..].trim_start();
    }
    let column_count = rows.first().map_or(0, Vec::len);
    if column_count == 0 || rows.iter().any(|row| row.len() != column_count) {
        return Err("every grid-template-areas row needs the same, non-zero number of cells".to_string());
    }

    let mut areas: Vec<GridTemplateArea> = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        for (column_index, name) in row.iter().enumerate() {
            if name.chars().all(|c| c == '.') {
                continue;
            }
            let (row_line, column_line) = (row_index as u16 + 1, column_index as u16 + 1);
            match areas.iter_mut().find(|area| area.name.as_ref() == *name) {
                Some(area) => {
                    area.row_start = area.row_start.min(row_line);
                    area.row_end = area.row_end.max(row_line + 1);
                    area.column_start = area.column_start.min(column_line);
                    area.column_end = area.column_end.max(column_line + 1);
                }
                None => areas.push(GridTemplateArea {
                    name: SharedString::from(name.to_string()),
                    row_start: row_line,
                    row_end: row_line + 1,
                    column_start: column_line,
                    column_end: column_line + 1,
                }),
            }
        }
    }
    for area in &areas {
        for row_index in area.row_start..area.row_end {
            for column_index in area.column_start..area.column_end {
                if rows[row_index as usize - 1][column_index as usize - 1] != area.name.as_ref() {
                    return Err(format!("grid area {:?} is not a rectangle", area.name));
                }
            }
        }
    }
    Ok(GridTemplateAreas {
        areas,
        row_count: rows.len() as u16,
        column_count: column_count as u16,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(min: GridTrackSize, max: GridTrackSize) -> GridTrack {
        GridTrack { min, max }
    }

    /// `1em` is 20px, `1rem` 10px and the viewport 800x600.
    fn parse_template(text: &str) -> Result<GridTemplateTracks, String> {
        super::parse_template(text, &LengthContext::for_tests()).map(|parsed| parsed.value)
    }

    #[test]
    fn parses_fixed_flexible_and_intrinsic_tracks() {
        let template = parse_template("100px 1fr 25% auto min-content max-content").unwrap();
        let sizes: Vec<_> = template
            .entries
            .iter()
            .map(|entry| match entry {
                GridTemplateEntry::Track(track) => *track,
                GridTemplateEntry::Repeat { .. } => panic!("no repeat expected"),
            })
            .collect();
        assert_eq!(
            sizes,
            vec![
                track(GridTrackSize::Length(100.0), GridTrackSize::Length(100.0)),
                track(GridTrackSize::Auto, GridTrackSize::Fr(1.0)),
                track(GridTrackSize::Percent(0.25), GridTrackSize::Percent(0.25)),
                track(GridTrackSize::Auto, GridTrackSize::Auto),
                track(GridTrackSize::MinContent, GridTrackSize::MinContent),
                track(GridTrackSize::MaxContent, GridTrackSize::MaxContent),
            ]
        );
        assert_eq!(template.line_names.len(), template.entries.len() + 1);
    }

    #[test]
    fn parses_minmax_and_fit_content() {
        let template = parse_template("minmax(100px, 1fr) fit-content(40%)").unwrap();
        assert_eq!(
            template.entries,
            vec![
                GridTemplateEntry::Track(track(
                    GridTrackSize::Length(100.0),
                    GridTrackSize::Fr(1.0)
                )),
                GridTemplateEntry::Track(track(
                    GridTrackSize::Auto,
                    GridTrackSize::FitContentPercent(0.4)
                )),
            ]
        );
    }

    #[test]
    fn parses_repeat_forms() {
        let template =
            parse_template("repeat(3, 1fr) repeat(auto-fill, minmax(120px, 1fr))").unwrap();
        assert!(matches!(
            &template.entries[0],
            GridTemplateEntry::Repeat { repetition: GridRepetition::Count(3), tracks, .. } if tracks.len() == 1
        ));
        assert!(matches!(
            &template.entries[1],
            GridTemplateEntry::Repeat { repetition: GridRepetition::AutoFill, .. }
        ));
        assert!(parse_template("repeat(auto-fit, 100px) repeat(auto-fill, 100px)").is_err());
        assert!(parse_template("repeat(2, repeat(2, 1fr))").is_err());
    }

    #[test]
    fn aligns_line_names_with_tracks() {
        let template = parse_template("[a b] 1fr 2fr [c]").unwrap();
        let names: Vec<Vec<&str>> = template
            .line_names
            .iter()
            .map(|set| set.iter().map(|name| name.as_ref()).collect())
            .collect();
        assert_eq!(names, vec![vec!["a", "b"], vec![], vec!["c"]]);
    }

    #[test]
    fn none_is_an_empty_template() {
        assert_eq!(parse_template("none").unwrap(), GridTemplateTracks::default());
    }

    #[test]
    fn rejects_invalid_templates() {
        assert!(parse_template("").is_err());
        assert!(parse_template("minmax(1fr, 100px)").is_err());
        assert!(parse_template("-5px").is_err());
        assert!(parse_template("blah").is_err());
    }

    fn only_track(text: &str) -> GridTrack {
        match parse_template(text).unwrap().entries.as_slice() {
            [GridTemplateEntry::Track(track)] => *track,
            entries => panic!("{text} should be one track, got {entries:?}"),
        }
    }

    fn lengths(text: &str) -> GridTrackSize {
        let track = only_track(text);
        assert_eq!(track.min, track.max, "{text}");
        track.min
    }

    #[test]
    fn relative_units_resolve_in_the_context_inside_repeat_and_minmax() {
        let template = parse_template("repeat(2, 10em) minmax(5rem, 1fr)").unwrap();
        assert_eq!(
            template.entries,
            vec![
                GridTemplateEntry::Repeat {
                    repetition: GridRepetition::Count(2),
                    tracks: vec![track(GridTrackSize::Length(200.0), GridTrackSize::Length(200.0))],
                    line_names: vec![vec![], vec![]],
                },
                GridTemplateEntry::Track(track(GridTrackSize::Length(50.0), GridTrackSize::Fr(1.0))),
            ]
        );
    }

    #[test]
    fn every_relative_unit_is_a_length() {
        assert_eq!(lengths("2em"), GridTrackSize::Length(40.0));
        assert_eq!(lengths("1.5rem"), GridTrackSize::Length(15.0));
        assert_eq!(lengths("10vw"), GridTrackSize::Length(80.0));
        assert_eq!(lengths("10vh"), GridTrackSize::Length(60.0));
        assert_eq!(lengths("10vmin"), GridTrackSize::Length(60.0));
        assert_eq!(lengths("10vmax"), GridTrackSize::Length(80.0));
        assert_eq!(lengths("1in"), GridTrackSize::Length(96.0));
    }

    #[test]
    fn a_calc_without_a_percentage_is_a_length() {
        assert_eq!(lengths("calc(50vw - 20px)"), GridTrackSize::Length(380.0));
        assert_eq!(lengths("calc(1em + 2rem)"), GridTrackSize::Length(40.0));
        assert_eq!(lengths("calc(2 * (1em + 5px))"), GridTrackSize::Length(50.0));
        assert_eq!(lengths("calc(100% / 4)"), GridTrackSize::Percent(0.25));
        assert_eq!(lengths("calc(25% + 25%)"), GridTrackSize::Percent(0.5));
    }

    #[test]
    fn a_calc_that_comes_out_negative_clamps_to_zero() {
        assert_eq!(lengths("calc(1em - 30px)"), GridTrackSize::Length(0.0));
    }

    #[test]
    fn a_calc_of_a_length_and_a_percentage_is_a_calc_track() {
        assert_eq!(
            lengths("calc(100% - 20px)"),
            GridTrackSize::Calc { length: -20.0, fraction: 1.0 }
        );
        assert_eq!(
            lengths("calc(10% + 1em)"),
            GridTrackSize::Calc { length: 20.0, fraction: 0.1 }
        );
        assert_eq!(
            only_track("minmax(calc(50% - 1rem), 1fr)"),
            track(GridTrackSize::Calc { length: -10.0, fraction: 0.5 }, GridTrackSize::Fr(1.0))
        );
    }

    #[test]
    fn a_calc_is_one_track_whatever_its_spaces() {
        let template = parse_template("calc( 1em  +  2px ) [x] calc(100%-0px)");
        assert!(template.is_err(), "calc(100%-0px) is not a valid sum");
        let template = parse_template("calc( 1em  +  2px ) [x] 1fr").unwrap();
        assert_eq!(template.entries.len(), 2);
    }

    #[test]
    fn fit_content_takes_relative_lengths_but_not_a_mix() {
        assert_eq!(
            only_track("fit-content(5em)"),
            track(GridTrackSize::Auto, GridTrackSize::FitContentLength(100.0))
        );
        assert_eq!(
            only_track("fit-content(calc(10vw + 1px))"),
            track(GridTrackSize::Auto, GridTrackSize::FitContentLength(81.0))
        );
        assert!(parse_template("fit-content(calc(100% - 1px))").is_err());
    }

    #[test]
    fn auto_tracks_take_relative_lengths() {
        let tracks = parse_auto_tracks("2rem minmax(1em, calc(10vh))", &LengthContext::for_tests())
            .unwrap()
            .value;
        assert_eq!(
            tracks,
            vec![
                track(GridTrackSize::Length(20.0), GridTrackSize::Length(20.0)),
                track(GridTrackSize::Length(20.0), GridTrackSize::Length(60.0)),
            ]
        );
    }

    #[test]
    fn a_list_is_relative_when_a_length_depends_on_the_context() {
        let relative = |text: &str| parse_template_in(text, &LengthContext::for_tests()).relative;
        for text in ["10em", "1rem", "5vw", "5vh", "5vmin", "5vmax", "repeat(2, minmax(1em, 1fr))", "calc(100% - 1em)"] {
            assert!(relative(text), "{text}");
        }
        for text in ["10px", "50%", "1fr", "auto", "1in", "calc(100% - 20px)", "calc(1em - 1em)", "none", "repeat(auto-fill, 20px)"] {
            assert!(!relative(text), "{text}");
        }
        let auto = |text: &str| parse_auto_tracks(text, &LengthContext::for_tests()).unwrap().relative;
        assert!(auto("1em") && !auto("1px"));
    }

    fn parse_template_in(text: &str, context: &LengthContext) -> Parsed<GridTemplateTracks> {
        super::parse_template(text, context).unwrap()
    }

    #[test]
    fn validity_does_not_depend_on_the_context() {
        let contexts = [
            LengthContext::for_tests(),
            LengthContext::initial(0.0, 0.0),
            LengthContext::initial(1.0e6, 3.0),
        ];
        let texts = [
            "10em",
            "repeat(2, 10em)",
            "minmax(5rem, 1fr)",
            "calc(50vw - 20px)",
            "calc(1em - 1000px)",
            "calc(100% - 1em)",
            "fit-content(calc(1em - 1000px))",
            "fit-content(calc(100% - 1em))",
            "-1em",
            "calc(-1em)",
            "calc(0px - 1em)",
            "minmax(1fr, 1em)",
            "calc(1em",
            "5emm",
        ];
        for text in texts {
            let validity: Vec<bool> = contexts
                .iter()
                .map(|context| super::parse_template(text, context).is_ok())
                .collect();
            assert!(
                validity.iter().all(|valid| *valid == validity[0]),
                "{text} is {validity:?} across contexts"
            );
        }
    }

    #[test]
    fn rejects_invalid_relative_forms() {
        for text in [
            "-1em",
            "calc(-1em)",
            "calc(0px - 1em)",
            "-5rem",
            "5emm",
            "5 em",
            "calc(1em",
            "calc(1em +)",
            "calc(1fr)",
            "calc(10px * 2px)",
            "minmax(1fr, 1em)",
            "minmax(1em)",
            "repeat(2, calc(1em)",
            "5furlongs",
        ] {
            assert!(parse_template(text).is_err(), "{text} should be invalid");
        }
        assert!(parse_auto_tracks("-1em", &LengthContext::for_tests()).is_err());
    }

    #[test]
    fn parses_lines() {
        assert_eq!(parse_line("3").unwrap(), GridPlacement::Line(3));
        assert_eq!(parse_line("-1").unwrap(), GridPlacement::Line(-1));
        assert_eq!(parse_line("span 2").unwrap(), GridPlacement::Span(2));
        assert_eq!(parse_line("auto").unwrap(), GridPlacement::Auto);
        assert_eq!(
            parse_line("main").unwrap(),
            GridPlacement::NamedLine("main".into(), 0)
        );
        assert_eq!(
            parse_line("2 col").unwrap(),
            GridPlacement::NamedLine("col".into(), 2)
        );
        assert_eq!(
            parse_line("span col").unwrap(),
            GridPlacement::NamedSpan("col".into(), 1)
        );
        assert!(parse_line("0").is_err());
        assert!(parse_line("span 0").is_err());
    }

    #[test]
    fn parses_line_pairs() {
        assert_eq!(
            parse_line_pair("1 / span 2").unwrap(),
            (GridPlacement::Line(1), GridPlacement::Span(2))
        );
        assert_eq!(
            parse_line_pair("2").unwrap(),
            (GridPlacement::Line(2), GridPlacement::Auto)
        );
        assert_eq!(
            parse_line_pair("main").unwrap(),
            (
                GridPlacement::NamedLine("main".into(), 0),
                GridPlacement::NamedLine("main".into(), 0)
            )
        );
    }

    #[test]
    fn parses_grid_area() {
        let [row, column] = parse_area("main").unwrap();
        assert_eq!(row.0, GridPlacement::NamedLine("main".into(), 0));
        assert_eq!(column.1, GridPlacement::NamedLine("main".into(), 0));

        let [row, column] = parse_area("1 / 2 / 3 / 4").unwrap();
        assert_eq!(row, (GridPlacement::Line(1), GridPlacement::Line(3)));
        assert_eq!(column, (GridPlacement::Line(2), GridPlacement::Line(4)));
    }

    #[test]
    fn parses_template_areas() {
        let areas = parse_template_areas(r#""header header" "side main""#).unwrap();
        assert_eq!((areas.row_count, areas.column_count), (2, 2));
        let header = areas.areas.iter().find(|a| a.name.as_ref() == "header").unwrap();
        assert_eq!(
            (header.row_start, header.row_end, header.column_start, header.column_end),
            (1, 2, 1, 3)
        );
        assert!(parse_template_areas(r#""a b" "b a""#).is_err());
        assert!(parse_template_areas(r#""a a" "a""#).is_err());
        assert!(parse_template_areas("a a").is_err());
    }
}
