//! Breaking laid-out paragraphs into pages.
//!
//! Works on the continuous Normal-view layout: each paragraph's lines with
//! their heights. A page holds lines until the next one would overflow the
//! text height, with the original's rules on top: manual page breaks, Page Break
//! Before, Keep Lines Together, Keep With Next, and widow control (no
//! paragraph leaves a single line alone at the bottom or top of a page).

use crate::layout::ParaLayout;
use quill_core::SectionStart;

/// Where a page starts: the paragraph and line, and the y in the
/// continuous layout (the paragraph's top, space before included, when the
/// page starts with a paragraph's first line).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageStart {
    pub para: usize,
    pub line: usize,
    pub y: f32,
    /// The page follows a manual page break.
    pub manual: bool,
}

/// The paragraph settings pagination cares about.
#[derive(Clone, Copy, Debug, Default)]
pub struct Keeps {
    pub page_break_before: bool,
    pub keep_with_next: bool,
    pub keep_together: bool,
}

/// One thing pagination places: a line of a paragraph, or a whole table
/// row (which never splits across pages).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Unit {
    pub para: usize,
    pub line: usize,
    pub top: f32,
    pub bottom: f32,
    /// The paragraph's first and last lines (both, for a row).
    pub first: bool,
    pub last: bool,
    /// The line ends with a manual page break.
    pub hard_break: bool,
    /// Room the footnotes referenced on this line need on its page.
    pub notes: f32,
}

/// A paragraph's lines as units, the paragraph's top at `top`.
pub fn line_units(para: usize, l: &ParaLayout, top: f32) -> Vec<Unit> {
    let n = l.lines.len();
    l.lines
        .iter()
        .enumerate()
        .map(|(li, line)| Unit {
            para,
            line: li,
            top: if li == 0 { top } else { top + line.top },
            // A line holding only a page break takes no room on its page,
            // so a break at the foot of a page cannot push itself onto the
            // next page and leave one blank.
            bottom: if line.hard_break && line.end - line.start == 1 {
                top + line.top
            } else {
                top + line.top + line.height
            },
            first: li == 0,
            last: li + 1 == n,
            hard_break: line.hard_break,
            notes: 0.0,
        })
        .collect()
}

/// Paginate paragraphs laid out at `tops` (continuous y of each paragraph,
/// as many as `layouts`) into pages `height` points tall.
pub fn paginate(
    layouts: &[&ParaLayout],
    tops: &[f32],
    keeps: &[Keeps],
    height: f32,
    widow_control: bool,
) -> Vec<PageStart> {
    let units: Vec<Unit> = layouts
        .iter()
        .enumerate()
        .flat_map(|(p, l)| line_units(p, l, tops[p]))
        .collect();
    paginate_units(&units, keeps, height, widow_control)
}

/// Paginate units in order down the column into pages `height` tall.
pub fn paginate_units(
    units: &[Unit],
    keeps: &[Keeps],
    height: f32,
    widow_control: bool,
) -> Vec<PageStart> {
    let one = [SectionLayout::default()];
    let bands = paginate_sections(units, keeps, &one, &[height], widow_control, 0.0);
    page_starts(&bands)
}

/// What pagination needs to know about a section: where it starts in the
/// text, how it starts on the page and its columns.
#[derive(Clone, Copy, Debug)]
pub struct SectionLayout {
    pub first_para: usize,
    pub start: SectionStart,
    pub columns: usize,
    pub column_width: f32,
    pub spacing: f32,
}

impl Default for SectionLayout {
    fn default() -> Self {
        SectionLayout {
            first_para: 0,
            start: SectionStart::NewPage,
            columns: 1,
            column_width: 0.0,
            spacing: 0.0,
        }
    }
}

