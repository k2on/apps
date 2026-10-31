//! An outline: a [`crate::table`] whose rows are gathered under group
//! headings, whose cursor walks the rows alone, and whose rows and groups
//! are dragged — or shifted from the keyboard — into a new order.
//!
//! The rule is the sidebar's ([`crate::sidebar`]): **a heading is drawn, not
//! stored as a row.** The cursor's `n` counts rows only, so it is the reader's
//! `n` too, and the keyboard walks a plain column ([`crate::vim::Grid::column`]
//! over [`rows`]) — `j` off a group's last row lands on the next group's
//! first, `5j` crosses headings without counting them, `gg` is the first row
//! of the first group. A folded group is a heading with no rows, so the
//! cursor never lands inside it.
//!
//! **Every line is one height** (`line`, the caller's), heading or row, so a
//! drag is [`crate::reorder`]'s arithmetic over the lines as drawn: a row
//! dragged is one line among headings and rows, and lands under whichever
//! heading is above where it is let go ([`Drag::drop`]). A heading dragged
//! takes its group with it; while it is held every group is drawn folded,
//! headings alone, so what moves is one line among lines again.
//!
//! The order is the app's. A drop, or a [`shift`], says what goes where, by
//! row and group numbers as they were drawn; the app moves its records.
//!
//! The zebra runs on across headings, the way it would down one table: a
//! stripe says "the next row", and a heading is not one.
use iced::time::Instant;
use iced::widget::{column, container, mouse_area, rule};
use iced::{Element, Length};

use crate::reorder::{self, Reorder, Settle};
use crate::{style, table};

/// One group: its heading, the rows under it, and whatever is drawn after
/// them that is not a row (a field a new row is typed into).
pub struct Group<'a, R, M> {
    pub head: Element<'a, M>,
    /// Empty when the group is folded.
    pub rows: Vec<R>,
    pub foot: Option<Element<'a, M>>,
}

/// How many rows the cursor can be on: every group's, headings not counted.
pub fn rows<R, M>(groups: &[Group<'_, R, M>]) -> usize {
    groups.iter().map(|g| g.rows.len()).sum()
}

/// What a press is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grab {
    Row(usize),
    Group(usize),
}

/// A line as drawn: a group's heading, or a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Line {
    Head(usize),
    Row(usize),
}

fn lines(sizes: &[usize]) -> Vec<Line> {
    let mut out = Vec::new();
    let mut n = 0;
    for (g, &size) in sizes.iter().enumerate() {
        out.push(Line::Head(g));
        out.extend((n..n + size).map(Line::Row));
        n += size;
    }
    out
}

/// Which group row `n` is in, by each group's row count.
pub fn group_of(sizes: &[usize], n: usize) -> Option<usize> {
    let mut start = 0;
    for (g, &size) in sizes.iter().enumerate() {
        if n < start + size {
            return Some(g);
        }
        start += size;
    }
    None
}

/// The rows of group `g`, as row numbers.
pub fn rows_of(sizes: &[usize], g: usize) -> std::ops::Range<usize> {
    let start: usize = sizes[..g.min(sizes.len())].iter().sum();
    start..start + sizes.get(g).copied().unwrap_or(0)
}

/// A press on a row or a heading, and the drag it may become.
#[derive(Debug, Clone, PartialEq)]
pub struct Drag {
    grab: Grab,
    reorder: Reorder,
}

/// What letting go did, in the numbers rows and groups were drawn with.
#[derive(Debug, Clone, PartialEq)]
pub enum Dropped {
    /// It never moved far enough to be a drag.
    Click(Grab),
    /// Let go where it was.
    Stayed,
    /// Row `row` goes into group `group`, in front of row `before` (one of
    /// that group's) or at its end. `settle` carries it the rest of the way
    /// once the outline is drawn in the new order.
    Row {
        row: usize,
        group: usize,
        before: Option<usize>,
        settle: Settle,
    },
    /// Group `group` goes in front of group `before`, or after every other.
    Group { group: usize, before: Option<usize> },
}

impl Drag {
    /// A press on `grab`, the pointer at `y`, among groups of `sizes` rows,
    /// each line `line` tall.
    pub fn press(grab: Grab, sizes: &[usize], line: f32, y: f32, now: Instant) -> Drag {
        let reorder = match grab {
            Grab::Row(n) => {
                let all = lines(sizes);
                let at = all.iter().position(|l| *l == Line::Row(n)).unwrap_or(0);
                Reorder::press(at, all.len(), line, y, now)
            }
            Grab::Group(g) => Reorder::press(g, sizes.len(), line, y, now),
        };
        Drag { grab, reorder }
    }

