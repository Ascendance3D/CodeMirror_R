//! `scrollTo(x, y)` after a queued scroll position (`op.scrollToPos`) in the same
//! operation. `scrollTo` is `scrollToCoords` (src/display/scrolling.js:125-129):
//! when an axis is given it first calls `resolveScrollToPos` (:140-147), which
//! clears the queued position and turns its `estimateCoords` rectangle into
//! `op.scrollLeft` / `op.scrollTop` through `calculateScrollPos`; then the given
//! axes replace those values. `endOperation_finish` (src/display/operations.js:144-152)
//! applies `op.scrollTop`, then `op.scrollLeft`, then a queued position that is
//! still set. A missing axis keeps the resolved value, and `scrollTo(null, null)`
//! leaves the queued position for the exact `scrollPosIntoView`.
//!
//! The expected values are CM5 5.65.21 in headless Chromium 151 at
//! [`Metrics::oracle`] (DejaVu Sans Mono 32px, line-height 38px), `setSize(600, 300)`
//! (CODEMIRROR_R_SCROLL_TO_POS_01 lane recording). For "zero" the page measures a
//! character width of 0, so `charWidth` gives 10; the long lines are CJK there, 32px
//! wide in both. Each row: case, document, zero width, wrapping, then
//! `doc.scrollLeft`, `doc.scrollTop`, the selection, the UTF-16 length and the undo
//! depth after the operation.
//!
//! Recorded rows left out (not part of this correction): with wrapping, CM5 uses
//! estimated heights for lines it has not drawn (`estimateHeight`), and Rust
//! measures every wrapped line; and at the right edge CM5 clamps to the DOM's whole
//! pixel `scrollWidth`.

use cm_core::Pos;
use cm_modes::Spec;
use cm_view::{Editor, EditorOptions, Metrics};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Doc {
    Long,
    Short,
    Empty,
}
use Doc::*;