/// One column's share of the continuous layout: `y..end`, put on page
/// `page` at (`x`, `top`) points from the top left corner of its text
/// area, `w` wide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub y: f32,
    pub end: f32,
    pub page: usize,
    pub section: usize,
    pub x: f32,
    pub top: f32,
    pub w: f32,
    /// The first line's paragraph and line.
    pub para: usize,
    pub line: usize,
    /// The band follows a manual page break.
    pub manual: bool,
    unit: usize,
}

/// Why a column ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    /// Nothing more fits.
    Full,
    /// A page break, or a paragraph with Page Break Before.
    Page,
    /// The units ran out.
    Done,
}

/// Fill a column `height` tall with units from `first`, stopping at
/// `end`, keeping room for the footnotes of the lines it holds (and
/// `note_sep` above them). Returns the first unit of the next column and
/// why it stopped.
fn column_end(
    units: &[Unit],
    keeps: &[Keeps],
    first: usize,
    end: usize,
    height: f32,
    widow_control: bool,
    note_sep: f32,
) -> (usize, End) {
    let first_of = |i: usize| {
        let p = units[i].para;
        let mut j = i;
        while j > 0 && units[j - 1].para == p {
            j -= 1;
        }
        j
    };
    let top = units[first].top;
    let mut notes = 0.0;
    for i in first..end {
        let u = &units[i];
        let keep = keeps.get(u.para).copied().unwrap_or_default();
        if u.first && keep.page_break_before && i > first {
            return (i, End::Page);
        }
        notes += u.notes;
        let sep = if notes > 0.0 { note_sep } else { 0.0 };
        if u.bottom - top + notes + sep > height && i > first {
            let mut b = i;
            let para_first = first_of(i);
            let lines_in_para = units[para_first..]
                .iter()
                .take_while(|x| x.para == u.para)
                .count();
            if keep.keep_together && !u.first {
                b = para_first;
            } else if widow_control && lines_in_para > 1 {
                if u.line == 1 {
                    // Don't leave the first line alone (an orphan).
                    b = para_first;
                } else if u.last && u.line >= 2 {
                    // Take a line along so the last one is not alone.
                    b = i - 1;
                }
            }
            // Paragraphs kept with this one move too.
            if units[b].first {
                while b > 0 {
                    let prev = units[b - 1].para;
                    let pk = keeps.get(prev).copied().unwrap_or_default();
                    if !pk.keep_with_next {
                        break;
                    }
                    let f = first_of(b - 1);
                    if f <= first {
                        break;
                    }
                    b = f;
                }
            }
            if b <= first {
                b = i;
            }
            return (b, End::Full);
        }
        if u.hard_break && i + 1 < units.len() {
            return (i + 1, End::Page);
        }
    }
    (end, End::Done)
}

