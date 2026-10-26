//! Several editors that can reach each other: linked documents shown in two
//! editors, or a handler of one editor that calls another (decision D2).
//!
//! An [`EditorSet`] owns its editors. [`EditorSet::run`] takes one editor out of
//! its slot for the length of the call, so the editor is borrowed `&mut` while the
//! others stay reachable through the set ([`Editors`]). An editor that is out of its
//! slot (busy further up the stack) is not reachable: work for it is parked on it
//! and delivered when the outermost call returns ([`EditorSet::flush_parked`]).
//!
//! The order of events across editors is CM5's: one operation group for every
//! editor that an operation reached, each editor's operation ending in the order the
//! operations started.

use std::any::Any;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use cm_core::operation_group::{EditorId, Editors, fire_orphan_delayed, group_open};

use crate::editor::Editor;
use crate::events::Cx;

/// Editors that can reach each other, keyed by [`EditorId`].
#[derive(Default)]
pub struct EditorSet {
    slots: BTreeMap<EditorId, Option<Editor>>,
}

impl EditorSet {
    pub fn new() -> EditorSet {
        EditorSet::default()
    }
    /// Add `ed`; returns its id.
    pub fn insert(&mut self, ed: Editor) -> EditorId {
        let id = ed.id;
        self.slots.insert(id, Some(ed));
        id
    }
    /// The editor `id`, unless it is unknown or busy.
    pub fn get(&self, id: EditorId) -> Option<&Editor> {
        self.slots.get(&id)?.as_ref()
    }
    pub fn get_mut(&mut self, id: EditorId) -> Option<&mut Editor> {
        self.slots.get_mut(&id)?.as_mut()
    }
    /// Take the editor out of the set (dropping it later detaches its document).
    /// `None` when it is unknown or busy.
    pub fn remove(&mut self, id: EditorId) -> Option<Editor> {
        if self.slots.get(&id)?.is_none() {
            return None;
        }
        self.slots.remove(&id)?
    }
    pub fn ids(&self) -> Vec<EditorId> {
        self.slots.keys().copied().collect()
    }
    pub fn len(&self) -> usize {
        self.slots.len()
    }
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// `cm.operation(f)` for editor `id`, with the rest of the set reachable from
    /// `f` ([`Cx::with_editor`], [`Cx::run_op`]). `None` when `id` is unknown or
    /// busy. When the outermost call returns, work parked on any editor of the set
    /// is delivered.
    pub fn run<R>(&mut self, id: EditorId, f: impl FnOnce(&mut Cx) -> R) -> Option<R> {
        let mut ed = self.slots.get_mut(&id)?.take()?;
        let r = catch_unwind(AssertUnwindSafe(|| {
            ed.run_op_with(self, |ed, rest| {
                f(&mut Cx {
                    editor: ed,
                    others: rest,
                })
            })
        }));
        self.slots.insert(id, Some(ed));
        let r = r.unwrap_or_else(|p| resume_unwind(p));
        if !group_open() {
            self.flush_parked();
        }
        Some(r)
    }

    /// Deliver the work parked on the editors of the set, until none is left.
    pub fn flush_parked(&mut self) {
        loop {
            let ids: Vec<EditorId> = self
                .slots
                .iter()
                .filter(|(_, e)| e.as_ref().is_some_and(|e| e.parked_len() > 0))
                .map(|(id, _)| *id)
                .collect();
            if ids.is_empty() {
                return;
            }
            for id in ids {
                let Some(mut ed) = self.slots.get_mut(&id).and_then(Option::take) else {
                    continue;
                };
                let r = catch_unwind(AssertUnwindSafe(|| ed.flush_parked_with(self)));
                self.slots.insert(id, Some(ed));
                if let Err(p) = r {
                    resume_unwind(p);
                }
            }
        }
    }

    /// The host's `setTimeout(fireOrphanDelayed, 0)`: deliver the orphan queue with
    /// the set's editors reachable, then any parked work.
    pub fn fire_orphan_delayed(&mut self) {
        fire_orphan_delayed(self);
        self.flush_parked();
    }
}

impl Editors for EditorSet {
    fn with_editor(
        &mut self,
        id: EditorId,
        f: &mut dyn FnMut(&mut dyn Any, &mut dyn Editors),
    ) -> bool {
        let Some(mut ed) = self.slots.get_mut(&id).and_then(Option::take) else {
            return false;
        };
        let r = catch_unwind(AssertUnwindSafe(|| f(&mut ed, self)));
        self.slots.insert(id, Some(ed));
        if let Err(p) = r {
            resume_unwind(p);
        }
        true
    }
}
