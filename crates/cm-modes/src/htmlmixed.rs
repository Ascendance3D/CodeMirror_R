//! Port of `mode/htmlmixed/htmlmixed.js` (CodeMirror 5.65.21): html (the `xml` mode
//! in html configuration) with `<script>` / `<style>` bodies handed to inner modes.
//!
//! Spec options: `multilineTagIndentFactor`, `multilineTagIndentPastTag`,
//! `allowMissingTagName` (passed to xml), `tags` (`ConfigValue::Obj` of
//! [`HtmlTags`]) and `scriptTypes` (`ConfigValue::Obj` of `Vec<ScriptType>`). JS
//! regexps in those become Rust predicates. The tag name is spliced into the end-tag
//! regexp unescaped in JS; here it is matched literally (identical for names without
//! regexp metacharacters).

use std::rc::Rc;

use cm_core::util::string_stream::StringStream;

use crate::js::{self, is_js_space};
use crate::{
    ConfigValue, JsArg, MimeTarget, Mode, ModeOptions, ModeSpec, Registry, Spec, State, copy_state,
    start_state,
};

/// A predicate standing in for a JS `RegExp.test`.
pub type Matcher = Rc<dyn Fn(&[u16]) -> bool>;

/// One `[attr, regexp, mode]` entry of a `tags` list.
#[derive(Clone)]
pub struct TagRule {
    pub attr: Option<String>,
    pub matches: Option<Matcher>,
    pub mode: Spec,
}

/// The `tags` option: tag name -> rules, in object key order.
pub type HtmlTags = Vec<(String, Vec<TagRule>)>;

/// One `{matches, mode}` entry of the `scriptTypes` option.
#[derive(Clone)]
pub struct ScriptType {
    pub matches: Matcher,
    pub mode: Spec,
}

/// `/^...$/i`-style whole-string ASCII case-insensitive test.
fn eq_ci(s: &[u16], lit: &str) -> bool {
    s.len() == lit.len()
        && s.iter()
            .zip(lit.bytes())
            .all(|(&a, b)| a < 0x80 && (a as u8).eq_ignore_ascii_case(&b))
}

/// Case-insensitive search for an ASCII literal anywhere in `s`.
pub fn contains_ci(s: &[u16], lit: &str) -> bool {
    let n = lit.len();
    s.len() >= n && (0..=s.len() - n).any(|i| eq_ci(&s[i..i + n], lit))
}

fn strip_prefix_ci<'a>(s: &'a [u16], lit: &str) -> Option<&'a [u16]> {
    (s.len() >= lit.len() && eq_ci(&s[..lit.len()], lit)).then(|| &s[lit.len()..])
}

/// htmlmixed.js `defaultTags`.
fn default_tags() -> HtmlTags {
    let m = |f: fn(&[u16]) -> bool| -> Option<Matcher> { Some(Rc::new(f)) };
    // /./ : any unit but a line terminator.
    let any = |s: &[u16]| {
        s.iter()
            .any(|&c| !matches!(c, 0x0a | 0x0d | 0x2028 | 0x2029))
    };
    vec![
        (
            "script".into(),
            vec![
                // ["lang", /(javascript|babel)/i, "javascript"]
                TagRule {
                    attr: Some("lang".into()),
                    matches: m(|s| contains_ci(s, "javascript") || contains_ci(s, "babel")),
                    mode: Spec::from("javascript"),
                },
                // ["type", /^(?:text|application)\/(?:x-)?(?:java|ecma)script$|^module$|^$/i, "javascript"]
                TagRule {
                    attr: Some("type".into()),
                    matches: m(|s| {
                        if s.is_empty() || eq_ci(s, "module") {
                            return true;
                        }
                        let Some(r) = strip_prefix_ci(s, "text/")
                            .or_else(|| strip_prefix_ci(s, "application/"))
                        else {
                            return false;
                        };
                        let r = strip_prefix_ci(r, "x-").unwrap_or(r);
                        eq_ci(r, "javascript") || eq_ci(r, "ecmascript")
                    }),
                    mode: Spec::from("javascript"),
                },
                TagRule {
                    attr: Some("type".into()),
                    matches: m(any),
                    mode: Spec::from("text/plain"),
                },
                TagRule {
                    attr: None,
                    matches: None,
                    mode: Spec::from("javascript"),
                },
            ],
        ),
        (
            "style".into(),
            vec![
                // ["lang", /^css$/i, "css"]
                TagRule {
                    attr: Some("lang".into()),
                    matches: m(|s| eq_ci(s, "css")),
                    mode: Spec::from("css"),
                },
                // ["type", /^(text\/)?(x-)?(stylesheet|css)$/i, "css"]
                TagRule {
                    attr: Some("type".into()),
                    matches: m(|s| {
                        let r = strip_prefix_ci(s, "text/").unwrap_or(s);
                        let r = strip_prefix_ci(r, "x-").unwrap_or(r);
                        eq_ci(r, "stylesheet") || eq_ci(r, "css")
                    }),
                    mode: Spec::from("css"),
                },
                TagRule {
                    attr: Some("type".into()),
                    matches: m(any),
                    mode: Spec::from("text/plain"),
                },
                TagRule {
                    attr: None,
                    matches: None,
                    mode: Spec::from("css"),
                },
            ],
        ),
    ]
}