/// Lay units out in their sections' columns. `page_heights` gives the
/// available body height of each page; its last value is reused for later
/// pages. A section that starts without a break goes on below what is on
/// the page already, and the columns of the section before it are balanced,
/// as in the original.
pub fn paginate_sections(
    units: &[Unit],
    keeps: &[Keeps],
    sections: &[SectionLayout],
    page_heights: &[f32],
    widow_control: bool,
    note_sep: f32,
) -> Vec<Band> {
    let page_height = |page: usize| {
        page_heights
            .get(page)
            .or_else(|| page_heights.last())
            .copied()
            .unwrap_or(36.0)
            .max(36.0)
    };
    let mut bands: Vec<Band> = Vec::new();
    let band = |i: usize, end: usize, page, section, x, top, w| Band {
        y: units[i].top,
        end: if end < units.len() {
            units[end].top
        } else {
            units[end - 1].bottom
        },
        page,
        section,
        x,
        top,
        w,
        para: units[i].para,
        line: units[i].line,
        manual: i > 0 && units[i - 1].hard_break,
        unit: i,
    };
    if units.is_empty() {
        return vec![Band {
            y: 0.0,
            end: 0.0,
            page: 0,
            section: 0,
            x: 0.0,
            top: 0.0,
            w: 0.0,
            para: 0,
            line: 0,
            manual: false,
            unit: 0,
        }];
    }
    let (mut page, mut col, mut top) = (0usize, 0usize, 0.0f32);
    let mut last = End::Done;
    let mut i = 0;
    for (s, sec) in sections.iter().enumerate() {
        let end = sections.get(s + 1).map_or(units.len(), |n| {
            units.partition_point(|u| u.para < n.first_para)
        });
        if i >= end {
            continue;
        }
        let cols = sec.columns.max(1);
        let stride = sec.column_width + sec.spacing;
        if !bands.is_empty() {
            let start = if last == End::Page {
                SectionStart::NewPage
            } else {
                sec.start
            };
            match start {
                SectionStart::NoBreak => {
                    let prev = &sections[s - 1];
                    if prev.columns > 1 {
                        balance(
                            &mut bands,
                            units,
                            keeps,
                            prev,
                            i,
                            page,
                            page_height(page),
                            widow_control,
                            note_sep,
                        );
                    }
                    top = bands
                        .iter()
                        .filter(|b| b.page == page)
                        .map(|b| b.top + b.end - b.y)
                        .fold(top, f32::max);
                    col = 0;
                    if top > 0.0 && units[i].bottom - units[i].top > page_height(page) - top {
                        (page, top) = (page + 1, 0.0);
                    }
                }
                SectionStart::NewColumn if col + 1 < cols => col += 1,
                SectionStart::NewColumn | SectionStart::NewPage => {
                    (page, col, top) = (page + 1, 0, 0.0)
                }
                SectionStart::EvenPage | SectionStart::OddPage => {
                    // Page index 0 is page 1, an odd page.
                    let want = usize::from(sec.start == SectionStart::EvenPage);
                    page += 1;
                    if page % 2 != want {
                        page += 1;
                    }
                    (col, top) = (0, 0.0);
                }
            }
        }
        loop {
            let (next, why) = column_end(
                units,
                keeps,
                i,
                end,
                page_height(page) - top,
                widow_control,
                note_sep,
            );
            bands.push(band(
                i,
                next,
                page,
                s,
                col as f32 * stride,
                top,
                sec.column_width,
            ));
            i = next;
            last = why;
            if i >= end {
                break;
            }
            if why == End::Full && col + 1 < cols {
                col += 1;
            } else {
                (page, col, top) = (page + 1, 0, 0.0);
            }
        }
    }
    bands
}

#[allow(clippy::too_many_arguments)]
/// Even out the columns of `sec` on `page`, the last bands in `bands`, by
/// finding the shortest height that still holds them all.
fn balance(
    bands: &mut Vec<Band>,
    units: &[Unit],
    keeps: &[Keeps],
    sec: &SectionLayout,
    end: usize,
    page: usize,
    height: f32,
    widow_control: bool,
    note_sep: f32,
) {
    let Some(last) = bands.last().copied() else {
        return;
    };
    let k = bands
        .iter()
        .rposition(|b| b.page != page || b.section != last.section)
        .map_or(0, |k| k + 1);
    let first = bands[k];
    let room = height - first.top;
    let cols = sec.columns.max(1);
    let fill = |h: f32| {
        let mut out = Vec::new();
        let mut j = first.unit;
        for _ in 0..cols {
            let (next, why) = column_end(units, keeps, j, end, h, widow_control, note_sep);
            out.push((j, next));
            j = next;
            if j >= end || why == End::Page {
                break;
            }
        }
        (j >= end).then_some(out)
    };
    let total = last.end - first.y;
    let (mut lo, mut hi) = (total / cols as f32, room);
    if fill(lo).is_none() {
        for _ in 0..16 {
            let mid = (lo + hi) / 2.0;
            if fill(mid).is_some() {
                hi = mid;
            } else {
                lo = mid;
            }
        }
    } else {
        hi = lo;
    }
    let Some(cols_out) = fill(hi) else {
        return;
    };
    bands.truncate(k);
    let stride = sec.column_width + sec.spacing;
    for (c, (a, b)) in cols_out.into_iter().enumerate() {
        let mut band = first;
        band.y = units[a].top;
        band.end = if b < units.len() {
            units[b].top
        } else {
            units[b - 1].bottom
        };
        band.x = c as f32 * stride;
        band.para = units[a].para;
        band.line = units[a].line;
        band.manual = a > 0 && units[a - 1].hard_break;
        band.unit = a;
        bands.push(band);
    }
}

