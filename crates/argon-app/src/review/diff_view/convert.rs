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

pub fn gutter_range(
    range: &DiffLineRange,
    order: impl Fn((u32, Side)) -> Option<usize>,
) -> GutterRange {
    let first = (range.start() as u32, app_side(range.side()));
    let last = (range.end() as u32, app_side(range.end_side()));
    let (start, end) = match (order(first), order(last)) {
        (Some(a), Some(b)) if b < a => (last, first),
        _ => (first, last),
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

    fn unified_order(anchor: (u32, Side)) -> Option<usize> {
        [
            ((8, Side::Left), 0),
            ((8, Side::Right), 0),
            ((9, Side::Left), 1),
            ((10, Side::Left), 2),
            ((9, Side::Right), 3),
            ((10, Side::Right), 4),
            ((11, Side::Right), 5),
            ((12, Side::Right), 6),
        ]
        .into_iter()
        .find_map(|(key, ix)| (key == anchor).then_some(ix))
    }

    #[test]
    fn single_line_has_no_start() {
        let range = range("a.rs", (9, Side::Right), (9, Side::Right));
        let gutter = gutter_range(&range, unified_order);
        assert_eq!(
            gutter,
            GutterRange {
                path: "a.rs".to_string(),
                line: 9,
                side: Side::Right,
                start: None,
            }
        );
    }

    #[test]
    fn upward_unified_drag_across_sides_starts_at_the_upper_line() {
        let range = range("a.rs", (12, Side::Right), (9, Side::Left));
        let gutter = gutter_range(&range, unified_order);
        assert_eq!(gutter.start, Some((9, Side::Left)));
        assert_eq!((gutter.line, gutter.side), (12, Side::Right));
    }

    #[test]
    fn upward_split_drag_on_each_side_is_ordered() {
        let left = gutter_range(
            &range("a.rs", (10, Side::Left), (8, Side::Left)),
            unified_order,
        );
        assert_eq!(left.start, Some((8, Side::Left)));
        assert_eq!((left.line, left.side), (10, Side::Left));
        let right = gutter_range(
            &range("a.rs", (12, Side::Right), (9, Side::Right)),
            unified_order,
        );
        assert_eq!(right.start, Some((9, Side::Right)));
        assert_eq!((right.line, right.side), (12, Side::Right));
    }

    #[test]
    fn downward_drag_keeps_its_direction() {
        let gutter = gutter_range(
            &range("a.rs", (9, Side::Left), (11, Side::Right)),
            unified_order,
        );
        assert_eq!(gutter.start, Some((9, Side::Left)));
        assert_eq!((gutter.line, gutter.side), (11, Side::Right));
    }

    #[test]
    fn positions_map_sides() {
        let position = position("a.rs", (4, Side::Left));
        assert_eq!(position.side(), DiffSide::Original);
        assert_eq!(position.line(), 4);
        assert_eq!(app_side(position.side()), Side::Left);
    }
}