/// htmlmixed.js: `addTags(from, to)`: each source list is prepended to the target's.
fn add_tags(from: &HtmlTags, to: &mut HtmlTags) {
    for (tag, source) in from {
        let i = match to.iter().position(|(t, _)| t == tag) {
            Some(i) => i,
            None => {
                to.push((tag.clone(), Vec::new()));
                to.len() - 1
            }
        };
        let dest = &mut to[i].1;
        for r in source.iter().rev() {
            dest.insert(0, r.clone());
        }
    }
}

/// `/^\s*(.*?)\s*$/.exec(v)[1]`. `.` stops at line terminators; when the value holds
/// one that is not trailing space, the JS exec returns null and `[1]` throws.
fn trim_lazy(v: &[u16]) -> &[u16] {
    let v = &v[v.iter().take_while(|&&c| is_js_space(c)).count()..];
    let is_term = |c: u16| matches!(c, 0x0a | 0x0d | 0x2028 | 0x2029);
    for end in 0..=v.len() {
        if end > 0 && is_term(v[end - 1]) {
            break;
        }
        if v[end..].iter().all(|&c| is_js_space(c)) {
            return &v[..end];
        }
    }
    panic!("htmlmixed getAttrValue: TypeError, /^\\s*(.*?)\\s*$/ does not match");
}

/// htmlmixed.js: `getAttrValue(text, attr)` with
/// `new RegExp("\\s+" + attr + "\\s*=\\s*('|\")?([^'\"]+)('|\")?\\s*")`.
fn get_attr_value(text: &[u16], attr: &str) -> Vec<u16> {
    let a = js::to_u16(attr);
    let quote = |c: u16| c == b'\'' as u16 || c == b'"' as u16;
    let nonquote_run = |from: usize| text[from..].iter().take_while(|&&c| !quote(c)).count();
    for j in 1..text.len() {
        // `\s+` must end right before `attr`.
        if !is_js_space(text[j - 1]) || !text[j..].starts_with(&a) {
            continue;
        }
        let mut p = j + a.len();
        p += text[p..].iter().take_while(|&&c| is_js_space(c)).count();
        if text.get(p) != Some(&(b'=' as u16)) {
            continue;
        }
        p += 1;
        let k = text[p..].iter().take_while(|&&c| is_js_space(c)).count();
        let q = p + k;
        // Greedy `\s*`, optional quote, then `([^'"]+)`.
        if text.get(q).is_some_and(|&c| quote(c)) {
            let n = nonquote_run(q + 1);
            if n > 0 {
                return trim_lazy(&text[q + 1..q + 1 + n]).to_vec();
            }
        } else {
            let n = nonquote_run(q);
            if n > 0 {
                return trim_lazy(&text[q..q + n]).to_vec();
            }
        }
        // Backtrack `\s*` by one: the group starts at the last space.
        if k > 0 {
            let n = nonquote_run(q - 1);
            return trim_lazy(&text[q - 1..q - 1 + n]).to_vec();
        }
    }
    Vec::new()
}

