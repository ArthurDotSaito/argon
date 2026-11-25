use serde::{Deserialize, Serialize};

use crate::ui::columns::COLUMN_GAP;

pub const SUBJECT_MIN_WIDTH: f32 = 220.;
const MAX_WIDTH: f32 = 1200.;
const HIDE_ORDER: [Column; 6] = [
    Column::Comments,
    Column::Size,
    Column::People,
    Column::Signal,
    Column::Updated,
    Column::Reason,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Reason,
    Signal,
    People,
    Comments,
    Size,
    Updated,
}

impl Column {
    pub const ALL: [Column; 6] = [
        Column::Reason,
        Column::Signal,
        Column::People,
        Column::Comments,
        Column::Size,
        Column::Updated,
    ];

    pub fn default_width(self) -> f32 {
        match self {
            Column::Reason => 284.,
            Column::Signal => 100.,
            Column::People => 56.,
            Column::Comments => 44.,
            Column::Size => 92.,
            Column::Updated => 66.,
        }
    }

    pub fn min_width(self) -> f32 {
        match self {
            Column::Reason => 80.,
            Column::Signal => 56.,
            Column::People => 40.,
            Column::Comments => 36.,
            Column::Size => 56.,
            Column::Updated => 48.,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnWidths {
    pub reason: f32,
    pub signal: f32,
    pub people: f32,
    pub comments: f32,
    pub size: f32,
    pub updated: f32,
}

impl Default for ColumnWidths {
    fn default() -> Self {
        Self {
            reason: Column::Reason.default_width(),
            signal: Column::Signal.default_width(),
            people: Column::People.default_width(),
            comments: Column::Comments.default_width(),
            size: Column::Size.default_width(),
            updated: Column::Updated.default_width(),
        }
    }
}

impl ColumnWidths {
    pub fn get(&self, column: Column) -> f32 {
        match column {
            Column::Reason => self.reason,
            Column::Signal => self.signal,
            Column::People => self.people,
            Column::Comments => self.comments,
            Column::Size => self.size,
            Column::Updated => self.updated,
        }
    }

    pub fn with(mut self, column: Column, width: f32) -> ColumnWidths {
        let slot = match column {
            Column::Reason => &mut self.reason,
            Column::Signal => &mut self.signal,
            Column::People => &mut self.people,
            Column::Comments => &mut self.comments,
            Column::Size => &mut self.size,
            Column::Updated => &mut self.updated,
        };
        *slot = width;
        self
    }

    pub fn clamped(self) -> ColumnWidths {
        Column::ALL.into_iter().fold(self, |widths, column| {
            let width = widths.get(column).max(column.min_width()).min(MAX_WIDTH);
            widths.with(column, width)
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnLayout([Option<f32>; 6]);

impl Default for ColumnLayout {
    fn default() -> Self {
        Self::showing_all(&ColumnWidths::default())
    }
}

impl ColumnLayout {
    pub fn width(&self, column: Column) -> Option<f32> {
        self.0[column.index()]
    }

    fn showing_all(widths: &ColumnWidths) -> ColumnLayout {
        ColumnLayout(Column::ALL.map(|column| Some(widths.get(column))))
    }

    fn required(&self, width_of: impl Fn(Column, f32) -> f32) -> f32 {
        Column::ALL
            .into_iter()
            .filter_map(|column| Some(COLUMN_GAP + width_of(column, self.width(column)?)))
            .sum::<f32>()
            + SUBJECT_MIN_WIDTH
    }
}

pub fn fit(available: Option<f32>, widths: &ColumnWidths) -> ColumnLayout {
    let mut layout = ColumnLayout::showing_all(widths);
    let Some(available) = available else {
        return layout;
    };
    let reason_min = Column::Reason.min_width();
    let with_reason = |reason: f32| {
        move |column: Column, width: f32| {
            if column == Column::Reason {
                reason
            } else {
                width
            }
        }
    };
    let mut excess = layout.required(with_reason(reason_min)) - available;
    for column in HIDE_ORDER {
        if excess <= 0. {
            break;
        }
        if column == Column::Reason {
            continue;
        }
        let width = widths.get(column);
        let given = (width - column.default_width()).clamp(0., excess);
        layout.0[column.index()] = Some(width - given);
        excess -= given;
    }
    let mut hide = HIDE_ORDER.into_iter();
    while layout.required(with_reason(reason_min)) > available {
        let Some(column) = hide.next() else {
            break;
        };
        layout.0[column.index()] = None;
    }
    if layout.width(Column::Reason).is_some() {
        let spare = available - layout.required(with_reason(0.));
        layout.0[Column::Reason.index()] = Some(widths.reason.min(spare).max(reason_min));
    }
    layout
}

pub fn resized(
    widths: ColumnWidths,
    column: Column,
    width: f32,
    available: Option<f32>,
) -> ColumnWidths {
    let max = available.map_or(MAX_WIDTH, |available| {
        let others = fit(Some(available), &widths)
            .required(|other, shown| if other == column { 0. } else { shown });
        (available - others).min(MAX_WIDTH)
    });
    widths.with(column, width.min(max).max(column.min_width()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_COLUMNS: f32 = 284. + 100. + 56. + 44. + 92. + 66.;
    const ALL_GAPS: f32 = COLUMN_GAP * 6.;

    fn visible(layout: &ColumnLayout) -> Vec<Column> {
        Column::ALL
            .into_iter()
            .filter(|column| layout.width(*column).is_some())
            .collect()
    }

    fn subject_width(available: f32, layout: &ColumnLayout) -> f32 {
        available - layout.required(|_, width| width) + SUBJECT_MIN_WIDTH
    }

    #[test]
    fn unmeasured_list_shows_every_column_at_its_chosen_width() {
        let widths = ColumnWidths::default().with(Column::Size, 120.);
        let layout = fit(None, &widths);
        assert_eq!(layout.width(Column::Size), Some(120.));
        assert_eq!(visible(&layout), Column::ALL);
        assert_eq!(ColumnLayout::default(), fit(None, &ColumnWidths::default()));
    }

    #[test]
    fn wide_list_keeps_every_chosen_width() {
        let available = SUBJECT_MIN_WIDTH + DEFAULT_COLUMNS + ALL_GAPS + 300.;
        let layout = fit(Some(available), &ColumnWidths::default());
        assert_eq!(layout, ColumnLayout::default());
        assert_eq!(subject_width(available, &layout), SUBJECT_MIN_WIDTH + 300.);
    }

    #[test]
    fn reason_gives_way_before_any_column_hides() {
        let available = SUBJECT_MIN_WIDTH + DEFAULT_COLUMNS + ALL_GAPS - 100.;
        let layout = fit(Some(available), &ColumnWidths::default());
        assert_eq!(visible(&layout), Column::ALL);
        assert_eq!(layout.width(Column::Reason), Some(184.));
        assert_eq!(subject_width(available, &layout), SUBJECT_MIN_WIDTH);
    }

    #[test]
    fn columns_hide_in_priority_order_as_the_list_narrows() {
        let widths = ColumnWidths::default();
        let mut hidden = Vec::new();
        let mut available = 900.;
        while available > 0. {
            let layout = fit(Some(available), &widths);
            for column in Column::ALL {
                if layout.width(column).is_none() && !hidden.contains(&column) {
                    hidden.push(column);
                }
            }
            available -= 1.;
        }
        assert_eq!(hidden, HIDE_ORDER);
    }

    #[test]
    fn reason_takes_back_the_room_a_hidden_column_frees() {
        let all_at_reason_min = SUBJECT_MIN_WIDTH + DEFAULT_COLUMNS - 284. + 80. + ALL_GAPS;
        let layout = fit(Some(all_at_reason_min - 1.), &ColumnWidths::default());
        assert_eq!(layout.width(Column::Comments), None);
        assert_eq!(
            layout.width(Column::Reason),
            Some(80. + 44. + COLUMN_GAP - 1.)
        );
    }

    #[test]
    fn oversized_column_gives_back_its_extra_width_before_anything_hides() {
        let widths = ColumnWidths::default().with(Column::Updated, 600.);
        let available = 786.;
        let layout = fit(Some(available), &widths);
        assert_eq!(visible(&layout), Column::ALL);
        assert_eq!(layout.width(Column::Updated), Some(122.));
        assert_eq!(layout.width(Column::Reason), Some(80.));
        assert_eq!(subject_width(available, &layout), SUBJECT_MIN_WIDTH);
    }

    #[test]
    fn oversized_columns_hide_like_default_ones_once_back_at_their_defaults() {
        let widths = ColumnWidths::default()
            .with(Column::Updated, 600.)
            .with(Column::Signal, 400.);
        for available in [300., 420., 560., 700.] {
            assert_eq!(
                visible(&fit(Some(available), &widths)),
                visible(&fit(Some(available), &ColumnWidths::default())),
                "at {available}"
            );
        }
    }

    #[test]
    fn subject_keeps_its_minimum_while_any_column_is_visible() {
        let widths = ColumnWidths::default();
        let mut available = 1200.;
        while available > SUBJECT_MIN_WIDTH {
            let layout = fit(Some(available), &widths);
            assert!(
                subject_width(available, &layout) >= SUBJECT_MIN_WIDTH,
                "subject squeezed at {available}"
            );
            available -= 7.;
        }
    }

    #[test]
    fn list_narrower_than_the_subject_hides_everything() {
        let layout = fit(Some(SUBJECT_MIN_WIDTH - 40.), &ColumnWidths::default());
        assert!(visible(&layout).is_empty());
    }

    #[test]
    fn resize_stops_at_the_column_minimum() {
        let widths = resized(ColumnWidths::default(), Column::Size, 3., Some(1400.));
        assert_eq!(widths.size, Column::Size.min_width());
    }

    #[test]
    fn resize_never_squeezes_the_subject_below_its_minimum() {
        let available = SUBJECT_MIN_WIDTH + DEFAULT_COLUMNS + ALL_GAPS + 50.;
        let widths = resized(
            ColumnWidths::default(),
            Column::Signal,
            900.,
            Some(available),
        );
        assert_eq!(widths.signal, 150.);
        let layout = fit(Some(available), &widths);
        assert_eq!(visible(&layout), Column::ALL);
        assert_eq!(layout.width(Column::Reason), Some(284.));
        assert_eq!(subject_width(available, &layout), SUBJECT_MIN_WIDTH);
    }

    #[test]
    fn squeezed_reason_cannot_grow_past_the_width_on_screen() {
        let available = SUBJECT_MIN_WIDTH + DEFAULT_COLUMNS + ALL_GAPS - 100.;
        let widths = resized(
            ColumnWidths::default(),
            Column::Reason,
            400.,
            Some(available),
        );
        assert_eq!(widths.reason, 184.);
    }

    #[test]
    fn unmeasured_resize_only_respects_the_limits() {
        let widths = resized(ColumnWidths::default(), Column::Reason, 5000., None);
        assert_eq!(widths.reason, MAX_WIDTH);
    }

    #[test]
    fn saved_widths_tolerate_missing_and_out_of_range_values() {
        let widths: ColumnWidths =
            serde_json::from_str(r#"{"reason": 10, "size": 99999, "signal": 130}"#).unwrap();
        let widths = widths.clamped();
        assert_eq!(widths.reason, Column::Reason.min_width());
        assert_eq!(widths.size, MAX_WIDTH);
        assert_eq!(widths.signal, 130.);
        assert_eq!(widths.updated, Column::Updated.default_width());
    }

    #[test]
    fn widths_round_trip_through_json() {
        let widths = ColumnWidths::default().with(Column::People, 72.);
        let json = serde_json::to_string(&widths).unwrap();
        assert_eq!(serde_json::from_str::<ColumnWidths>(&json).unwrap(), widths);
    }
}
