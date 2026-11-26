//! A hidden native text view that mirrors the document for VoiceOver.
//!
//! Quill draws its own document, so the Mac sees an empty rectangle. This
//! puts a real, invisible `NSTextView` over the document with the same text
//! and selection. It never takes the mouse or the keyboard: all editing
//! still happens in Quill's own view. Its only job is to give VoiceOver, Voice
//! Control and the other accessibility tools a text area they already
//! understand, with the caret, lines, words and ranges that AppKit provides.
//!
//! The mirror's own layout (its font, its wrapping, never scrolled) has
//! nothing to do with what Quill draws, so every question about lines and
//! positions is answered from Quill's layout instead. That is what puts
//! VoiceOver's box over the words being read.
//!
//! Keeping the copy current costs a pass over the whole document after each
//! edit, so the mirror holds the text only while VoiceOver is running.
//! Without VoiceOver it stays empty, as the document area was before.

use crate::docview::DocView;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{ClassType, DeclaredClass, declare_class, msg_send, msg_send_id, mutability, sel};
use objc2_app_kit::{NSTextView, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRange, NSRect, NSSize, NSString};
use std::cell::RefCell;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

declare_class!(
    struct MirrorView;

    unsafe impl ClassType for MirrorView {
        type Super = NSTextView;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "QuillMirrorTextView";
    }

    impl DeclaredClass for MirrorView {
        type Ivars = RefCell<Geometry>;
    }

    unsafe impl MirrorView {
        // Clicks go through to Quill's own view underneath.
        #[method(hitTest:)]
        fn hit_test(&self, _point: NSPoint) -> *mut NSView {
            std::ptr::null_mut()
        }

        // The keyboard stays with Quill's view.
        #[method(acceptsFirstResponder)]
        fn accepts_first_responder(&self) -> bool {
            false
        }

        // To accessibility it is the focused text whenever Quill's view has
        // the keyboard, as a native text view would be.
        #[method(isAccessibilityFocused)]
        fn is_accessibility_focused(&self) -> bool {
            // SAFETY: plain AppKit getters on the main thread.
            unsafe {
                let window: *mut AnyObject = msg_send![self, window];
                let parent: *mut AnyObject = msg_send![self, superview];
                !window.is_null() && !parent.is_null() && {
                    let responder: *mut AnyObject = msg_send![window, firstResponder];
                    let key: bool = msg_send![window, isKeyWindow];
                    key && responder == parent
                }
            }
        }

        // VoiceOver moving its text cursor sets the selection here. Keep it
        // for Quill, which moves its own caret on the next redraw and sends
        // the selection back in the usual way.
        #[method(setAccessibilitySelectedTextRange:)]
        fn set_accessibility_selected_text_range(&self, range: NSRange) {
            self.ivars().borrow_mut().request = Some(range);
            // SAFETY: main-thread messages to live views.
            unsafe {
                let _: () = msg_send![super(self), setAccessibilitySelectedTextRange: range];
                let parent: *mut AnyObject = msg_send![self, superview];
                if !parent.is_null() {
                    let _: () = msg_send![parent, setNeedsDisplay: true];
                }
            }
        }

        #[method(accessibilityInsertionPointLineNumber)]
        fn insertion_point_line(&self) -> isize {
            let g = self.ivars().borrow();
            g.line_of(g.caret).map_or(0, |line| line as isize)
        }

        #[method(accessibilityLineForIndex:)]
        fn line_for_index(&self, index: isize) -> isize {
            let g = self.ivars().borrow();
            g.line_of(index.max(0) as usize).map_or(0, |line| line as isize)
        }

        #[method(accessibilityRangeForLine:)]
        fn range_for_line(&self, line: isize) -> NSRange {
            let g = self.ivars().borrow();
            match usize::try_from(line).ok().and_then(|line| g.lines.get(line)) {
                Some(line) => NSRange::new(line.start, line.end - line.start),
                None => NSRange::new(0, 0),
            }
        }

        #[method(accessibilityFrameForRange:)]
        fn frame_for_range(&self, range: NSRange) -> NSRect {
            let rect = self.ivars().borrow().rect(range.location, range.length);
            rect.map_or(NSRect::ZERO, |rect| self.view_to_screen(rect))
        }

        #[method(accessibilityRangeForPosition:)]
        fn range_for_position(&self, point: NSPoint) -> NSRange {
            let point = self.screen_to_view(point);
            let g = self.ivars().borrow();
            match g.index_at(point.x, point.y) {
                Some(index) if index < g.len => NSRange::new(index, 1),
                Some(index) => NSRange::new(index, 0),
                None => NSRange::new(0, 0),
            }
        }

        #[method(accessibilityVisibleCharacterRange)]
        fn visible_character_range(&self) -> NSRange {
            let (start, end) = self.ivars().borrow().visible();
            NSRange::new(start, end - start)
        }
    }
);