type Sel = (i64, i64, i64, i64);
type Row = (&'static str, Doc, bool, bool, f64, f64, Sel, usize, usize);

const ROWS: &[Row] = &[
    (
        "A01",
        Long,
        false,
        false,
        1336.5625,
        0.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A02",
        Long,
        false,
        false,
        0.0,
        1262.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A03",
        Long,
        false,
        false,
        50.0,
        100.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A04",
        Long,
        false,
        false,
        1340.5625,
        1262.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A05",
        Long,
        false,
        false,
        30.0,
        200.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A06",
        Long,
        false,
        false,
        1340.5625,
        1262.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A07",
        Long,
        false,
        false,
        1721.875,
        300.0,
        (45, 120, 45, 120),
        9063,
        1,
    ),
    (
        "A08",
        Long,
        false,
        false,
        1336.5625,
        0.0,
        (10, 5, 40, 100),
        9059,
        0,
    ),
    (
        "A09",
        Long,
        false,
        false,
        2107.1875,
        100.0,
        (0, 0, 0, 0),
        9059,
        0,
    ),
    (
        "A10",
        Long,
        false,
        false,
        200.0,
        400.0,
        (0, 0, 0, 0),
        9059,
        0,
    ),
    ("A11", Long, false, false, 0.0, 0.0, (5, 2, 5, 2), 9059, 0),
    (
        "A12",
        Long,
        false,
        false,
        1336.5625,
        0.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A13",
        Long,
        false,
        false,
        47.796875,
        900.0,
        (2, 3, 2, 3),
        9059,
        0,
    ),
    (
        "A15",
        Long,
        false,
        false,
        0.0,
        0.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A01",
        Long,
        false,
        true,
        0.0,
        0.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A03",
        Long,
        false,
        true,
        0.0,
        100.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A05",
        Long,
        false,
        true,
        0.0,
        200.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A07",
        Long,
        false,
        true,
        0.0,
        300.0,
        (45, 120, 45, 120),
        9063,
        1,
    ),
    (
        "A08",
        Long,
        false,
        true,
        0.0,
        0.0,
        (10, 5, 40, 100),
        9059,
        0,
    ),
    ("A09", Long, false, true, 0.0, 100.0, (0, 0, 0, 0), 9059, 0),
    ("A10", Long, false, true, 0.0, 400.0, (0, 0, 0, 0), 9059, 0),
    ("A11", Long, false, true, 0.0, 0.0, (5, 2, 5, 2), 9059, 0),
    (
        "A12",
        Long,
        false,
        true,
        0.0,
        0.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    ("A13", Long, false, true, 0.0, 900.0, (2, 3, 2, 3), 9059, 0),
    (
        "A15",
        Long,
        false,
        true,
        0.0,
        0.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A01",
        Long,
        true,
        false,
        410.0,
        0.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A02",
        Long,
        true,
        false,
        0.0,
        1262.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A03",
        Long,
        true,
        false,
        50.0,
        100.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A04",
        Long,
        true,
        false,
        2614.0,
        1262.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A05",
        Long,
        true,
        false,
        30.0,
        200.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A06",
        Long,
        true,
        false,
        2614.0,
        1262.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A07",
        Long,
        true,
        false,
        610.0,
        300.0,
        (45, 120, 45, 120),
        9063,
        1,
    ),
    (
        "A08",
        Long,
        true,
        false,
        410.0,
        0.0,
        (10, 5, 40, 100),
        9059,
        0,
    ),
    (
        "A09",
        Long,
        true,
        false,
        810.0,
        100.0,
        (0, 0, 0, 0),
        9059,
        0,
    ),
    (
        "A10",
        Long,
        true,
        false,
        200.0,
        400.0,
        (0, 0, 0, 0),
        9059,
        0,
    ),
    ("A11", Long, true, false, 0.0, 0.0, (5, 2, 5, 2), 9059, 0),
    (
        "A12",
        Long,
        true,
        false,
        410.0,
        0.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    ("A13", Long, true, false, 20.0, 900.0, (2, 3, 2, 3), 9059, 0),
    (
        "A15",
        Long,
        true,
        false,
        0.0,
        0.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A01",
        Long,
        true,
        true,
        0.0,
        0.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A03",
        Long,
        true,
        true,
        0.0,
        100.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A05",
        Long,
        true,
        true,
        0.0,
        200.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    (
        "A07",
        Long,
        true,
        true,
        0.0,
        300.0,
        (45, 120, 45, 120),
        9063,
        1,
    ),
    ("A08", Long, true, true, 0.0, 0.0, (10, 5, 40, 100), 9059, 0),
    ("A09", Long, true, true, 0.0, 100.0, (0, 0, 0, 0), 9059, 0),
    ("A10", Long, true, true, 0.0, 400.0, (0, 0, 0, 0), 9059, 0),
    ("A11", Long, true, true, 0.0, 0.0, (5, 2, 5, 2), 9059, 0),
    (
        "A12",
        Long,
        true,
        true,
        0.0,
        0.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    ("A13", Long, true, true, 0.0, 900.0, (2, 3, 2, 3), 9059, 0),
    (
        "A15",
        Long,
        true,
        true,
        0.0,
        0.0,
        (40, 100, 40, 100),
        9059,
        0,
    ),
    ("A01", Short, false, false, 0.0, 0.0, (40, 2, 40, 2), 179, 0),
    (
        "A02",
        Short,
        false,
        false,
        0.0,
        1262.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    (
        "A03",
        Short,
        false,
        false,
        0.0,
        100.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    (
        "A04",
        Short,
        false,
        false,
        0.0,
        1262.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    (
        "A05",
        Short,
        false,
        false,
        0.0,
        200.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    (
        "A06",
        Short,
        false,
        false,
        0.0,
        1262.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    (
        "A07",
        Short,
        false,
        false,
        0.0,
        300.0,
        (45, 2, 45, 2),
        183,
        1,
    ),
    ("A08", Short, false, false, 0.0, 0.0, (10, 2, 40, 2), 179, 0),
    ("A09", Short, false, false, 0.0, 100.0, (0, 0, 0, 0), 179, 0),
    ("A10", Short, false, false, 0.0, 400.0, (0, 0, 0, 0), 179, 0),
    ("A11", Short, false, false, 0.0, 0.0, (5, 2, 5, 2), 179, 0),
    ("A12", Short, false, false, 0.0, 0.0, (40, 2, 40, 2), 179, 0),
    ("A13", Short, false, false, 0.0, 900.0, (2, 2, 2, 2), 179, 0),
    (
        "A14",
        Short,
        false,
        false,
        0.0,
        1988.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    ("A15", Short, false, false, 0.0, 0.0, (40, 2, 40, 2), 179, 0),
    ("A01", Short, false, true, 0.0, 0.0, (40, 2, 40, 2), 179, 0),
    (
        "A02",
        Short,
        false,
        true,
        0.0,
        1262.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    (
        "A03",
        Short,
        false,
        true,
        0.0,
        100.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    (
        "A04",
        Short,
        false,
        true,
        0.0,
        1262.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    (
        "A05",
        Short,
        false,
        true,
        0.0,
        200.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    (
        "A06",
        Short,
        false,
        true,
        0.0,
        1262.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    (
        "A07",
        Short,
        false,
        true,
        0.0,
        300.0,
        (45, 2, 45, 2),
        183,
        1,
    ),
    ("A08", Short, false, true, 0.0, 0.0, (10, 2, 40, 2), 179, 0),
    ("A09", Short, false, true, 0.0, 100.0, (0, 0, 0, 0), 179, 0),
    ("A10", Short, false, true, 0.0, 400.0, (0, 0, 0, 0), 179, 0),
    ("A11", Short, false, true, 0.0, 0.0, (5, 2, 5, 2), 179, 0),
    ("A12", Short, false, true, 0.0, 0.0, (40, 2, 40, 2), 179, 0),
    ("A13", Short, false, true, 0.0, 900.0, (2, 2, 2, 2), 179, 0),
    (
        "A14",
        Short,
        false,
        true,
        0.0,
        1988.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    ("A15", Short, false, true, 0.0, 0.0, (40, 2, 40, 2), 179, 0),
    ("A01", Short, true, false, 0.0, 0.0, (40, 2, 40, 2), 179, 0),
    (
        "A02",
        Short,
        true,
        false,
        0.0,
        1262.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    (
        "A03",
        Short,
        true,
        false,
        0.0,
        100.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    (
        "A04",
        Short,
        true,
        false,
        0.0,
        1262.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    (
        "A05",
        Short,
        true,
        false,
        0.0,
        200.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    (
        "A06",
        Short,
        true,
        false,
        0.0,
        1262.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    (
        "A07",
        Short,
        true,
        false,
        0.0,
        300.0,
        (45, 2, 45, 2),
        183,
        1,
    ),
    ("A08", Short, true, false, 0.0, 0.0, (10, 2, 40, 2), 179, 0),
    ("A09", Short, true, false, 0.0, 100.0, (0, 0, 0, 0), 179, 0),
    ("A10", Short, true, false, 0.0, 400.0, (0, 0, 0, 0), 179, 0),
    ("A11", Short, true, false, 0.0, 0.0, (5, 2, 5, 2), 179, 0),
    ("A12", Short, true, false, 0.0, 0.0, (40, 2, 40, 2), 179, 0),
    ("A13", Short, true, false, 0.0, 900.0, (2, 2, 2, 2), 179, 0),
    (
        "A14",
        Short,
        true,
        false,
        0.0,
        1988.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    ("A15", Short, true, false, 0.0, 0.0, (40, 2, 40, 2), 179, 0),
    ("A01", Short, true, true, 0.0, 0.0, (40, 2, 40, 2), 179, 0),
    (
        "A02",
        Short,
        true,
        true,
        0.0,
        1262.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    ("A03", Short, true, true, 0.0, 100.0, (40, 2, 40, 2), 179, 0),
    (
        "A04",
        Short,
        true,
        true,
        0.0,
        1262.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    ("A05", Short, true, true, 0.0, 200.0, (40, 2, 40, 2), 179, 0),
    (
        "A06",
        Short,
        true,
        true,
        0.0,
        1262.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    ("A07", Short, true, true, 0.0, 300.0, (45, 2, 45, 2), 183, 1),
    ("A08", Short, true, true, 0.0, 0.0, (10, 2, 40, 2), 179, 0),
    ("A09", Short, true, true, 0.0, 100.0, (0, 0, 0, 0), 179, 0),
    ("A10", Short, true, true, 0.0, 400.0, (0, 0, 0, 0), 179, 0),
    ("A11", Short, true, true, 0.0, 0.0, (5, 2, 5, 2), 179, 0),
    ("A12", Short, true, true, 0.0, 0.0, (40, 2, 40, 2), 179, 0),
    ("A13", Short, true, true, 0.0, 900.0, (2, 2, 2, 2), 179, 0),
    (
        "A14",
        Short,
        true,
        true,
        0.0,
        1988.0,
        (40, 2, 40, 2),
        179,
        0,
    ),
    ("A15", Short, true, true, 0.0, 0.0, (40, 2, 40, 2), 179, 0),
    ("A01", Empty, false, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A02", Empty, false, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A03", Empty, false, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A04", Empty, false, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A05", Empty, false, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A06", Empty, false, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A07", Empty, false, false, 0.0, 0.0, (1, 0, 1, 0), 4, 1),
    ("A08", Empty, false, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A09", Empty, false, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A10", Empty, false, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A11", Empty, false, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A12", Empty, false, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A13", Empty, false, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A14", Empty, false, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A15", Empty, false, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A01", Empty, false, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A02", Empty, false, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A03", Empty, false, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A04", Empty, false, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A05", Empty, false, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A06", Empty, false, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A07", Empty, false, true, 0.0, 0.0, (1, 0, 1, 0), 4, 1),
    ("A08", Empty, false, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A09", Empty, false, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A10", Empty, false, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A11", Empty, false, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A12", Empty, false, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A13", Empty, false, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A14", Empty, false, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A15", Empty, false, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A01", Empty, true, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A02", Empty, true, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A03", Empty, true, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A04", Empty, true, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A05", Empty, true, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A06", Empty, true, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A07", Empty, true, false, 0.0, 0.0, (1, 0, 1, 0), 4, 1),
    ("A08", Empty, true, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A09", Empty, true, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A10", Empty, true, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A11", Empty, true, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A12", Empty, true, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A13", Empty, true, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A14", Empty, true, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A15", Empty, true, false, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A01", Empty, true, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A02", Empty, true, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A03", Empty, true, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A04", Empty, true, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A05", Empty, true, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A06", Empty, true, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A07", Empty, true, true, 0.0, 0.0, (1, 0, 1, 0), 4, 1),
    ("A08", Empty, true, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A09", Empty, true, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A10", Empty, true, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A11", Empty, true, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A12", Empty, true, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A13", Empty, true, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A14", Empty, true, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
    ("A15", Empty, true, true, 0.0, 0.0, (0, 0, 0, 0), 0, 0),
];

fn editor(doc: Doc, zero: bool, wrap: bool) -> Editor {
    let ch = if zero { "\u{4e00}" } else { "x" };
    let text = match doc {
        Long => vec![ch.repeat(150); 60].join("\n"),
        Short => vec!["ab"; 60].join("\n"),
        Empty => String::new(),
    };
    let mut m = Metrics::oracle();
    if zero {
        m.char_width = 0.0;
    }
    let options = EditorOptions {
        line_wrapping: wrap,
        ..EditorOptions::default()
    };
    let mut cm = Editor::new(&text, Spec::Null, options, m);
    cm.set_size(600.0, 300.0);
    cm
}

fn cursor(cm: &mut Editor, line: i64, ch: i64) {
    cm.doc.set_cursor(Pos::new(line, ch), None);
}

/// The case's steps; `pre` runs as its own operation, then `ops` in one.
fn run(id: &str, cm: &mut Editor) {
    let pre = |cm: &mut Editor| match id {
        "A12" => {
            cursor(cm, 40, 100);
            cm.scroll_to(None, Some(0.0));
        }
        "A13" | "A15" => cm.scroll_to(Some(300.0), Some(800.0)),
        _ => {}
    };
    cm.run_op(pre);
    cm.run_op(|cm| match id {
        "A01" | "A12" => {
            cursor(cm, 40, 100);
            cm.scroll_to(None, Some(0.0));
        }
        "A02" => {
            cursor(cm, 40, 100);
            cm.scroll_to(Some(0.0), None);
        }
        "A03" => {
            cursor(cm, 40, 100);
            cm.scroll_to(Some(50.0), Some(100.0));
        }
        "A04" => {
            cursor(cm, 40, 100);
            cm.scroll_to(None, None);
        }
        "A05" => {
            cursor(cm, 40, 100);
            cm.scroll_to(None, Some(200.0));
            cm.scroll_to(Some(30.0), None);
        }
        "A06" => {
            cm.scroll_to(Some(0.0), Some(0.0));
            cursor(cm, 40, 100);
        }
        "A07" => {
            cm.doc.replace_range("new\n", Pos::new(0, 0), None, None);
            cursor(cm, 45, 120);
            cm.scroll_to(None, Some(300.0));
        }
        "A08" => {
            cm.doc
                .set_selection(Pos::new(10, 5), Some(Pos::new(40, 100)), None);
            cm.scroll_to(None, Some(0.0));
        }
        "A09" => {
            let p = Pos::new(50, 140);
            cm.scroll_into_view(p, p, 0.0);
            cm.scroll_to(None, Some(100.0));
        }
        "A10" => cm.scroll_to(Some(200.0), Some(400.0)),
        "A11" => {
            cursor(cm, 5, 2);
            cm.scroll_to(None, Some(0.0));
        }
        "A13" => {
            cursor(cm, 2, 3);
            cm.scroll_to(None, Some(900.0));
        }
        "A14" => {
            cursor(cm, 40, 100);
            cm.scroll_to(Some(100000.0), Some(100000.0));
        }
        "A15" => {
            cursor(cm, 40, 100);
            cm.scroll_to(Some(-50.0), Some(-50.0));
        }
        _ => panic!("{id}"),
    });
}

fn sel(cm: &Editor) -> Vec<Sel> {
    cm.doc
        .list_selections()
        .iter()
        .map(|r| (r.anchor.line, r.anchor.ch, r.head.line, r.head.ch))
        .collect()
}

/// Run every recorded row of the cases `ids` and compare the final state.
fn check(ids: &[&str]) {
    let rows: Vec<&Row> = ROWS.iter().filter(|r| ids.contains(&r.0)).collect();
    assert!(!rows.is_empty(), "{ids:?}");
    for &&(id, doc, zero, wrap, left, top, s, len, undo) in &rows {
        let mut cm = editor(doc, zero, wrap);
        run(id, &mut cm);
        let at = format!("{id} {doc:?} zero {zero} wrap {wrap}");
        assert_eq!(
            (cm.display.scroll_left, cm.display.scroll_top),
            (left, top),
            "{at}"
        );
        assert_eq!(sel(&cm), vec![s], "{at}");
        assert_eq!(cm.get_value().encode_utf16().count(), len, "{at}");
        let h = cm.doc.history_size();
        assert_eq!((h.undo, h.redo), (undo, 0), "{at}");
    }
}

/// `setCursor` then `scrollTo(null, y)`: y wins, the left comes from the estimate.
#[test]
fn queued_cursor_then_vertical_scroll() {
    check(&["A01"]);
}

/// `setCursor` then `scrollTo(x, null)`: x wins, the top comes from the estimate.
#[test]
fn queued_cursor_then_horizontal_scroll() {
    check(&["A02"]);
}

#[test]
fn queued_cursor_then_both_axes() {
    check(&["A03"]);
}

/// `scrollTo(null, null)` does not resolve: the cursor is scrolled into view
/// exactly at the end, as before this correction.
#[test]
fn scroll_with_no_axis_keeps_the_queued_cursor() {
    check(&["A04"]);
}

/// Two `scrollTo` calls: the first resolves the cursor, the second gives the left
/// and keeps the first's top.
#[test]
fn multiple_scroll_requests() {
    check(&["A05"]);
}

/// A cursor move after `scrollTo` queues a new position, which wins exactly.
#[test]
fn cursor_after_scroll_is_visible() {
    check(&["A06", "A11"]);
}

#[test]
fn document_change_then_queued_cursor() {
    check(&["A07"]);
}

#[test]
fn selection_change_then_scroll() {
    check(&["A08"]);
}

/// `scrollIntoView(pos)` (`scrollToRange`) is resolved the same way.
#[test]
fn scroll_into_view_then_scroll() {
    check(&["A09"]);
}

#[test]
fn scroll_without_a_queued_position() {
    check(&["A10"]);
}

#[test]
fn repeated_operations() {
    check(&["A12", "A13"]);
}

/// The given coordinates are clamped when the operation ends.
#[test]
fn queued_cursor_then_clamped_scroll() {
    check(&["A14", "A15"]);
}

/// Every recorded row, by document, width and wrapping.
#[test]
fn all_rows_by_document_width_and_wrapping() {
    for doc in [Long, Short, Empty] {
        for zero in [false, true] {
            for wrap in [false, true] {
                let n = ROWS
                    .iter()
                    .filter(|r| r.1 == doc && r.2 == zero && r.3 == wrap)
                    .count();
                assert!(n >= 11, "{doc:?} {zero} {wrap}: {n}");
            }
        }
    }
    let ids: Vec<&str> = ROWS.iter().map(|r| r.0).collect();
    check(&ids);
}

/// Inside the operation `scrollTo` only stores `op.scrollLeft` / `op.scrollTop`:
/// `doc.scrollLeft` and `doc.scrollTop` stay 0 until it ends (recorded: 0, 0 in the
/// operation; 300, 500 after; `curOp.scrollToPos` null).
#[test]
fn scroll_is_pending_until_the_operation_ends() {
    let mut cm = editor(Long, false, false);
    let mid = cm.run_op(|cm| {
        cursor(cm, 40, 100);
        cm.scroll_to(Some(300.0), Some(500.0));
        (cm.display.scroll_left, cm.display.scroll_top)
    });
    assert_eq!(mid, (0.0, 0.0));
    assert_eq!(
        (cm.display.scroll_left, cm.display.scroll_top),
        (300.0, 500.0)
    );
}

/// Two editors, the second's operation nested in the first's: each resolves its
/// own queued cursor (recorded: a 0/1262, b 410/0).
#[test]
fn editors_keep_their_own_queued_position() {
    let (mut a, mut b) = (editor(Long, false, false), editor(Long, true, false));
    a.run_op(|a| {
        cursor(a, 40, 100);
        b.run_op(|b| {
            cursor(b, 40, 100);
            b.scroll_to(None, Some(0.0));
        });
        a.scroll_to(Some(0.0), None);
    });
    b.flush_parked();
    assert_eq!((a.display.scroll_left, a.display.scroll_top), (0.0, 1262.0));
    assert_eq!((b.display.scroll_left, b.display.scroll_top), (410.0, 0.0));
}
