//! `src/edit/commands.js`: the named commands key maps bind to.

use std::collections::HashMap;
use std::rc::Rc;

use cm_core::util::misc::count_column;
use cm_core::{Pos, SelOptions};

use crate::editor::{CommandFn, CoordSys, Editor, Indent, sel_dont_scroll, sel_move, space_str};
use crate::movement::{
    UnitH, UnitV, line_end, line_side, line_start, line_start_smart, search_non_ws,
};

fn cmd(f: fn(&mut Editor)) -> CommandFn {
    Rc::new(move |cm: &mut Editor| {
        f(cm);
        true
    })
}

fn move_bias(bias: i64) -> Option<SelOptions> {
    Some(SelOptions {
        origin: Some("+move".into()),
        bias: Some(bias),
        ..Default::default()
    })
}

/// The built-in `CodeMirror.commands` table.
pub fn builtin_commands() -> HashMap<String, CommandFn> {
    let mut c: HashMap<String, CommandFn> = HashMap::new();
    let mut add = |name: &str, f: fn(&mut Editor)| {
        c.insert(name.to_string(), cmd(f));
    };
    add("selectAll", |cm| cm.run_op(|cm| cm.doc.select_all()));
    add("singleSelection", |cm| {
        cm.run_op(|cm| {
            let a = cm.doc.get_cursor(Some("anchor"));
            let h = cm.doc.get_cursor(Some("head"));
            cm.doc.set_selection(a, Some(h), sel_dont_scroll());
        })
    });
    add("killLine", |cm| {
        cm.delete_near_selection(|cm, range| {
            if range.empty() {
                let len = cm.line_text(range.head.line).len() as i64;
                if range.head.ch == len && range.head.line < cm.last_line() {
                    (range.head, Pos::new(range.head.line + 1, 0))
                } else {
                    (range.head, Pos::new(range.head.line, len))
                }
            } else {
                (range.from(), range.to())
            }
        })
    });
    add("deleteLine", |cm| {
        cm.delete_near_selection(|cm, range| {
            (
                Pos::new(range.from().line, 0),
                cm.doc.clip_pos(Pos::new(range.to().line + 1, 0)),
            )
        })
    });
    add("delLineLeft", |cm| {
        cm.delete_near_selection(|_, range| (Pos::new(range.from().line, 0), range.from()))
    });
    add("delWrappedLineLeft", |cm| {
        cm.delete_near_selection(|cm, range| {
            let top = cm.char_coords(range.head, CoordSys::Doc).top + 5.0;
            let left = cm.coords_char(0.0, top).pos();
            (left, range.from())
        })
    });
    add("delWrappedLineRight", |cm| {
        cm.delete_near_selection(|cm, range| {
            let top = cm.char_coords(range.head, CoordSys::Doc).top + 5.0;
            let right = cm.coords_char(line_div_width(cm) + 100.0, top).pos();
            (range.from(), right)
        })
    });
    add("undo", |cm| cm.run_op(|cm| cm.doc.undo()));
    add("redo", |cm| cm.run_op(|cm| cm.doc.redo()));
    add("undoSelection", |cm| {
        cm.run_op(|cm| cm.doc.undo_selection())
    });
    add("redoSelection", |cm| {
        cm.run_op(|cm| cm.doc.redo_selection())
    });
    add("goDocStart", |cm| {
        cm.run_op(|cm| {
            cm.doc
                .extend_selection(Pos::new(cm.first_line(), 0), None, None)
        })
    });
    add("goDocEnd", |cm| {
        cm.run_op(|cm| {
            cm.doc
                .extend_selection(Pos::line_end(cm.last_line()), None, None)
        })
    });
    add("goLineStart", |cm| {
        cm.extend_selections_by(|cm, r| line_start(cm, r.head.line), move_bias(1))
    });
    add("goLineStartSmart", |cm| {
        cm.extend_selections_by(|cm, r| line_start_smart(cm, r.head), move_bias(1))
    });
    add("goLineEnd", |cm| {
        cm.extend_selections_by(|cm, r| line_end(cm, r.head.line), move_bias(-1))
    });
    add("goLineRight", |cm| {
        cm.extend_selections_by(
            |cm, r| line_side(cm, r.head, line_div_width(cm) + 100.0),
            sel_move(),
        )
    });
    add("goLineLeft", |cm| {
        cm.extend_selections_by(|cm, r| line_side(cm, r.head, 0.0), sel_move())
    });
    add("goLineLeftSmart", |cm| {
        cm.extend_selections_by(
            |cm, r| {
                let pos = line_side(cm, r.head, 0.0);
                if pos.ch < search_non_ws(&cm.line_text(pos.line)) {
                    return line_start_smart(cm, r.head);
                }
                pos
            },
            sel_move(),
        )
    });
    add("goLineUp", |cm| cm.move_v(-1, UnitV::Line));
    add("goLineDown", |cm| cm.move_v(1, UnitV::Line));
    add("goPageUp", |cm| cm.move_v(-1, UnitV::Page));
    add("goPageDown", |cm| cm.move_v(1, UnitV::Page));
    add("goCharLeft", |cm| cm.move_h(-1, UnitH::Char));
    add("goCharRight", |cm| cm.move_h(1, UnitH::Char));
    add("goColumnLeft", |cm| cm.move_h(-1, UnitH::Column));
    add("goColumnRight", |cm| cm.move_h(1, UnitH::Column));
    add("goWordLeft", |cm| cm.move_h(-1, UnitH::Word));
    add("goGroupRight", |cm| cm.move_h(1, UnitH::Group));
    add("goGroupLeft", |cm| cm.move_h(-1, UnitH::Group));
    add("goWordRight", |cm| cm.move_h(1, UnitH::Word));
    add("delCharBefore", |cm| cm.delete_h(-1, UnitH::Codepoint));
    add("delCharAfter", |cm| cm.delete_h(1, UnitH::Char));
    add("delWordBefore", |cm| cm.delete_h(-1, UnitH::Word));
    add("delWordAfter", |cm| cm.delete_h(1, UnitH::Word));
    add("delGroupBefore", |cm| cm.delete_h(-1, UnitH::Group));
    add("delGroupAfter", |cm| cm.delete_h(1, UnitH::Group));
    add("indentAuto", |cm| cm.indent_selection(Indent::Smart));
    add("indentMore", |cm| cm.indent_selection(Indent::Add));
    add("indentLess", |cm| cm.indent_selection(Indent::Subtract));
    add("insertTab", |cm| {
        cm.run_op(|cm| cm.doc.replace_selection("\t", None, None))
    });
    add("insertSoftTab", |cm| {
        cm.run_op(|cm| {
            let tab_size = cm.options.tab_size;
            let spaces: Vec<String> = cm
                .doc
                .list_selections()
                .iter()
                .map(|r| {
                    let pos = r.from();
                    let col = count_column(
                        &cm.line_text(pos.line),
                        Some(pos.ch.max(0) as usize),
                        tab_size,
                        0,
                        0,
                    );
                    space_str((tab_size - col % tab_size) as f64)
                })
                .collect();
            let refs: Vec<&str> = spaces.iter().map(String::as_str).collect();
            cm.doc.replace_selections(&refs, None, None);
        })
    });
    add("defaultTab", |cm| {
        if cm.doc.something_selected() {
            cm.indent_selection(Indent::Add)
        } else {
            cm.exec_command("insertTab");
        }
    });
    add("transposeChars", transpose_chars);
    add("newlineAndIndent", |cm| {
        cm.run_op(|cm| {
            let sels = cm.doc.list_selections();
            let sep = cm.doc.line_separator();
            for s in sels.iter().rev() {
                cm.doc
                    .replace_range(&sep, s.anchor, Some(s.head), Some("+input"));
            }
            let sels = cm.doc.list_selections();
            for s in &sels {
                cm.indent_line(s.from().line, None, true);
            }
            cm.ensure_cursor_visible();
        })
    });
    add("openLine", |cm| {
        cm.run_op(|cm| cm.doc.replace_selection("\n", Some("start"), None))
    });
    add("toggleOverwrite", |cm| cm.toggle_overwrite(None));
    c
}