/// The first band on each page, as page starts. A page left blank to
/// start a section on an even or odd page starts where the next one does.
pub fn page_starts(bands: &[Band]) -> Vec<PageStart> {
    let pages = bands.last().map_or(1, |b| b.page + 1);
    (0..pages)
        .map(|k| {
            let b = bands
                .iter()
                .find(|b| b.page >= k)
                .copied()
                .unwrap_or(bands[bands.len() - 1]);
            PageStart {
                para: b.para,
                line: b.line,
                y: b.y,
                manual: b.page == k && b.manual,
            }
        })
        .collect()
}

/// The page holding continuous y `y`.
pub fn page_at(pages: &[PageStart], y: f32) -> usize {
    pages.partition_point(|p| p.y <= y).saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Line;

    /// A paragraph of `n` lines, each 10 points tall.
    fn para(n: usize) -> ParaLayout {
        let mut l = ParaLayout::default();
        for k in 0..n {
            l.lines.push(Line {
                first: 0,
                last: 0,
                start: 0,
                end: 0,
                top: 10.0 * k as f32,
                height: 10.0,
                baseline: 8.0,
                end_x: 0.0,
                left: 0.0,
                hard_break: false,
                changed: false,
            });
        }
        l.height = 10.0 * n as f32;
        l
    }

    fn run(paras: &[ParaLayout], keeps: &[Keeps], height: f32) -> Vec<(usize, usize)> {
        let mut tops = Vec::new();
        let mut y = 0.0;
        for p in paras {
            tops.push(y);
            y += p.height;
        }
        let refs: Vec<&ParaLayout> = paras.iter().collect();
        paginate(&refs, &tops, keeps, height, true)
            .iter()
            .map(|p| (p.para, p.line))
            .collect()
    }

    #[test]
    fn breaks_when_a_line_does_not_fit() {
        let paras: Vec<_> = (0..10).map(|_| para(1)).collect();
        assert_eq!(run(&paras, &[], 35.0), [(0, 0), (3, 0), (6, 0), (9, 0)]);
    }

    #[test]
    fn pages_can_have_different_body_heights() {
        let paras: Vec<_> = (0..14).map(|_| para(1)).collect();
        let mut tops = Vec::new();
        let mut y = 0.0;
        for para in &paras {
            tops.push(y);
            y += para.height;
        }
        let units: Vec<_> = paras
            .iter()
            .enumerate()
            .flat_map(|(i, layout)| line_units(i, layout, tops[i]))
            .collect();
        let bands = paginate_sections(
            &units,
            &[],
            &[SectionLayout::default()],
            &[40.0, 60.0],
            true,
            0.0,
        );
        assert_eq!(
            bands.iter().map(|band| band.para).collect::<Vec<_>>(),
            [0, 4, 10]
        );
    }

    #[test]
    fn widow_control_keeps_two_lines_together() {
        // Five lines on a four-line page: the last two go over.
        assert_eq!(run(&[para(5)], &[], 40.0), [(0, 0), (0, 3)]);
        // A paragraph starting on the last line moves whole.
        let paras = [para(3), para(3)];
        assert_eq!(run(&paras, &[], 40.0), [(0, 0), (1, 0)]);
    }

    #[test]
    fn keeps_and_page_break_before() {
        let mut keeps = vec![Keeps::default(); 3];
        keeps[1].keep_together = true;
        let paras = [para(2), para(3), para(1)];
        assert_eq!(run(&paras, &keeps, 40.0), [(0, 0), (1, 0)]);

        // A heading (para 1) kept with the paragraph after it, which only
        // gets one line on the page: both move.
        let mut keeps = vec![Keeps::default(); 3];
        keeps[1].keep_with_next = true;
        let paras = [para(2), para(1), para(3)];
        assert_eq!(run(&paras, &keeps, 40.0), [(0, 0), (1, 0)]);

        let mut keeps = vec![Keeps::default(); 2];
        keeps[1].page_break_before = true;
        assert_eq!(run(&[para(1), para(1)], &keeps, 100.0), [(0, 0), (1, 0)]);
    }

    #[test]
    fn manual_page_breaks() {
        let mut p = para(1);
        p.lines[0].hard_break = true;
        assert_eq!(run(&[p, para(1)], &[], 100.0), [(0, 0), (1, 0)]);
    }

    #[test]
    fn finds_the_page_for_a_y() {
        let pages = [
            PageStart {
                para: 0,
                line: 0,
                y: 0.0,
                manual: false,
            },
            PageStart {
                para: 3,
                line: 0,
                y: 30.0,
                manual: false,
            },
        ];
        assert_eq!(page_at(&pages, 0.0), 0);
        assert_eq!(page_at(&pages, 29.0), 0);
        assert_eq!(page_at(&pages, 30.0), 1);
        assert_eq!(page_at(&pages, 300.0), 1);
    }

    fn sections(paras: &[ParaLayout], secs: &[SectionLayout], height: f32) -> Vec<Band> {
        let mut tops = Vec::new();
        let mut y = 0.0;
        for p in paras {
            tops.push(y);
            y += p.height;
        }
        let units: Vec<Unit> = paras
            .iter()
            .enumerate()
            .flat_map(|(i, l)| line_units(i, l, tops[i]))
            .collect();
        paginate_sections(&units, &[], secs, &[height], true, 0.0)
    }

    fn cols(first_para: usize, start: SectionStart, columns: usize) -> SectionLayout {
        SectionLayout {
            first_para,
            start,
            columns,
            column_width: 100.0,
            spacing: 20.0,
        }
    }

    #[test]
    fn columns_fill_side_by_side() {
        let paras: Vec<_> = (0..10).map(|_| para(1)).collect();
        let b = sections(&paras, &[cols(0, SectionStart::NewPage, 2)], 40.0);
        let at: Vec<_> = b.iter().map(|b| (b.page, b.x, b.para)).collect();
        assert_eq!(at, [(0, 0.0, 0), (0, 120.0, 4), (1, 0.0, 8)]);
    }

    #[test]
    fn continuous_section_balances_the_columns_above() {
        // Six lines in two columns, then a one-column section with no
        // break: three lines a column, and the new section below them.
        let paras: Vec<_> = (0..8).map(|_| para(1)).collect();
        let secs = [
            cols(0, SectionStart::NewPage, 2),
            cols(6, SectionStart::NoBreak, 1),
        ];
        let b = sections(&paras, &secs, 100.0);
        let at: Vec<_> = b.iter().map(|b| (b.page, b.x, b.top, b.para)).collect();
        assert_eq!(
            at,
            [(0, 0.0, 0.0, 0), (0, 120.0, 0.0, 3), (0, 0.0, 30.0, 6)]
        );
    }

    #[test]
    fn odd_page_sections_leave_a_blank_page() {
        let paras: Vec<_> = (0..2).map(|_| para(1)).collect();
        let secs = [
            cols(0, SectionStart::NewPage, 1),
            cols(1, SectionStart::OddPage, 1),
        ];
        let b = sections(&paras, &secs, 100.0);
        assert_eq!(b.iter().map(|b| b.page).collect::<Vec<_>>(), [0, 2]);
        let pages = page_starts(&b);
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[1].y, pages[2].y);
    }
}
