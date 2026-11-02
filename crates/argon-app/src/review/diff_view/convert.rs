use argon_core::Side;
use gpui_kit::component::diff::{DiffLinePosition, DiffLineRange, DiffSide};

pub fn kit_side(side: Side) -> DiffSide {
    match side {
        Side::Left => DiffSide::Original,
        Side::Right => DiffSide::Modified,
    }
}

pub fn app_side(side: DiffSide) -> Side {
    match side {
        DiffSide::Original => Side::Left,
        DiffSide::Modified => Side::Right,
    }
}

pub fn position(path: &str, (line, side): (u32, Side)) -> DiffLinePosition {
    DiffLinePosition::new(path.to_string(), kit_side(side), line as usize)
}

pub fn range(path: &str, start: (u32, Side), end: (u32, Side)) -> DiffLineRange {
    DiffLineRange::new(
        path.to_string(),
        kit_side(start.1),
        start.0 as usize,
        end.0 as usize,
    )
    .with_end_side(kit_side(end.1))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GutterRange {
    pub path: String,
    pub line: u32,
    pub side: Side,
    pub start: Option<(u32, Side)>,
}

pub fn gutter_range(range: &DiffLineRange) -> GutterRange {
    let first = (range.start() as u32, app_side(range.side()));
    let last = (range.end() as u32, app_side(range.end_side()));
    let (start, end) = if first.1 == last.1 && first.0 > last.0 {
        (last, first)
    } else {
        (first, last)
    };
    GutterRange {
        path: range.path().to_string(),
        line: end.0,
        side: end.1,
        start: (start != end).then_some(start),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_has_no_start() {
        let range = range("a.rs", (7, Side::Right), (7, Side::Right));
        assert_eq!(
            gutter_range(&range),
            GutterRange {
                path: "a.rs".to_string(),
                line: 7,
                side: Side::Right,
                start: None,
            }
        );
    }

    #[test]
    fn upward_drag_on_one_side_is_normalized() {
        let range = range("a.rs", (12, Side::Left), (9, Side::Left));
        let gutter = gutter_range(&range);
        assert_eq!(gutter.line, 12);
        assert_eq!(gutter.side, Side::Left);
        assert_eq!(gutter.start, Some((9, Side::Left)));
    }

    #[test]
    fn cross_side_range_keeps_both_sides() {
        let range = range("a.rs", (3, Side::Left), (5, Side::Right));
        let gutter = gutter_range(&range);
        assert_eq!(gutter.line, 5);
        assert_eq!(gutter.side, Side::Right);
        assert_eq!(gutter.start, Some((3, Side::Left)));
    }

    #[test]
    fn positions_map_sides() {
        let position = position("a.rs", (4, Side::Left));
        assert_eq!(position.side(), DiffSide::Original);
        assert_eq!(position.line(), 4);
        assert_eq!(app_side(position.side()), Side::Left);
    }
}