impl MirrorView {
    /// A rectangle in Quill's view (top-left origin, points) on the screen.
    fn view_to_screen(&self, rect: NSRect) -> NSRect {
        // SAFETY: main-thread AppKit conversions on live views.
        unsafe {
            let parent: *mut AnyObject = msg_send![self, superview];
            let window: *mut AnyObject = msg_send![self, window];
            if parent.is_null() || window.is_null() {
                return NSRect::ZERO;
            }
            let nil: *mut AnyObject = std::ptr::null_mut();
            let in_window: NSRect = msg_send![parent, convertRect: rect, toView: nil];
            msg_send![window, convertRectToScreen: in_window]
        }
    }

    /// A screen point in Quill's view (top-left origin, points).
    fn screen_to_view(&self, point: NSPoint) -> NSPoint {
        // SAFETY: main-thread AppKit conversions on live views.
        unsafe {
            let parent: *mut AnyObject = msg_send![self, superview];
            let window: *mut AnyObject = msg_send![self, window];
            if parent.is_null() || window.is_null() {
                return NSPoint::new(f64::NAN, f64::NAN);
            }
            let nil: *mut AnyObject = std::ptr::null_mut();
            let in_window: NSPoint = msg_send![window, convertPointFromScreen: point];
            msg_send![parent, convertPoint: in_window, fromView: nil]
        }
    }
}

/// One of Quill's laid-out lines, in the mirror's UTF-16 positions and in
/// Quill's view coordinates before scrolling and zoom (see `Geometry`).
#[derive(Debug, Default, PartialEq)]
struct MirrorLine {
    start: usize,
    /// Past the paragraph's line break on a paragraph's last line.
    end: usize,
    /// The end of the line's own text.
    text_end: usize,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    /// The x of each position from `start` to `text_end`. Empty for lines
    /// far from view, which then answer with the whole line.
    xs: Vec<f64>,
}

/// What the mirror knows of Quill's layout. The lines are kept until the
/// layout changes; scrolling and zooming change only `shift` and `zoom`.
#[derive(Debug)]
struct Geometry {
    lines: Vec<MirrorLine>,
    /// Where each paragraph's lines start in `lines`, and the end.
    first_line: Vec<usize>,
    /// The lines that have `xs` just now.
    filled: Vec<usize>,
    /// The document area, in points in Quill's view.
    area: NSRect,
    /// Added to a line's position for window coordinates: the text area's
    /// corner less the scroll.
    shift: (f64, f64),
    /// The document zoom, from window coordinates to points.
    zoom: f64,
    /// The caret, as a UTF-16 position.
    caret: usize,
    /// Length of the mirror's text in UTF-16 units.
    len: usize,
    /// A selection VoiceOver asked for, not yet passed to Quill.
    request: Option<NSRange>,
}

impl Default for Geometry {
    fn default() -> Self {
        Geometry {
            lines: Vec::new(),
            first_line: Vec::new(),
            filled: Vec::new(),
            area: NSRect::ZERO,
            shift: (0.0, 0.0),
            zoom: 1.0,
            caret: 0,
            len: 0,
            request: None,
        }
    }
}

impl Geometry {
    /// A line position as points in Quill's view.
    fn points(&self, x: f64, y: f64) -> (f64, f64) {
        (
            (x + self.shift.0) * self.zoom,
            (y + self.shift.1) * self.zoom,
        )
    }

    /// A point in Quill's view as a line position.
    fn line_position(&self, x: f64, y: f64) -> (f64, f64) {
        let zoom = if self.zoom > 0.0 { self.zoom } else { 1.0 };
        (x / zoom - self.shift.0, y / zoom - self.shift.1)
    }

    /// The line holding UTF-16 position `index`. A position at a wrap
    /// belongs to the lower line, like the caret.
    fn line_of(&self, index: usize) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        Some(
            self.lines
                .partition_point(|line| line.start <= index)
                .saturating_sub(1),
        )
    }

    fn x_at(line: &MirrorLine, index: usize) -> f64 {
        let index = index.clamp(line.start, line.text_end);
        match line.xs.get(index - line.start) {
            Some(&x) => x,
            None if index == line.text_end && index > line.start => line.x + line.width,
            None => line.x,
        }
    }

    /// Bounds of the UTF-16 range in points in Quill's view: the union of
    /// its pieces on each line. An empty range is a thin caret.
    fn rect(&self, location: usize, length: usize) -> Option<NSRect> {
        let first = self.line_of(location)?;
        let end = location.saturating_add(length);
        // At least one point wide, whatever the zoom.
        let one = 1.0 / if self.zoom > 0.0 { self.zoom } else { 1.0 };
        let mut out: Option<(f64, f64, f64, f64)> = None;
        for line in &self.lines[first..] {
            if line.start > end || (length > 0 && line.start == end) {
                break;
            }
            let x0 = Self::x_at(line, location.max(line.start));
            let x1 = if length == 0 {
                x0 + one
            } else {
                Self::x_at(line, end.min(line.text_end)).max(x0 + one)
            };
            let piece = (x0, line.y, x1, line.y + line.height);
            out = Some(match out {
                None => piece,
                Some(o) => (
                    o.0.min(piece.0),
                    o.1.min(piece.1),
                    o.2.max(piece.2),
                    o.3.max(piece.3),
                ),
            });
            if length == 0 {
                break;
            }
        }
        let (x0, y0, x1, y1) = out?;
        let (x0, y0) = self.points(x0, y0);
        let (x1, y1) = self.points(x1, y1);
        Some(NSRect::new(
            NSPoint::new(x0, y0),
            NSSize::new(x1 - x0, y1 - y0),
        ))
    }

    /// The UTF-16 position nearest a point in Quill's view.
    fn index_at(&self, x: f64, y: f64) -> Option<usize> {
        let (x, y) = self.line_position(x, y);
        let line = self
            .lines
            .iter()
            .find(|line| (line.y..line.y + line.height).contains(&y))?;
        if line.xs.is_empty() {
            return Some(line.start);
        }
        let mut best = line.start;
        for (i, pair) in line.xs.windows(2).enumerate() {
            if x >= (pair[0] + pair[1]) / 2.0 {
                best = line.start + i + 1;
            }
        }
        Some(best)
    }

    /// The UTF-16 range of the lines that show in the document area.
    fn visible(&self) -> (usize, usize) {
        let (_, top) = self.line_position(0.0, self.area.origin.y);
        let (_, bottom) = self.line_position(0.0, self.area.origin.y + self.area.size.height);
        let mut shown = self
            .lines
            .iter()
            .filter(|line| line.y + line.height > top && line.y < bottom);
        match shown.next() {
            Some(first) => {
                let last = shown.next_back().unwrap_or(first);
                (first.start, last.end)
            }
            None => (0, 0),
        }
    }
}

