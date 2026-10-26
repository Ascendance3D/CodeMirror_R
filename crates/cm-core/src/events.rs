//! `src/util/event.js`: typed listener lists.
//!
//! The JS keeps one array of handler functions per emitter and event type. `on` and
//! `off` build a new array, so a dispatch that is already running keeps iterating
//! the array it started with: a handler removed during the dispatch still runs, and
//! one added during it does not ([`Listeners::snapshot`]).
//!
//! Each handler gets a [`ListenerId`] that is unique on the thread and never reused;
//! [`Listeners::off`] removes by id. [`Listeners::off_fn`] removes by function
//! identity, like `off(emitter, type, f)`: the same handler `Rc` registered twice is
//! two entries, and one `off_fn` removes the first of them.

use std::cell::Cell;
use std::rc::Rc;

/// The id of one registered handler. Ids are unique on the thread and never reused,
/// so one id names one entry in one list.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ListenerId(u64);

thread_local! {
    static NEXT_LISTENER: Cell<u64> = const { Cell::new(1) };
}

impl ListenerId {
    fn next() -> ListenerId {
        NEXT_LISTENER.with(|n| {
            let v = n.get();
            n.set(v + 1);
            ListenerId(v)
        })
    }
}

/// One registered handler.
pub struct Entry<H: ?Sized> {
    pub id: ListenerId,
    pub f: Rc<H>,
}

impl<H: ?Sized> Clone for Entry<H> {
    fn clone(&self) -> Self {
        Entry {
            id: self.id,
            f: self.f.clone(),
        }
    }
}

impl<H: ?Sized> Entry<H> {
    /// JS function identity: the same `Rc` allocation. Only the data pointers are
    /// compared; the vtable halves of `dyn` pointers may differ for one function.
    pub fn same_fn(&self, other: &Rc<H>) -> bool {
        std::ptr::addr_eq(Rc::as_ptr(&self.f), Rc::as_ptr(other))
    }
}

/// The handlers of one event type on one emitter, copy-on-write.
pub struct Listeners<H: ?Sized>(Rc<[Entry<H>]>);

impl<H: ?Sized> Default for Listeners<H> {
    fn default() -> Self {
        Listeners(Rc::from(Vec::<Entry<H>>::new()))
    }
}

impl<H: ?Sized> Clone for Listeners<H> {
    fn clone(&self) -> Self {
        Listeners(self.0.clone())
    }
}

impl<H: ?Sized> Listeners<H> {
    /// `on(emitter, type, f)`: append `f`.
    pub fn on_rc(&mut self, f: Rc<H>) -> ListenerId {
        let id = ListenerId::next();
        let mut v = self.0.to_vec();
        v.push(Entry { id, f });
        self.0 = v.into();
        id
    }
    /// Remove the handler registered as `id`. `false` when it is not in this list.
    pub fn off(&mut self, id: ListenerId) -> bool {
        self.remove_where(|e| e.id == id)
    }
    /// `off(emitter, type, f)`: remove the first entry with the same function.
    pub fn off_fn(&mut self, f: &Rc<H>) -> bool {
        self.remove_where(|e| e.same_fn(f))
    }
    fn remove_where(&mut self, pred: impl Fn(&Entry<H>) -> bool) -> bool {
        match self.0.iter().position(pred) {
            Some(i) => {
                let mut v = self.0.to_vec();
                v.remove(i);
                self.0 = v.into();
                true
            }
            None => false,
        }
    }
    /// `getHandlers(emitter, type)`: the handlers as of now. Later `on`/`off` calls
    /// do not change the returned list. Callers release the borrow of whatever owns
    /// the list before they call a handler from it.
    pub fn snapshot(&self) -> Rc<[Entry<H>]> {
        self.0.clone()
    }
    /// `!hasHandler(emitter, type)`.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn contains(&self, id: ListenerId) -> bool {
        self.0.iter().any(|e| e.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    type H = dyn Fn(&RefCell<Vec<String>>);

    fn run(ls: &Listeners<H>, log: &RefCell<Vec<String>>) {
        for e in ls.snapshot().iter() {
            (e.f)(log);
        }
    }

    #[test]
    fn ids_are_unique_and_never_reused() {
        let mut a: Listeners<H> = Listeners::default();
        let mut b: Listeners<H> = Listeners::default();
        let f: Rc<H> = Rc::new(|_| {});
        let i1 = a.on_rc(f.clone());
        assert!(a.off(i1));
        let i2 = a.on_rc(f.clone());
        let i3 = b.on_rc(f);
        assert!(i1 != i2 && i2 != i3 && i1 != i3);
        assert!(!b.off(i2), "an id names an entry in one list only");
        assert!(!a.off(i1), "removed ids stay removed");
    }

    /// event.js: `off` builds a new array, so the running dispatch still calls a
    /// handler removed by an earlier handler, and does not call one added by it.
    #[test]
    fn snapshot_is_unaffected_by_on_and_off() {
        let ls: Rc<RefCell<Listeners<H>>> = Rc::default();
        let log = RefCell::new(Vec::new());
        let h2: Rc<H> = Rc::new(|l| l.borrow_mut().push("h2".into()));
        let h3: Rc<H> = Rc::new(|l| l.borrow_mut().push("h3".into()));
        {
            let (ls2, h2c, h3c) = (ls.clone(), h2.clone(), h3.clone());
            ls.borrow_mut()
                .on_rc(Rc::new(move |l: &RefCell<Vec<String>>| {
                    l.borrow_mut().push("h1".into());
                    ls2.borrow_mut().off_fn(&h2c);
                    ls2.borrow_mut().on_rc(h3c.clone());
                }));
        }
        ls.borrow_mut().on_rc(h2);
        let snap = ls.borrow().clone();
        run(&snap, &log);
        assert_eq!(*log.borrow(), ["h1", "h2"]);
        log.borrow_mut().clear();
        let snap = ls.borrow().clone();
        run(&snap, &log);
        // h1 adds another h3 each time.
        assert_eq!(*log.borrow(), ["h1", "h3"]);
    }

    /// The same function twice is two entries; `off_fn` removes the first.
    #[test]
    fn duplicate_function_and_identity_removal() {
        let mut ls: Listeners<H> = Listeners::default();
        let log = RefCell::new(Vec::new());
        let dup: Rc<H> = Rc::new(|l| l.borrow_mut().push("dup".into()));
        let first = ls.on_rc(dup.clone());
        let second = ls.on_rc(dup.clone());
        run(&ls, &log);
        assert_eq!(log.borrow().len(), 2);
        assert!(ls.off_fn(&dup));
        assert!(!ls.contains(first) && ls.contains(second));
        let other: Rc<H> = Rc::new(|_| {});
        assert!(!ls.off_fn(&other), "another closure is another function");
        assert!(ls.off_fn(&dup) && ls.is_empty());
    }
}