/// `cm.display.lineDiv.offsetWidth`: at least the widest line.
fn line_div_width(cm: &Editor) -> f64 {
    cm.text_area_width().max(cm.max_line_width())
}

/// `transposeChars` (runInOp). Works on UTF-16 units like the JS `charAt`.
fn transpose_chars(cm: &mut Editor) {
    cm.run_op(|cm| {
        let ranges = cm.doc.list_selections();
        let mut new_sel: Vec<(Pos, Option<Pos>)> = Vec::new();
        for r in &ranges {
            if !r.empty() {
                continue;
            }
            let mut cur = r.head;
            let line = cm.line_text(cur.line);
            if !line.is_empty() {
                let len = line.len() as i64;
                let char_at = |i: i64| -> Vec<u16> {
                    if i >= 0 && i < len {
                        vec![line[i as usize]]
                    } else {
                        vec![]
                    }
                };
                if cur.ch == len {
                    cur = Pos::new(cur.line, cur.ch - 1);
                }
                if cur.ch > 0 {
                    cur = Pos::new(cur.line, cur.ch + 1);
                    let mut s = char_at(cur.ch - 1);
                    s.extend(char_at(cur.ch - 2));
                    cm.doc.replace_range_utf16(
                        &s,
                        Pos::new(cur.line, cur.ch - 2),
                        Some(cur),
                        Some("+transpose"),
                    );
                } else if cur.line > cm.first_line() {
                    let prev = cm.line_text(cur.line - 1);
                    if !prev.is_empty() {
                        cur = Pos::new(cur.line, 1);
                        let mut s = char_at(0);
                        s.extend(cm_core::text::to_u16(&cm.doc.line_separator()));
                        s.push(prev[prev.len() - 1]);
                        cm.doc.replace_range_utf16(
                            &s,
                            Pos::new(cur.line - 1, prev.len() as i64 - 1),
                            Some(cur),
                            Some("+transpose"),
                        );
                    }
                }
            }
            new_sel.push((cur, None));
        }
        cm.doc.set_selections(&new_sel, None, None);
    });
}