/// htmlmixed.js: `findMatchingMode(tagInfo, tagText)`; `None` = undefined.
fn find_matching_mode(tag_info: &[TagRule], tag_text: &[u16]) -> Option<Spec> {
    for r in tag_info {
        match &r.attr {
            None => return Some(r.mode.clone()),
            Some(attr) if attr.is_empty() => return Some(r.mode.clone()),
            Some(attr) => {
                let m = r.matches.as_ref().expect("tag rule regexp");
                if m(&get_attr_value(tag_text, attr)) {
                    return Some(r.mode.clone());
                }
            }
        }
    }
    None
}

/// `/^<\/\s*name\s*>/i` at the start of `s`: the match length.
fn end_tag_at(s: &[u16], name: &[u16]) -> Option<usize> {
    if s.len() < 2 || s[0] != b'<' as u16 || s[1] != b'/' as u16 {
        return None;
    }
    let mut p = 2 + s[2..].iter().take_while(|&&c| is_js_space(c)).count();
    if s.len() < p + name.len()
        || !s[p..p + name.len()].iter().zip(name).all(|(&a, &b)| {
            a == b || (a < 0x80 && b < 0x80 && (a as u8).eq_ignore_ascii_case(&(b as u8)))
        })
    {
        return None;
    }
    p += name.len();
    p += s[p..].iter().take_while(|&&c| is_js_space(c)).count();
    (s.get(p) == Some(&(b'>' as u16))).then_some(p + 1)
}

/// `str.search(/<\/\s*name\s*>/i)`.
fn search_end_tag(s: &[u16], name: &[u16]) -> Option<usize> {
    (0..s.len()).find(|&i| end_tag_at(&s[i..], name).is_some())
}

/// `/\btag\b/.test(style)`.
fn has_tag_word(style: &str) -> bool {
    let b = style.as_bytes();
    let w = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    (0..b.len().saturating_sub(2)).any(|i| {
        &b[i..i + 3] == b"tag" && (i == 0 || !w(b[i - 1])) && (i + 3 == b.len() || !w(b[i + 3]))
    })
}

/// `state.token`: `html`, or the closure installed for a script/style body.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenFn {
    Html,
    /// The inner closure; the end-tag regexps are built from this tag name.
    Local(Vec<u16>),
}

#[derive(Clone)]
pub struct HtmlMixedState {
    pub token: TokenFn,
    pub in_tag: Option<Vec<u16>>,
    pub local_mode: Option<Rc<dyn Mode>>,
    pub local_state: Option<Box<State>>,
    pub html_state: Box<State>,
}

impl std::fmt::Debug for HtmlMixedState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HtmlMixedState")
            .field("token", &self.token)
            .field("in_tag", &self.in_tag)
            .field("html_state", &self.html_state)
            .finish()
    }
}

pub struct HtmlMixedMode {
    html_mode: Rc<dyn Mode>,
    tags: HtmlTags,
    registry: Registry,
    options: ModeOptions,
}

impl HtmlMixedMode {
    pub fn new(registry: &Registry, options: &ModeOptions, spec: &ModeSpec) -> HtmlMixedMode {
        let mut xml = ModeSpec::named("xml").with("htmlMode", ConfigValue::Bool(true));
        for k in [
            "multilineTagIndentFactor",
            "multilineTagIndentPastTag",
            "allowMissingTagName",
        ] {
            if let Some(v) = spec.get(k) {
                xml = xml.with(k, v.clone());
            }
        }
        let html_mode = registry.get_mode(options, &Spec::Obj(xml));
        let mut tags = HtmlTags::new();
        add_tags(&default_tags(), &mut tags);
        if let Some(t) = spec.get("tags").and_then(|v| v.as_obj::<HtmlTags>()) {
            add_tags(t, &mut tags);
        }
        if let Some(st) = spec
            .get("scriptTypes")
            .and_then(|v| v.as_obj::<Vec<ScriptType>>())
        {
            // `tags.script` always exists (defaults).
            let script = &mut tags.iter_mut().find(|(t, _)| t == "script").unwrap().1;
            for s in st.iter().rev() {
                script.insert(
                    0,
                    TagRule {
                        attr: Some("type".into()),
                        matches: Some(s.matches.clone()),
                        mode: s.mode.clone(),
                    },
                );
            }
        }
        HtmlMixedMode {
            html_mode,
            tags,
            registry: registry.clone(),
            options: options.clone(),
        }
    }