    pub fn grab(&self) -> Grab {
        self.grab
    }

    /// Whether the press has become a drag.
    pub fn dragging(&self) -> bool {
        self.reorder.dragging()
    }

    /// The pointer moved to `y`.
    pub fn drag(&mut self, y: f32, now: Instant) {
        self.reorder.drag(y, now);
    }

    /// Whether anything is still sliding out of the way.
    pub fn moving(&self, now: Instant) -> bool {
        self.reorder.moving(now)
    }

    /// Let go, among the same `sizes` it was pressed among.
    pub fn drop(&self, sizes: &[usize], now: Instant) -> Dropped {
        let (from, to, settle) = match self.reorder.drop(now) {
            reorder::Drop::Click(_) => return Dropped::Click(self.grab),
            reorder::Drop::Moved { from, to, settle } => (from, to, settle),
        };
        if from == to {
            return Dropped::Stayed;
        }
        match self.grab {
            Grab::Group(g) => {
                let order = reorder::moved(&(0..sizes.len()).collect::<Vec<_>>(), from, to);
                let at = order.iter().position(|&o| o == g).unwrap_or(0);
                Dropped::Group {
                    group: g,
                    before: order.get(at + 1).copied(),
                }
            }
            Grab::Row(n) => {
                let mut order = reorder::moved(&lines(sizes), from, to);
                // Above the first heading is the first group's top.
                if order.first() == Some(&Line::Row(n)) {
                    order.swap(0, 1);
                }
                let at = order.iter().position(|l| *l == Line::Row(n)).unwrap_or(0);
                let group = order[..at]
                    .iter()
                    .rev()
                    .find_map(|l| match l {
                        Line::Head(g) => Some(*g),
                        Line::Row(_) => None,
                    })
                    .unwrap_or(0);
                let before = match order.get(at + 1) {
                    Some(Line::Row(r)) => Some(*r),
                    _ => None,
                };
                Dropped::Row {
                    row: n,
                    group,
                    before,
                    settle,
                }
            }
        }
    }
}

/// How far down its scrollable row `n` sits, 0 to 1, for
/// [`crate::scroll::reveal_to`]: by lines drawn, headings counted.
pub fn progress(sizes: &[usize], n: usize) -> f32 {
    let all = lines(sizes);
    let at = all.iter().position(|l| *l == Line::Row(n)).unwrap_or(0);
    match all.len() {
        0 | 1 => 0.0,
        len => at as f32 / (len - 1) as f32,
    }
}

/// What a keyboard shift of rows `first..=last` one place `down` (or up)
/// does, by the numbers they were drawn with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shift {
    /// Nothing is that way.
    Refused,
    /// The rows go into `group`, in front of row `before` or at its end,
    /// in their order.
    Rows { group: usize, before: Option<usize> },
    /// The rows are every row of groups `first..=last`, so the groups move:
    /// in front of group `before`, or after every other.
    Groups { first: usize, last: usize, before: Option<usize> },
}

/// Rows `first..=last` one place down (or up), among groups of `sizes`
/// rows: past the next row in their group, or off its end into the top of
/// the next group (off its top into the end of the one before). When they
/// are every row of whole groups, the groups move past their neighbour —
/// select a group's rows and it is the group that moves.
pub fn shift(sizes: &[usize], first: usize, last: usize, down: bool) -> Shift {
    let (Some(g), Some(h)) = (group_of(sizes, first), group_of(sizes, last)) else {
        return Shift::Refused;
    };
    if rows_of(sizes, g).start == first && rows_of(sizes, h).end == last + 1 {
        return groups(sizes.len(), g, h, down);
    }
    match down {
        true => {
            let in_group = rows_of(sizes, h);
            if last + 1 < in_group.end {
                // Past the next row: in front of the one after it, if it is
                // in the same group.
                let before = (last + 2 < in_group.end).then_some(last + 2);
                Shift::Rows { group: h, before }
            } else if h + 1 < sizes.len() {
                let next = rows_of(sizes, h + 1);
                Shift::Rows {
                    group: h + 1,
                    before: (!next.is_empty()).then_some(next.start),
                }
            } else {
                Shift::Refused
            }
        }
        false => {
            let in_group = rows_of(sizes, g);
            if first > in_group.start {
                Shift::Rows {
                    group: g,
                    before: Some(first - 1),
                }
            } else if g > 0 {
                Shift::Rows { group: g - 1, before: None }
            } else {
                Shift::Refused
            }
        }
    }
}