/// Where a stretch of a paragraph's text starts in the mirror. A paragraph's
/// pieces start at byte 0 and run in order; each lasts until the next.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Piece {
    byte: usize,
    /// The mirror's UTF-16 position of `byte`.
    unit: usize,
    /// False for text the view hides, which the mirror leaves out.
    shown: bool,
}

/// A shown piece is split every this many UTF-16 units, so finding one
/// position never counts through more than this much text.
const PIECE_UNITS: usize = 256;

/// How the mirror's text lines up with the document's paragraphs.
#[derive(Debug, Default)]
struct MirrorMap {
    /// Where each paragraph starts in the mirror, in UTF-16 units.
    starts: Vec<usize>,
    /// Where each paragraph's pieces start in `pieces`, and the end.
    first_piece: Vec<usize>,
    pieces: Vec<Piece>,
    /// Length of the mirror's text in UTF-16 units.
    len: usize,
}

impl MirrorMap {
    /// The mirror's text for `paragraphs`, leaving out what the view hides:
    /// hidden text when `hide.0`, tracked deletions when `hide.1`. Line and
    /// page breaks inside a paragraph become plain line breaks, so they are
    /// not spoken as control characters; each is one UTF-16 unit either way.
    fn build(paragraphs: &[quill_core::Paragraph], hide: (bool, bool)) -> (String, MirrorMap) {
        use quill_core::para::chars;
        let mut text = String::new();
        let mut map = MirrorMap {
            starts: Vec::with_capacity(paragraphs.len()),
            first_piece: Vec::with_capacity(paragraphs.len() + 1),
            ..MirrorMap::default()
        };
        let mut unit = 0usize;
        for (i, paragraph) in paragraphs.iter().enumerate() {
            map.starts.push(unit);
            let first = map.pieces.len();
            map.first_piece.push(first);
            let mut since = 0;
            for (start, end, chp) in paragraph.run_bounds() {
                let shown = !((chp.hidden && hide.0)
                    || (hide.1 && quill_core::revision::is_deleted(chp.rev)));
                let last = map.pieces[first..].last().map(|p| p.shown);
                if last != Some(shown) {
                    map.pieces.push(Piece {
                        byte: start,
                        unit,
                        shown,
                    });
                    since = 0;
                }
                if !shown {
                    continue;
                }
                for (at, ch) in paragraph.text[start..end].char_indices() {
                    if since >= PIECE_UNITS {
                        map.pieces.push(Piece {
                            byte: start + at,
                            unit,
                            shown,
                        });
                        since = 0;
                    }
                    text.push(match ch {
                        chars::LINE_BREAK | chars::PAGE_BREAK => '\n',
                        ch => ch,
                    });
                    unit += ch.len_utf16();
                    since += ch.len_utf16();
                }
            }
            if map.pieces.len() == first {
                map.pieces.push(Piece {
                    byte: 0,
                    unit,
                    shown: true,
                });
            }
            if i + 1 < paragraphs.len() {
                text.push('\n');
                unit += 1;
            }
        }
        map.first_piece.push(map.pieces.len());
        map.len = unit;
        (text, map)
    }

    fn walk<'a>(&'a self, paragraphs: &'a [quill_core::Paragraph], para: usize) -> Walk<'a> {
        let pieces = &self.pieces[self.first_piece[para]..self.first_piece[para + 1]];
        Walk {
            text: &paragraphs[para].text,
            pieces,
            k: 0,
            byte: pieces[0].byte,
            unit: pieces[0].unit,
        }
    }