    fn tag_rules(&self, name: &[u16]) -> Option<&Vec<TagRule>> {
        let name = js::from_u16(name);
        self.tags.iter().find(|(t, _)| *t == name).map(|(_, r)| r)
    }

    /// htmlmixed.js: `html(stream, state)`
    fn html(&self, stream: &mut StringStream<'_>, state: &mut HtmlMixedState) -> Option<String> {
        let style = self.html_mode.token(stream, &mut state.html_state);
        let tag = style.as_deref().is_some_and(has_tag_word);
        let cur = stream.current();
        let tag_name = state
            .html_state
            .get::<crate::xml::XmlState>()
            .tag_name
            .as_ref()
            .filter(|t| !t.is_empty())
            .map(|t| js::to_lower(t));
        let plain = !cur
            .iter()
            .any(|&c| c == b'<' as u16 || c == b'>' as u16 || c == b'/' as u16 || is_js_space(c));
        if tag
            && plain
            && tag_name
                .as_ref()
                .is_some_and(|t| !t.is_empty() && self.tag_rules(t).is_some())
        {
            let mut t = tag_name.unwrap();
            t.push(b' ' as u16);
            state.in_tag = Some(t);
        } else if state.in_tag.is_some() && tag && cur.last() == Some(&(b'>' as u16)) {
            // /^([\S]+) (.*)/.exec(state.inTag): the tag name, then the text up to the
            // first line terminator.
            let in_tag = state.in_tag.take().unwrap();
            let sp = in_tag
                .iter()
                .position(|&c| is_js_space(c))
                .expect("inTag has a space");
            let (name, rest) = (&in_tag[..sp], &in_tag[sp + 1..]);
            let rest = &rest[..rest
                .iter()
                .position(|&c| matches!(c, 0x0a | 0x0d | 0x2028 | 0x2029))
                .unwrap_or(rest.len())];
            // `stream.current() == ">" && findMatchingMode(...)`: false / undefined -> "null".
            let mode_spec = if js::eq(cur, ">") {
                self.tag_rules(name)
                    .and_then(|r| find_matching_mode(r, rest))
            } else {
                None
            };
            let mode = self
                .registry
                .get_mode(&self.options, &mode_spec.unwrap_or(Spec::Null));
            state.token = TokenFn::Local(name.to_vec());
            let base: JsArg = self.html_mode.indent(&state.html_state, &[], &[]).into();
            state.local_state = Some(Box::new(start_state(&*mode, base, JsArg::Undefined)));
            state.local_mode = Some(mode);
        } else if let Some(in_tag) = &mut state.in_tag {
            in_tag.extend_from_slice(cur);
            if stream.eol() {
                in_tag.push(b' ' as u16);
            }
        }
        style
    }

    /// The closure htmlmixed installs as `state.token` for a script/style body.
    fn local(
        &self,
        stream: &mut StringStream<'_>,
        state: &mut HtmlMixedState,
        name: &[u16],
    ) -> Option<String> {
        if end_tag_at(&stream.string[stream.pos..], name).is_some() {
            state.token = TokenFn::Html;
            state.local_state = None;
            state.local_mode = None;
            return None;
        }
        let mode = state.local_mode.clone().expect("localMode");
        let style = mode.token(stream, state.local_state.as_mut().expect("localState"));
        // htmlmixed.js: maybeBackup(stream, endTag, style)
        let cur = stream.current().to_vec();
        if let Some(close) = search_end_tag(&cur, name) {
            stream.back_up(cur.len() - close);
        } else if cur.last() == Some(&(b'<' as u16)) || cur.ends_with(&[b'<' as u16, b'/' as u16]) {
            stream.back_up(cur.len());
            if end_tag_at(&stream.string[stream.pos..], name).is_none() {
                stream.pos += cur.len(); // stream.match(cur)
            }
        }
        style
    }
}