/// Groups `first..=last` of `count` one place down (or up).
pub fn groups(count: usize, first: usize, last: usize, down: bool) -> Shift {
    match down {
        true if last + 1 < count => Shift::Groups {
            first,
            last,
            before: (last + 2 < count).then_some(last + 2),
        },
        false if first > 0 => Shift::Groups {
            first,
            last,
            before: Some(first - 1),
        },
        _ => Shift::Refused,
    }
}

/// How an outline is drawn: every line's height, and what tells one row
/// from the next — a zebra, as [`crate::table`] draws, or a hairline under
/// each (counted in its line, so a line is still `line` tall).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub line: f32,
    pub zebra: bool,
    pub rules: bool,
}

/// The outline, drawn as `look` says.
///
/// `lit` is the rows under the cursor, first to last — one, or a visual
/// selection. While the outline has the keyboard (`focused`) they are
/// filled with the accent and `row` is told so, so its text asks
/// [`crate::style::on_row`]; without it they are marked, as a table's
/// cursor is ([`table::row`]). A press on a row or a heading is `grab`'s —
/// the app makes it a [`Drag`] and hears whether it was a click when it is
/// let go. A right press on one is `menu`'s. `drag` and `settle` are the
/// ones in progress.
#[allow(clippy::too_many_arguments)]
pub fn view<'a, R, M: Clone + 'a>(
    groups: Vec<Group<'a, R, M>>,
    lit: Option<(usize, usize)>,
    focused: bool,
    look: Look,
    drag: Option<&Drag>,
    settle: Option<&Settle>,
    now: Instant,
    grab: impl Fn(Grab) -> M,
    menu: impl Fn(Grab) -> M,
    row: impl Fn(R, bool) -> Element<'a, M>,
) -> Element<'a, M> {
    let line = look.line;
    let held = drag.filter(|d| d.dragging());
    let settling = settle.filter(|s| s.moving(now));
    let folded = held.is_some_and(|d| matches!(d.grab, Grab::Group(_)));
    let mut drawn: Vec<Element<'a, M>> = Vec::new();
    let mut n = 0;
    for (g, group) in groups.into_iter().enumerate() {
        drawn.push(
            mouse_area(container(group.head).width(Length::Fill).height(Length::Fixed(line)))
                .on_press(grab(Grab::Group(g)))
                .on_right_press(menu(Grab::Group(g)))
                .into(),
        );
        if folded {
            continue;
        }
        for r in group.rows {
            let on_cursor = lit.is_some_and(|(a, b)| a <= n && n <= b);
            let body = row(r, on_cursor && focused);
            let height = if look.rules { line - 1.0 } else { line };
            let painted = table::row(body, on_cursor, focused, look.zebra && n % 2 == 1).center_y(Length::Fixed(height));
            let drawn_row: Element<'a, M> = match look.rules {
                true => column![painted, rule::horizontal(1).style(style::hairline)].into(),
                false => painted.into(),
            };
            drawn.push(
                mouse_area(drawn_row)
                    .on_press(grab(Grab::Row(n)))
                    .on_right_press(menu(Grab::Row(n)))
                    .into(),
            );
            n += 1;
        }
        // A field being typed into is not a line a drag counts.
        if let (Some(foot), None, None) = (group.foot, held, settling) {
            drawn.push(foot);
        }
    }
    match (held, settling) {
        (None, None) => column(drawn).into(),
        (held, settling) => reorder::list(drawn, line, held.map(|d| &d.reorder), settling, now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dropped(grab: Grab, sizes: &[usize], by: f32) -> Dropped {
        let now = Instant::now();
        let mut d = Drag::press(grab, sizes, 10.0, 100.0, now);
        d.drag(100.0 + by, now);
        d.drop(sizes, now)
    }

    /// Two groups of two: `A r0 r1 B r2 r3`, lines ten tall.
    const TWO: [usize; 2] = [2, 2];

    #[test]
    fn a_press_that_never_moves_is_a_click() {
        assert_eq!(dropped(Grab::Row(1), &TWO, 0.0), Dropped::Click(Grab::Row(1)));
        assert_eq!(dropped(Grab::Row(1), &TWO, 2.0), Dropped::Click(Grab::Row(1)));
    }

    /// A row lands under whichever heading is above where it is let go, in
    /// front of the row below it there.
    #[test]
    fn a_row_lands_under_the_heading_above_it() {
        let row = |d: Dropped| match d {
            Dropped::Row { row, group, before, .. } => (row, group, before),
            other => panic!("{other:?}"),
        };
        assert_eq!(row(dropped(Grab::Row(1), &TWO, -10.0)), (1, 0, Some(0)), "up one, in its group");
        assert_eq!(row(dropped(Grab::Row(1), &TWO, 10.0)), (1, 1, Some(2)), "down past the heading");
        assert_eq!(row(dropped(Grab::Row(0), &TWO, 50.0)), (0, 1, None), "to the end");
        assert_eq!(
            row(dropped(Grab::Row(2), &TWO, -10.0)),
            (2, 0, None),
            "up past its heading, to the first group's end"
        );
        assert_eq!(row(dropped(Grab::Row(2), &TWO, -40.0)), (2, 0, Some(0)), "above every heading is the top");
        assert_eq!(row(dropped(Grab::Row(2), &TWO, 1000.0)), (2, 1, None), "past the end, held at it");
    }

    #[test]
    fn a_heading_moves_its_group() {
        assert_eq!(dropped(Grab::Group(0), &TWO, 10.0), Dropped::Group { group: 0, before: None });
        assert_eq!(dropped(Grab::Group(1), &TWO, -10.0), Dropped::Group { group: 1, before: Some(0) });
        assert_eq!(dropped(Grab::Group(1), &TWO, 10.0), Dropped::Stayed, "the last stays last");
    }

    /// Shifted rows pass the next row in their group, then cross into the
    /// next group's top; shifted up, its end. Every row of a group is the
    /// group.
    #[test]
    fn shifting_rows_and_groups() {
        let three = [3, 2];
        assert_eq!(shift(&three, 0, 0, true), Shift::Rows { group: 0, before: Some(2) });
        assert_eq!(
            shift(&three, 1, 1, true),
            Shift::Rows { group: 0, before: None },
            "past the last, its group's end"
        );
        assert_eq!(
            shift(&three, 2, 2, true),
            Shift::Rows { group: 1, before: Some(3) },
            "off the end, the next one's top"
        );
        assert_eq!(shift(&three, 4, 4, true), Shift::Refused);
        assert_eq!(
            shift(&three, 3, 3, false),
            Shift::Rows { group: 0, before: None },
            "off the top, the one before's end"
        );
        assert_eq!(shift(&three, 1, 2, false), Shift::Rows { group: 0, before: Some(0) });
        assert_eq!(shift(&three, 0, 0, false), Shift::Refused);
        assert_eq!(
            shift(&three, 0, 2, true),
            Shift::Groups {
                first: 0,
                last: 0,
                before: None
            }
        );
        assert_eq!(
            shift(&three, 3, 4, false),
            Shift::Groups {
                first: 1,
                last: 1,
                before: Some(0)
            }
        );
        assert_eq!(shift(&three, 0, 4, true), Shift::Refused, "every group, nowhere to go");
        assert_eq!(
            shift(&[1, 0, 1], 0, 0, true),
            Shift::Groups {
                first: 0,
                last: 0,
                before: Some(2)
            },
            "a folded group is passed like any other"
        );
    }

    /// A row's place counts the headings above it: the first row of the
    /// second group of two and two is the fifth line of six.
    #[test]
    fn headings_take_their_line() {
        assert_eq!((progress(&TWO, 0), progress(&TWO, 2), progress(&TWO, 3)), (0.2, 0.8, 1.0));
        assert_eq!(progress(&[], 0), 0.0);
    }

    #[test]
    fn rows_know_their_group() {
        assert_eq!((group_of(&TWO, 1), group_of(&TWO, 2), group_of(&TWO, 4)), (Some(0), Some(1), None));
        assert_eq!((rows_of(&TWO, 1), rows_of(&[0, 2], 0)), (2..4, 0..0));
    }
}