    /// The mirror's UTF-16 position of a document position.
    fn unit_of(&self, paragraphs: &[quill_core::Paragraph], p: quill_core::Position) -> usize {
        if self.starts.is_empty() {
            return 0;
        }
        let para = p.para.min(self.starts.len() - 1);
        self.walk(paragraphs, para).to(p.offset)
    }

    /// The document position of the mirror's UTF-16 position `index`. One
    /// inside text the view hides comes out at its start.
    fn position_of(
        &self,
        paragraphs: &[quill_core::Paragraph],
        index: usize,
    ) -> quill_core::Position {
        let para = self
            .starts
            .partition_point(|&start| start <= index)
            .saturating_sub(1);
        let text = &paragraphs[para].text;
        let pieces = &self.pieces[self.first_piece[para]..self.first_piece[para + 1]];
        let k = pieces
            .partition_point(|p| p.unit <= index)
            .saturating_sub(1);
        let piece = pieces[k];
        let end = pieces.get(k + 1).map_or(text.len(), |p| p.byte);
        if !piece.shown {
            return quill_core::Position::new(para, piece.byte);
        }
        let mut left = index.saturating_sub(piece.unit);
        let mut offset = end;
        for (at, ch) in text[piece.byte..end].char_indices() {
            if left == 0 {
                offset = piece.byte + at;
                break;
            }
            left = left.saturating_sub(ch.len_utf16());
        }
        quill_core::Position::new(para, offset)
    }
}

/// Goes through one paragraph turning byte offsets into the mirror's UTF-16
/// positions. Offsets asked for in order cost one pass over the text.
struct Walk<'a> {
    text: &'a str,
    pieces: &'a [Piece],
    /// The piece holding `byte`.
    k: usize,
    byte: usize,
    /// The mirror's UTF-16 position of `byte`.
    unit: usize,
}

impl Walk<'_> {
    fn to(&mut self, offset: usize) -> usize {
        let offset = self.text.floor_char_boundary(offset.min(self.text.len()));
        // Find the piece holding `offset`, from the current one when it is
        // ahead, from the start when it is behind.
        let from = if offset < self.byte { 0 } else { self.k };
        let k = from
            + self.pieces[from..]
                .partition_point(|p| p.byte <= offset)
                .saturating_sub(1);
        if k != self.k || offset < self.byte {
            self.k = k;
            self.byte = self.pieces[k].byte;
            self.unit = self.pieces[k].unit;
        }
        if self.pieces[k].shown {
            self.unit += self.text[self.byte..offset].encode_utf16().count();
        }
        self.byte = offset;
        self.unit
    }
}

pub struct TextMirror {
    view: Retained<MirrorView>,
    map: MirrorMap,
    /// The text generation and hiding the mirror's text was built for.
    text_key: Option<(u64, (bool, bool))>,
    /// A hash of the text the mirror holds, so a change that leaves it the
    /// same (a format, say) does not reset VoiceOver's place.
    text_hash: Option<u64>,
    /// What the line geometry was built for.
    geometry_key: Option<(u64, u32, bool, bool)>,
    /// The selection last turned into UTF-16 positions.
    selection: Option<quill_core::Selection>,
    /// The selection as a UTF-16 range, and what the view was last given.
    wanted: (usize, usize),
    range: (usize, usize),
    frame: Option<NSRect>,
}

impl TextMirror {
    pub fn new(window: &Window) -> Option<Self> {
        let handle = window.window_handle().ok()?.as_raw();
        let RawWindowHandle::AppKit(handle) = handle else {
            return None;
        };
        // Winit makes and uses its NSView on this thread.
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        let parent = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        let this = mtm
            .alloc::<MirrorView>()
            .set_ivars(RefCell::new(Geometry::default()));
        // SAFETY: a plain initWithFrame: on the freshly allocated subclass.
        let view: Retained<MirrorView> =
            unsafe { msg_send_id![super(this), initWithFrame: NSRect::ZERO] };
        // SAFETY: the view is ours and used only on the main thread.
        unsafe {
            let _: () = msg_send![&*view, setEditable: false];
            let _: () = msg_send![&*view, setSelectable: true];
            let _: () = msg_send![&*view, setDrawsBackground: false];
            // Nothing of it may show: Quill's own drawing is what people see.
            // A view at zero opacity is still there for accessibility.
            let _: () = msg_send![&*view, setAlphaValue: 0.0_f64];
            // Keep it the size of the document area instead of growing to
            // fit its text.
            let _: () = msg_send![&*view, setVerticallyResizable: false];
            let _: () = msg_send![&*view, setHorizontallyResizable: false];
            parent.addSubview(&view);
            point_focus_at_mirror(parent);
        }
        Some(Self {
            view,
            map: MirrorMap::default(),
            text_key: None,
            text_hash: None,
            geometry_key: None,
            selection: None,
            wanted: (0, 0),
            range: (0, 0),
            frame: None,
        })
    }

