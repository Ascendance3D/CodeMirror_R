import re

lib_file = "crates/cm-modes/src/lib.rs"
with open(lib_file, "r") as f:
    content = f.read()

content = content.replace(
"""    fn combine_tokens(&self, state: &State) -> Option<bool> {
        if let Some(ConfigValue::Obj(f)) = self.extensions.get("combineTokens") {
            if let Some(func) = f.downcast_ref::<std::rc::Rc<dyn Fn(&State) -> Option<bool>>>() {
                return func(state);
            }
        }
        self.inner.combine_tokens(state)
    }""",
"""    fn combine_tokens(&self, state: &State) -> Option<bool> {
        if let Some(ConfigValue::Obj(f)) = self.extensions.get("combineTokens") {
            if let Some(func) = f.downcast_ref::<std::rc::Rc<dyn Fn(&State) -> Option<bool>>>() {
                return func(state);
            }
        }
        self.inner.combine_tokens(state)
    }""")

content = content.replace(
"""    fn electric_input(&self, text: &[u16]) -> Option<bool> {
        if let Some(ConfigValue::Obj(f)) = self.extensions.get("electricInput") {
            if let Some(func) = f.downcast_ref::<std::rc::Rc<dyn Fn(&[u16]) -> Option<bool>>>() {
                return func(text);
            }
        }
        self.inner.electric_input(text)
    }""",
"""    fn electric_input(&self, text: &[u16]) -> Option<bool> {
        if let Some(ConfigValue::Obj(f)) = self.extensions.get("electricInput") {
            if let Some(func) = f.downcast_ref::<std::rc::Rc<dyn Fn(&[u16]) -> Option<bool>>>() {
                return func(text);
            }
        }
        self.inner.electric_input(text)
    }""")

# Just using cargo clippy --fix... Wait, they are inside CM_EVIDENCE="" cargo clippy
