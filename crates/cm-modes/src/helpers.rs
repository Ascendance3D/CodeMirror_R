//! `CodeMirror.registerHelper` / `registerGlobalHelper` and the mode half of
//! `cm.getHelpers(pos, type)` (`src/edit/methods.js`).
//!
//! Modes register helpers next to their definition: word lists for
//! `addon/hint` (`hintWords`), `wordChars` regexps, fold and lint functions. A
//! helper value is any JS value, so here it is an `Rc<dyn Any>` the consumer
//! downcasts (`hintWords` is a `Vec<String>`, `wordChars` a [`RegExp`](crate::RegExp)).
//! The JS tables are global and also exposed as `CodeMirror[type]`; here they live
//! in [`Registry::helpers`](crate::Registry), read through [`Helpers::table`].

use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;

use crate::{ConfigValue, Mode};

/// A helper value.
pub type HelperValue = Rc<dyn Any>;

/// A `registerGlobalHelper` predicate: `pred(mode, cm)`. The editor is passed as
/// `&dyn Any` (cm-modes does not know the editor type); `getHelpers` callers without
/// an editor pass `&()`.
pub type HelperPredicate = Rc<dyn Fn(&dyn Mode, &dyn Any) -> bool>;

/// `helpers[type]`: helpers by name, plus `_global`.
#[derive(Clone, Default)]
pub struct HelperTable {
    pub named: HashMap<String, HelperValue>,
    /// `_global`: `{pred, val}` in registration order.
    pub global: Vec<(HelperPredicate, HelperValue)>,
}

/// Every helper type.
#[derive(Clone, Default)]
pub struct Helpers {
    types: HashMap<String, HelperTable>,
}

impl Helpers {
    /// `CodeMirror.registerHelper(type, name, value)`.
    pub fn register(&mut self, ty: &str, name: &str, value: HelperValue) {
        self.types
            .entry(ty.to_string())
            .or_default()
            .named
            .insert(name.to_string(), value);
    }

    /// `CodeMirror.registerGlobalHelper(type, name, predicate, value)`.
    pub fn register_global(
        &mut self,
        ty: &str,
        name: &str,
        predicate: HelperPredicate,
        value: HelperValue,
    ) {
        self.register(ty, name, value.clone());
        self.types
            .get_mut(ty)
            .unwrap()
            .global
            .push((predicate, value));
    }

    /// `CodeMirror[type]` (`helpers[type]`).
    pub fn table(&self, ty: &str) -> Option<&HelperTable> {
        self.types.get(ty)
    }

    /// `CodeMirror[type][name]`.
    pub fn get(&self, ty: &str, name: &str) -> Option<&HelperValue> {
        self.types.get(ty)?.named.get(name)
    }

    /// `getHelpers(pos, type)` once `getModeAt(pos)` is known: the helpers named by
    /// `mode[type]` (a string or a list of names, as `ConfigValue::Obj(Rc<Vec<String>>)`),
    /// else the one for `mode.helperType`, else the one for `mode.name`; then every
    /// global helper whose predicate accepts the mode and that is not already found.
    pub fn for_mode(&self, mode: &dyn Mode, ty: &str, cm: &dyn Any) -> Vec<HelperValue> {
        let mut found: Vec<HelperValue> = Vec::new();
        let Some(help) = self.types.get(ty) else {
            return found;
        };
        match mode.prop(ty) {
            Some(ConfigValue::Str(name)) => {
                if let Some(v) = help.named.get(&name) {
                    found.push(v.clone());
                }
            }
            // `mode[type][i]` for i < `mode[type].length`: other truthy values have no
            // length and name nothing.
            Some(v) if v.truthy() => {
                if let Some(names) = v.as_obj::<Vec<String>>() {
                    for n in names {
                        if let Some(v) = help.named.get(n) {
                            found.push(v.clone());
                        }
                    }
                }
            }
            _ => {
                if let Some(v) = mode.helper_type().and_then(|t| help.named.get(t)) {
                    found.push(v.clone());
                } else if let Some(v) = help.named.get(mode.name()) {
                    found.push(v.clone());
                }
            }
        }
        for (pred, val) in &help.global {
            if pred(mode, cm) && !found.iter().any(|f| Rc::ptr_eq(f, val)) {
                found.push(val.clone());
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModeOptions, ModeSpec, Registry, Spec};

    fn val(s: &str) -> HelperValue {
        Rc::new(s.to_string())
    }
    fn names(v: &[HelperValue]) -> Vec<String> {
        v.iter()
            .map(|h| h.downcast_ref::<String>().unwrap().clone())
            .collect()
    }

    /// The lookup order of `getHelpers` (test/test.js "helpers" and "getHelpers").
    #[test]
    fn lookup_order() {
        let mut r = Registry::with_all_modes();
        r.helpers.register("hint", "javascript", val("js"));
        r.helpers.register("hint", "json", val("json"));
        r.helpers.register("hint", "a", val("A"));
        r.helpers.register("hint", "b", val("B"));
        r.helpers.register_global(
            "hint",
            "g",
            Rc::new(|m: &dyn Mode, _: &dyn Any| m.name() == "javascript"),
            val("G"),
        );
        let opts = ModeOptions::default();
        let js = r.get_mode(&opts, &Spec::from("javascript"));
        assert_eq!(names(&r.helpers.for_mode(&*js, "hint", &())), ["js", "G"]);
        // `helperType` wins over the name.
        let spec = ModeSpec {
            helper_type: Some("json".into()),
            ..ModeSpec::named("javascript")
        };
        let m = r.get_mode(&opts, &Spec::Obj(spec));
        assert_eq!(names(&r.helpers.for_mode(&*m, "hint", &())), ["json", "G"]);
        // `mode[type]` as a string, then as a list of names.
        let mut props = HashMap::new();
        props.insert("hint".to_string(), ConfigValue::Str("b".into()));
        let spec = ModeSpec {
            mode_props: Some(props),
            ..ModeSpec::named("javascript")
        };
        let m = r.get_mode(&opts, &Spec::Obj(spec));
        assert_eq!(names(&r.helpers.for_mode(&*m, "hint", &())), ["B", "G"]);
        let mut props = HashMap::new();
        let list: Rc<dyn Any> = Rc::new(vec!["a".to_string(), "x".into(), "b".into()]);
        props.insert("hint".to_string(), ConfigValue::Obj(list));
        let spec = ModeSpec {
            mode_props: Some(props),
            ..ModeSpec::named("javascript")
        };
        let m = r.get_mode(&opts, &Spec::Obj(spec));
        assert_eq!(
            names(&r.helpers.for_mode(&*m, "hint", &())),
            ["A", "B", "G"]
        );
        // A global helper already found by name is not added twice.
        let g = r.helpers.get("hint", "g").unwrap().clone();
        r.helpers
            .register_global("hint", "g2", Rc::new(|_: &dyn Mode, _: &dyn Any| true), g);
        assert_eq!(names(&r.helpers.for_mode(&*js, "hint", &())), ["js", "G"]);
        let css = r.get_mode(&opts, &Spec::from("css"));
        assert_eq!(names(&r.helpers.for_mode(&*css, "hint", &())), ["G"]);
        assert!(r.helpers.for_mode(&*css, "nope", &()).is_empty());
    }
}