    /// Bring the mirror in line with the document: its frame over the text,
    /// its text, and the selection as UTF-16 positions. `zoom` is the
    /// document zoom, which turns the view's units into window points.
    ///
    /// This runs after every draw, so it rebuilds the text only when the
    /// document's text or what is hidden changes, and the lines only when
    /// the layout does. Otherwise it costs about what shows on screen.
    /// While VoiceOver is off it only empties the mirror. The caret's blink
    /// redraws a focused window, so it fills again soon after VoiceOver
    /// starts.
    pub fn sync(&mut self, view: &DocView, zoom: f32) {
        if !voice_over_on() {
            self.empty();
            return;
        }
        let area = view.area;
        let frame = NSRect::new(
            NSPoint::new((area.x * zoom) as f64, (area.y * zoom) as f64),
            NSSize::new((area.w * zoom) as f64, (area.h * zoom) as f64),
        );
        let paragraphs = &view.doc.main.paras;
        let text_key = (view.text_generation, view.mirror_hides());
        let mut new_text = None;
        if self.text_key != Some(text_key) || self.map.starts.len() != paragraphs.len() {
            let (text, map) = MirrorMap::build(paragraphs, text_key.1);
            self.map = map;
            self.text_key = Some(text_key);
            self.geometry_key = None;
            self.selection = None;
            let mut hasher = std::hash::DefaultHasher::new();
            std::hash::Hash::hash(&text, &mut hasher);
            let hash = std::hash::Hasher::finish(&hasher);
            if self.text_hash != Some(hash) {
                self.text_hash = Some(hash);
                new_text = Some(text);
            }
        }
        let shift = view.mirror_shift();
        {
            let mut g = self.view.ivars().borrow_mut();
            let basis = view.mirror_basis();
            if self.geometry_key != Some(basis) {
                let (lines, first_line) = mirror_lines(view, &self.map);
                g.lines = lines;
                g.first_line = first_line;
                g.filled.clear();
                self.geometry_key = Some(basis);
            }
            g.area = frame;
            g.shift = (shift.0 as f64, shift.1 as f64);
            g.zoom = zoom as f64;
            g.len = self.map.len;
            fill_glyph_positions(&mut g, view, &self.map);
        }
        if self.selection != Some(view.sel) {
            let a = self.map.unit_of(paragraphs, view.sel.anchor);
            let b = self.map.unit_of(paragraphs, view.sel.focus);
            self.view.ivars().borrow_mut().caret = b;
            self.selection = Some(view.sel);
            self.wanted = (a.min(b), a.max(b) - a.min(b));
        }
        // SAFETY: main-thread messages to our own live view.
        unsafe {
            if self.frame != Some(frame) {
                let _: () = msg_send![&*self.view, setFrame: frame];
                self.frame = Some(frame);
            }
            if let Some(text) = new_text {
                let string = NSString::from_str(&text);
                let _: () = msg_send![&*self.view, setString: &*string];
                self.range = (usize::MAX, 0);
            }
            let range = self.wanted;
            if range != self.range {
                let _: () = msg_send![
                    &*self.view,
                    setSelectedRange: NSRange::new(range.0, range.1)
                ];
                self.range = range;
            }
        }
    }
}

impl TextMirror {
    /// Drop the mirror's text and lines, once, so it holds no copy of the
    /// document. The next sync with VoiceOver on builds them again.
    fn empty(&mut self) {
        if self.text_key.is_none() && self.text_hash.is_none() {
            return;
        }
        self.map = MirrorMap::default();
        self.text_key = None;
        self.text_hash = None;
        self.geometry_key = None;
        self.selection = None;
        self.wanted = (0, 0);
        self.range = (0, 0);
        *self.view.ivars().borrow_mut() = Geometry::default();
        // SAFETY: main-thread messages to our own live view.
        unsafe {
            let _: () = msg_send![&*self.view, setString: &*NSString::from_str("")];
        }
    }

    /// The selection VoiceOver asked for, as positions in `view`, which must
    /// be the document the mirror was last synced with.
    pub fn take_request(&self, view: &DocView) -> Option<quill_core::Selection> {
        let range = self.view.ivars().borrow_mut().request.take()?;
        let paragraphs = &view.doc.main.paras;
        if self.map.starts.len() != paragraphs.len() || paragraphs.is_empty() {
            return None;
        }
        let anchor = self.map.position_of(paragraphs, range.location);
        let focus = self
            .map
            .position_of(paragraphs, range.location.saturating_add(range.length));
        Some(quill_core::Selection::new(anchor, focus))
    }
}

impl Drop for TextMirror {
    fn drop(&mut self) {
        // SAFETY: the view is ours and still alive.
        unsafe {
            let _: () = msg_send![&*self.view, removeFromSuperview];
        }
    }
}

/// Whether VoiceOver is running. AppKit keeps the answer current, so asking
/// once a draw is cheap.
fn voice_over_on() -> bool {
    // SAFETY: plain getters on the shared workspace, on the main thread.
    unsafe {
        let workspace: *mut AnyObject = msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return false;
        }
        let on: bool = msg_send![workspace, isVoiceOverEnabled];
        on
    }
}