impl Mode for HtmlMixedMode {
    fn name(&self) -> &str {
        "htmlmixed"
    }
    fn start_state(&self, _a1: JsArg, _a2: JsArg) -> Option<State> {
        let html = start_state(&*self.html_mode, JsArg::Undefined, JsArg::Undefined);
        Some(State::new(HtmlMixedState {
            token: TokenFn::Html,
            in_tag: None,
            local_mode: None,
            local_state: None,
            html_state: Box::new(html),
        }))
    }
    fn copy_state(&self, state: &State) -> Option<State> {
        let s = state.get::<HtmlMixedState>();
        let local = match (&s.local_state, &s.local_mode) {
            (Some(ls), Some(lm)) => Some(Box::new(copy_state(&**lm, ls))),
            (Some(ls), None) => Some(ls.clone()),
            _ => None,
        };
        Some(State::new(HtmlMixedState {
            token: s.token.clone(),
            in_tag: s.in_tag.clone(),
            local_mode: s.local_mode.clone(),
            local_state: local,
            html_state: Box::new(copy_state(&*self.html_mode, &s.html_state)),
        }))
    }
    fn token(&self, stream: &mut StringStream<'_>, state: &mut State) -> Option<String> {
        let s = state.get_mut::<HtmlMixedState>();
        match s.token.clone() {
            TokenFn::Html => self.html(stream, s),
            TokenFn::Local(name) => self.local(stream, s, &name),
        }
    }
    fn indent(&self, state: &State, text_after: &[u16], line: &[u16]) -> Option<f64> {
        let s = state.get::<HtmlMixedState>();
        // /^\s*<\//
        let ws = js::space_len(text_after);
        let closing = text_after[ws..].starts_with(&[b'<' as u16, b'/' as u16]);
        match &s.local_mode {
            Some(lm) if !closing => {
                if lm.has_indent() {
                    lm.indent(
                        s.local_state.as_ref().expect("localState"),
                        text_after,
                        line,
                    )
                } else {
                    None
                }
            }
            _ => self.html_mode.indent(&s.html_state, text_after, line),
        }
    }
    fn has_indent(&self) -> bool {
        true
    }
    fn inner_mode(&self, state: &State) -> Option<(Rc<dyn Mode>, State)> {
        let s = state.get::<HtmlMixedState>();
        Some(match (&s.local_mode, &s.local_state) {
            (Some(m), Some(ls)) => (m.clone(), (**ls).clone()),
            (Some(m), None) => (m.clone(), (*s.html_state).clone()),
            (None, Some(ls)) => (self.html_mode.clone(), (**ls).clone()),
            (None, None) => (self.html_mode.clone(), (*s.html_state).clone()),
        })
    }
}

/// `defineMode("htmlmixed", ..., "xml", "javascript", "css")` and
/// `defineMIME("text/html", "htmlmixed")`.
pub fn register(r: &mut Registry) {
    r.define_mode(
        "htmlmixed",
        Rc::new(|reg, options, spec| {
            Rc::new(HtmlMixedMode::new(reg, options, spec)) as Rc<dyn Mode>
        }),
    );
    r.define_mime("text/html", MimeTarget::Name("htmlmixed".into()));
}

/// Helper for building a `tags` / `scriptTypes` option value.
pub fn obj<T: 'static>(v: T) -> ConfigValue {
    ConfigValue::Obj(Rc::new(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cm_core::text::to_u16;

    #[test]
    fn attr_value() {
        let v = |t: &str, a: &str| js::from_u16(&get_attr_value(&to_u16(t), a));
        assert_eq!(v(" type=\"text/css\" x", "type"), "text/css");
        assert_eq!(v(" type = 'a b ' ", "type"), "a b");
        assert_eq!(v(" type=", "type"), "");
        assert_eq!(v(" type=  \"\"", "type"), "");
        assert_eq!(v("type=x", "type"), "");
        assert_eq!(v(" xtype=1 type=2", "type"), "2");
    }

    #[test]
    fn end_tags() {
        assert_eq!(
            end_tag_at(&to_u16("</ SCRIPT >x"), &to_u16("script")),
            Some(11)
        );
        assert_eq!(
            search_end_tag(&to_u16("a</style>"), &to_u16("style")),
            Some(1)
        );
        assert!(
            has_tag_word("tag bracket")
                && has_tag_word("x tag")
                && !has_tag_word("tagx")
                && !has_tag_word("attribute")
        );
    }
}