/// Quill's laid-out lines in the mirror's UTF-16 positions, and where each
/// paragraph's lines start among them.
fn mirror_lines(view: &DocView, map: &MirrorMap) -> (Vec<MirrorLine>, Vec<usize>) {
    let paragraphs = &view.doc.main.paras;
    let mut lines = Vec::new();
    let mut first_line = Vec::with_capacity(paragraphs.len() + 1);
    for (para, paragraph) in paragraphs.iter().enumerate() {
        first_line.push(lines.len());
        let next = map.starts.get(para + 1).copied().unwrap_or(map.len);
        let mut walk = map.walk(paragraphs, para);
        let Some(boxes) = view.line_boxes(para) else {
            lines.push(MirrorLine {
                start: map.starts[para],
                end: next,
                text_end: walk.to(paragraph.text.len()),
                ..MirrorLine::default()
            });
            continue;
        };
        let mut boxes = boxes.peekable();
        while let Some(b) = boxes.next() {
            let start = walk.to(b.start);
            let text_end = walk.to(b.end);
            lines.push(MirrorLine {
                start,
                end: if boxes.peek().is_none() {
                    next
                } else {
                    text_end
                },
                text_end,
                x: b.x as f64,
                y: b.y as f64,
                width: b.width as f64,
                height: b.height as f64,
                xs: Vec::new(),
            });
        }
    }
    first_line.push(lines.len());
    (lines, first_line)
}

/// Paragraphs either side of those drawn whose lines may get glyph positions.
const NEAR_DRAWN: usize = 40;

/// Give glyph positions to the lines within a screen of the visible area,
/// and take them from lines that have left it. Only the paragraphs around
/// those last drawn are looked at, so this costs about a screenful.
fn fill_glyph_positions(g: &mut Geometry, view: &DocView, map: &MirrorMap) {
    let paragraphs = &view.doc.main.paras;
    let n = g.first_line.len().saturating_sub(1).min(paragraphs.len());
    let area = view.area;
    let top = (area.y - area.h) as f64 - g.shift.1;
    let bottom = (area.y + 2.0 * area.h) as f64 - g.shift.1;
    let mut wanted = Vec::new();
    for &(a, b) in view.drawn_paragraphs() {
        let paras = a.saturating_sub(NEAR_DRAWN)..(b + NEAR_DRAWN).min(n);
        for para in paras {
            for li in g.first_line[para]..g.first_line[para + 1] {
                if (top..bottom).contains(&g.lines[li].y) {
                    wanted.push((li, para));
                }
            }
        }
    }
    wanted.sort_unstable();
    wanted.dedup();
    for li in std::mem::take(&mut g.filled) {
        if wanted.binary_search_by_key(&li, |w| w.0).is_err() {
            g.lines[li].xs = Vec::new();
        }
    }
    for (li, para) in wanted {
        if g.lines[li].xs.is_empty() {
            let glyphs = view.line_glyph_xs(para, li - g.first_line[para]);
            g.lines[li].xs = glyph_xs(&g.lines[li], &glyphs, map.walk(paragraphs, para));
        }
        if !g.lines[li].xs.is_empty() {
            g.filled.push(li);
        }
    }
}

/// The x of each UTF-16 position of `line` from its glyphs (byte offset and
/// x). Empty when there are no glyphs.
fn glyph_xs(line: &MirrorLine, glyphs: &[(usize, f32)], mut walk: Walk) -> Vec<f64> {
    if glyphs.is_empty() {
        return Vec::new();
    }
    let mut xs = vec![f64::NAN; line.text_end - line.start + 1];
    for &(offset, gx) in glyphs {
        let index = walk.to(offset);
        if let Some(slot) = index.checked_sub(line.start).and_then(|i| xs.get_mut(i))
            && slot.is_nan()
        {
            *slot = gx as f64;
        }
    }
    let n = xs.len();
    xs[n - 1] = line.x + line.width;
    // Positions without a glyph of their own (the second half of an emoji,
    // a combining accent) take the one before.
    let mut prev = line.x;
    for slot in &mut xs {
        if slot.is_nan() {
            *slot = prev;
        }
        prev = *slot;
    }
    xs
}

/// Make Quill's own view name the mirror as its focused accessibility
/// element. AppKit asks the first responder, which is that view, and winit's
/// view would otherwise answer with the window, so VoiceOver never sees the
/// text as focused. The method is added once to the view's class; winit
/// does not define it there, so nothing of winit's is replaced.
unsafe fn point_focus_at_mirror(parent: &NSView) {
    static ADDED: std::sync::Once = std::sync::Once::new();
    ADDED.call_once(|| {
        // SAFETY: the class is winit's live view class, and the function has
        // the `@@:` signature it is registered with.
        unsafe {
            let class = objc2::ffi::object_getClass((parent as *const NSView).cast());
            let imp: unsafe extern "C" fn(*mut AnyObject, Sel) -> *mut AnyObject = focused_element;
            let added = objc2::ffi::class_addMethod(
                class as *mut _,
                sel!(accessibilityFocusedUIElement).as_ptr(),
                Some(std::mem::transmute::<
                    unsafe extern "C" fn(*mut AnyObject, Sel) -> *mut AnyObject,
                    unsafe extern "C" fn(),
                >(imp)),
                c"@@:".as_ptr(),
            );
            if added == objc2::ffi::NO {
                crate::ev!("text mirror: the view already answers for focus");
            }
        }
    });
}

/// `accessibilityFocusedUIElement` for Quill's view: its mirror subview,
/// or what NSView would answer when it has none.
unsafe extern "C" fn focused_element(this: *mut AnyObject, _cmd: Sel) -> *mut AnyObject {
    // SAFETY: AppKit calls this on the main thread with a live NSView.
    unsafe {
        let subviews: *mut AnyObject = msg_send![this, subviews];
        let count: usize = msg_send![subviews, count];
        for i in 0..count {
            let sub: *mut AnyObject = msg_send![subviews, objectAtIndex: i];
            let mirror: bool = msg_send![sub, isKindOfClass: MirrorView::class()];
            if mirror {
                return sub;
            }
        }
        let view = &*this.cast::<NSView>();
        msg_send![super(view, NSView::class()), accessibilityFocusedUIElement]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::{Paragraph, Position};

    /// Two lines of "abcd" then "ef", each glyph 10 points wide, the first
    /// a soft wrap, the second ending the paragraph.
    fn geometry() -> Geometry {
        let line = |start: usize, text_end: usize, end: usize, y: f64| MirrorLine {
            start,
            end,
            text_end,
            x: 100.0,
            y,
            width: (text_end - start) as f64 * 10.0,
            height: 20.0,
            xs: (0..=text_end - start)
                .map(|i| 100.0 + i as f64 * 10.0)
                .collect(),
        };
        Geometry {
            lines: vec![
                line(0, 4, 4, 50.0),
                line(4, 6, 7, 70.0),
                line(7, 7, 7, 90.0),
            ],
            area: NSRect::new(NSPoint::new(0.0, 70.0), NSSize::new(500.0, 30.0)),
            caret: 5,
            len: 7,
            ..Geometry::default()
        }
    }

    #[test]
    fn a_wrap_position_belongs_to_the_lower_line() {
        let g = geometry();
        assert_eq!(g.line_of(0), Some(0));
        assert_eq!(g.line_of(3), Some(0));
        assert_eq!(g.line_of(4), Some(1));
        assert_eq!(g.line_of(6), Some(1));
        assert_eq!(g.line_of(7), Some(2));
        assert_eq!(g.line_of(99), Some(2));
        assert_eq!(Geometry::default().line_of(0), None);
    }

    #[test]
    fn bounds_cover_the_characters_on_each_line() {
        let g = geometry();
        let one = g.rect(1, 2).unwrap();
        assert_eq!((one.origin.x, one.origin.y), (110.0, 50.0));
        assert_eq!((one.size.width, one.size.height), (20.0, 20.0));
        // From "c" on the first line to "e" on the second.
        let both = g.rect(2, 3).unwrap();
        assert_eq!((both.origin.x, both.origin.y), (100.0, 50.0));
        assert_eq!((both.size.width, both.size.height), (40.0, 40.0));
        // An empty range is a thin caret on its own line.
        let caret = g.rect(4, 0).unwrap();
        assert_eq!(
            (caret.origin.x, caret.origin.y, caret.size.width),
            (100.0, 70.0, 1.0)
        );
    }

    #[test]
    fn a_point_finds_the_nearest_position() {
        let g = geometry();
        assert_eq!(g.index_at(104.0, 55.0), Some(0));
        assert_eq!(g.index_at(106.0, 55.0), Some(1));
        assert_eq!(g.index_at(500.0, 75.0), Some(6));
        assert_eq!(g.index_at(100.0, 10.0), None);
    }

    #[test]
    fn only_lines_inside_the_area_are_visible() {
        assert_eq!(geometry().visible(), (4, 7));
    }

    #[test]
    fn scrolling_and_zoom_move_the_kept_lines() {
        // Scrolled down 20 and zoomed to 200%: the lines themselves are
        // not rebuilt, only how they are turned into points.
        let g = Geometry {
            shift: (0.0, -20.0),
            zoom: 2.0,
            area: NSRect::new(NSPoint::new(0.0, 60.0), NSSize::new(1000.0, 60.0)),
            ..geometry()
        };
        let one = g.rect(1, 2).unwrap();
        assert_eq!((one.origin.x, one.origin.y), (220.0, 60.0));
        assert_eq!((one.size.width, one.size.height), (40.0, 40.0));
        assert_eq!(g.rect(4, 0).unwrap().size.width, 1.0);
        assert_eq!(g.index_at(212.0, 70.0), Some(1));
        // The area covers view y 30 to 60: the first two lines.
        assert_eq!(g.visible(), (0, 7));
    }

    fn paragraph(text: &str) -> Paragraph {
        quill_core::text::from_plain_text(text).main.paras.remove(0)
    }

    fn deleted() -> u32 {
        quill_core::revision::intern(quill_core::revision::Revision {
            deleted: Some(quill_core::revision::Stamp::new("Ada", "2026-11-25")),
            ..Default::default()
        })
    }

    #[test]
    fn the_mirror_leaves_out_hidden_text_only_while_the_view_does() {
        let mut p = paragraph("one secret two");
        p.format_range(4, 11, &mut |c| c.hidden = true);
        let paras = [p, paragraph("next")];
        let (text, map) = MirrorMap::build(&paras, (true, true));
        assert_eq!(text, "one two\nnext");
        assert_eq!(map.len, 12);
        assert_eq!(map.starts, vec![0, 8]);
        // Positions after the hidden text move back over it.
        assert_eq!(map.unit_of(&paras, Position::new(0, 11)), 4);
        assert_eq!(map.unit_of(&paras, Position::new(0, 14)), 7);
        // A position inside it is where it was taken out.
        assert_eq!(map.unit_of(&paras, Position::new(0, 7)), 4);
        assert_eq!(map.position_of(&paras, 4), Position::new(0, 11));
        assert_eq!(map.position_of(&paras, 3), Position::new(0, 3));
        assert_eq!(map.position_of(&paras, 9), Position::new(1, 1));
        let (text, _) = MirrorMap::build(&paras, (false, true));
        assert_eq!(text, "one secret two\nnext");
    }

    #[test]
    fn the_mirror_leaves_out_deletions_only_while_changes_are_hidden() {
        let mut p = paragraph("keep gone keep");
        let rev = deleted();
        p.format_range(5, 10, &mut |c| c.rev = rev);
        let paras = [p];
        let (text, map) = MirrorMap::build(&paras, (true, true));
        assert_eq!(text, "keep keep");
        assert_eq!(map.position_of(&paras, 6), Position::new(0, 11));
        let (text, _) = MirrorMap::build(&paras, (true, false));
        assert_eq!(text, "keep gone keep");
    }

    #[test]
    fn a_paragraph_that_is_all_hidden_is_an_empty_line() {
        let mut p = paragraph("gone");
        p.format_range(0, 4, &mut |c| c.hidden = true);
        let paras = [paragraph("a"), p, paragraph("b")];
        let (text, map) = MirrorMap::build(&paras, (true, true));
        assert_eq!(text, "a\n\nb");
        assert_eq!(map.position_of(&paras, 2), Position::new(1, 0));
        assert_eq!(map.unit_of(&paras, Position::new(1, 4)), 2);
        assert_eq!(map.position_of(&paras, 3), Position::new(2, 0));
    }

    #[test]
    fn positions_in_a_long_paragraph_match_counting_from_the_start() {
        // Wide characters and breaks, long enough for many pieces.
        let text: String = "a\u{1f600}é\u{0b}"
            .chars()
            .cycle()
            .take(4 * PIECE_UNITS)
            .collect();
        let mut p = paragraph(&text);
        let cut = text.char_indices().nth(301).unwrap().0;
        let cut_end = text.char_indices().nth(333).unwrap().0;
        p.format_range(cut, cut_end, &mut |c| c.hidden = true);
        let paras = [p];
        let (mirror, map) = MirrorMap::build(&paras, (true, true));
        assert!(map.pieces.len() > 4);
        assert!(!mirror.contains('\u{0b}'));
        let mut walk = map.walk(&paras, 0);
        for (offset, _) in text.char_indices() {
            let shown = |o: usize| !(cut..cut_end).contains(&o);
            let expected: usize = text[..offset]
                .char_indices()
                .filter(|&(o, _)| shown(o))
                .map(|(_, c)| c.len_utf16())
                .sum();
            let unit = map.unit_of(&paras, Position::new(0, offset));
            assert_eq!(unit, expected, "at byte {offset}");
            assert_eq!(walk.to(offset), expected, "walking at byte {offset}");
            if shown(offset) {
                assert_eq!(map.position_of(&paras, unit), Position::new(0, offset));
            }
        }
        // Going back finds the same answers.
        assert_eq!(walk.to(0), 0);
        assert_eq!(walk.to(cut_end), map.unit_of(&paras, Position::new(0, cut)));
    }

    #[test]
    fn glyph_positions_skip_what_is_hidden() {
        let mut p = paragraph("abXYcd");
        p.format_range(2, 4, &mut |c| c.hidden = true);
        let paras = [p];
        let (_, map) = MirrorMap::build(&paras, (true, true));
        let line = MirrorLine {
            start: 0,
            end: 4,
            text_end: 4,
            x: 0.0,
            width: 40.0,
            ..MirrorLine::default()
        };
        // Hidden glyphs take no room, so X and Y sit where c does.
        let glyphs = [
            (0, 0.0),
            (1, 10.0),
            (2, 20.0),
            (3, 20.0),
            (4, 20.0),
            (5, 30.0),
        ];
        let xs = glyph_xs(&line, &glyphs, map.walk(&paras, 0));
        assert_eq!(xs, vec![0.0, 10.0, 20.0, 30.0, 40.0]);
    }
}
