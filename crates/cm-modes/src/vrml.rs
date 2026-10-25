//! A VRML97 (ISO/IEC 14772-1) classic-encoding mode. New code: CodeMirror 5 has no
//! VRML mode. This is the Rust twin of `oracle/cm5-vrml-mode.js`, a CM5 mode written
//! for this port; `node oracle/cm5-modes-tokens.mjs vrml` runs the JS one and
//! `tests/vrml_oracle.rs` requires identical tokens and indentation. Any change must
//! be made in both files. The style table is in the JS file's header comment.

use std::rc::Rc;

use cm_core::util::string_stream::{RegexMatch, StringStream};

use crate::js::{is_digit, is_hex, is_js_space};
use crate::{ConfigValue, JsArg, MimeTarget, Mode, ModeOptions, Registry, State};

const KEYWORDS: &[&str] = &[
    "DEF",
    "USE",
    "PROTO",
    "EXTERNPROTO",
    "IS",
    "ROUTE",
    "TO",
    "PROFILE",
    "COMPONENT",
    "META",
    "UNIT",
    "IMPORT",
    "EXPORT",
    "AS",
];
const ATOMS: &[&str] = &["TRUE", "FALSE", "NULL"];
const ACCESS_TYPES: &[&str] = &[
    "field",
    "exposedField",
    "eventIn",
    "eventOut",
    "initializeOnly",
    "inputOnly",
    "outputOnly",
    "inputOutput",
];
const FIELD_TYPES: &[&str] = &[
    "SFBool",
    "SFColor",
    "SFColorRGBA",
    "SFDouble",
    "SFFloat",
    "SFImage",
    "SFInt32",
    "SFMatrix3d",
    "SFMatrix3f",
    "SFMatrix4d",
    "SFMatrix4f",
    "SFNode",
    "SFRotation",
    "SFString",
    "SFTime",
    "SFVec2d",
    "SFVec2f",
    "SFVec3d",
    "SFVec3f",
    "SFVec4d",
    "SFVec4f",
    "MFBool",
    "MFColor",
    "MFColorRGBA",
    "MFDouble",
    "MFFloat",
    "MFImage",
    "MFInt32",
    "MFMatrix3d",
    "MFMatrix3f",
    "MFMatrix4d",
    "MFMatrix4f",
    "MFNode",
    "MFRotation",
    "MFString",
    "MFTime",
    "MFVec2d",
    "MFVec2f",
    "MFVec3d",
    "MFVec3f",
    "MFVec4d",
    "MFVec4f",
];
/// The 54 VRML97 node types.
const NODE_TYPES: &[&str] = &[
    "Anchor",
    "Appearance",
    "AudioClip",
    "Background",
    "Billboard",
    "Box",
    "Collision",
    "Color",
    "ColorInterpolator",
    "Cone",
    "Coordinate",
    "CoordinateInterpolator",
    "Cylinder",
    "CylinderSensor",
    "DirectionalLight",
    "ElevationGrid",
    "Extrusion",
    "Fog",
    "FontStyle",
    "Group",
    "ImageTexture",
    "IndexedFaceSet",
    "IndexedLineSet",
    "Inline",
    "LOD",
    "Material",
    "MovieTexture",
    "NavigationInfo",
    "Normal",
    "NormalInterpolator",
    "OrientationInterpolator",
    "PixelTexture",
    "PlaneSensor",
    "PointLight",
    "PointSet",
    "PositionInterpolator",
    "ProximitySensor",
    "ScalarInterpolator",
    "Script",
    "Shape",
    "Sound",
    "Sphere",
    "SphereSensor",
    "SpotLight",
    "Switch",
    "Text",
    "TextureCoordinate",
    "TextureTransform",
    "TimeSensor",
    "TouchSensor",
    "Transform",
    "Viewpoint",
    "VisibilitySensor",
    "WorldInfo",
];

fn is_in(set: &[&str], word: &[u16]) -> bool {
    set.iter()
        .any(|w| w.encode_utf16().eq(word.iter().copied()))
}

/// What the previous name-introducing token expects next (`state.expect`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    Def,
    Use,
    Route,
    RouteDot,
    RouteField,
    Is,
    FieldType,
    FieldName,
}

/// An open `{` or `[`.
#[derive(Debug)]
pub struct Context {
    pub prev: Option<Rc<Context>>,
    pub ty: u16,
    pub base: f64,
    pub indent: f64,
}

#[derive(Clone, Debug)]
pub struct VrmlState {
    pub in_string: bool,
    pub context: Option<Rc<Context>>,
    pub expect: Option<Expect>,
    pub indented: f64,
    pub base: JsArg,
    pub at_start: bool,
}

pub struct VrmlMode {
    indent_unit: f64,
}

/// IdFirstChar: not a control, space, `"` `#` `'` `+` `,` `-` `.`, digit, `[` `\` `]`,
/// `{` `}` or DEL.
fn id_first(c: u16) -> bool {
    !(c <= 0x20
        || matches!(c, 0x22 | 0x23 | 0x27 | 0x2b..=0x2e | 0x30..=0x39 | 0x5b..=0x5d | 0x7b | 0x7d | 0x7f))
}

/// IdRestChar: as [`id_first`], but `+`, `-` and digits are allowed.
fn id_rest(c: u16) -> bool {
    !(c <= 0x20
        || matches!(
            c,
            0x22 | 0x23 | 0x27 | 0x2c | 0x2e | 0x5b..=0x5d | 0x7b | 0x7d | 0x7f
        ))
}

/// `/^[+\-]?(?:0[xX][0-9a-fA-F]+|(?:\d+\.?\d*|\.\d+)(?:[eE][+\-]?\d+)?)/`
fn re_number(s: &[u16]) -> Option<RegexMatch> {
    let mut i = usize::from(matches!(s.first(), Some(&0x2b) | Some(&0x2d)));
    if s.get(i) == Some(&(b'0' as u16)) && matches!(s.get(i + 1), Some(&0x78) | Some(&0x58)) {
        let n = s[i + 2..].iter().take_while(|&&c| is_hex(c)).count();
        if n > 0 {
            return Some(RegexMatch {
                index: 0,
                len: i + 2 + n,
            });
        }
    }
    let digits = |from: usize| {
        s[from.min(s.len())..]
            .iter()
            .take_while(|&&c| is_digit(c))
            .count()
    };
    let d = digits(i);
    if d > 0 {
        i += d;
        if s.get(i) == Some(&(b'.' as u16)) {
            i += 1;
            i += digits(i);
        }
    } else if s.get(i) == Some(&(b'.' as u16)) && digits(i + 1) > 0 {
        i += 1 + digits(i + 1);
    } else {
        return None;
    }
    if matches!(s.get(i), Some(&0x65) | Some(&0x45)) {
        let j = i + 1 + usize::from(matches!(s.get(i + 1), Some(&0x2b) | Some(&0x2d)));
        let e = digits(j);
        if e > 0 {
            i = j + e;
        }
    }
    Some(RegexMatch { index: 0, len: i })
}

/// `/^#(VRML|X3D) V/`
fn is_header(s: &[u16]) -> bool {
    crate::js::starts_with(s, "#VRML V") || crate::js::starts_with(s, "#X3D V")
}

/// vrml.js: `tokenString`
fn token_string(stream: &mut StringStream<'_>, state: &mut VrmlState) -> Option<&'static str> {
    while let Some(ch) = stream.next() {
        if ch == b'\\' as u16 {
            stream.next();
        } else if ch == b'"' as u16 {
            state.in_string = false;
            break;
        }
    }
    Some("string")
}

impl VrmlMode {
    /// vrml.js: `tokenBase`
    fn token_base(
        &self,
        stream: &mut StringStream<'_>,
        state: &mut VrmlState,
    ) -> Option<&'static str> {
        let ch = stream.peek().unwrap_or(0);
        if ch == b'#' as u16 {
            let header =
                state.at_start && stream.pos == 0 && is_header(&stream.string[stream.pos..]);
            stream.skip_to_end();
            return Some(if header { "meta" } else { "comment" });
        }
        let expect = state.expect.take();
        if ch == b'"' as u16 {
            stream.next();
            state.in_string = true;
            return token_string(stream, state);
        }
        if ch == b'{' as u16 || ch == b'[' as u16 {
            stream.next();
            state.context = Some(Rc::new(Context {
                prev: state.context.take(),
                ty: ch,
                base: state.indented,
                indent: state.indented + self.indent_unit,
            }));
            return Some("bracket");
        }
        if ch == b'}' as u16 || ch == b']' as u16 {
            stream.next();
            let open = if ch == b'}' as u16 { b'{' } else { b'[' } as u16;
            if let Some(cx) = state.context.clone().filter(|c| c.ty == open) {
                state.context = cx.prev.clone();
                return Some("bracket");
            }
            return Some("error");
        }
        if ch == b',' as u16 {
            stream.eat_while_char(b',' as u16);
            state.expect = expect;
            return None;
        }
        if stream.match_with(re_number, true).is_some() {
            return Some("number");
        }
        if ch == b'.' as u16 {
            stream.next();
            if expect == Some(Expect::RouteDot) {
                state.expect = Some(Expect::RouteField);
            }
            return None;
        }
        if !id_first(ch) {
            stream.next();
            return Some("error");
        }
        let start = stream.pos;
        stream.next();
        stream.eat_while(id_rest);
        let word = &stream.string[start..stream.pos];
        if is_in(KEYWORDS, word) {
            state.expect = match String::from_utf16_lossy(word).as_str() {
                "DEF" | "PROTO" | "EXTERNPROTO" => Some(Expect::Def),
                "USE" => Some(Expect::Use),
                "ROUTE" | "TO" => Some(Expect::Route),
                "IS" => Some(Expect::Is),
                _ => None,
            };
            return Some("keyword");
        }
        if is_in(ATOMS, word) {
            return Some("atom");
        }
        if is_in(ACCESS_TYPES, word) {
            state.expect = Some(Expect::FieldType);
            return Some("keyword");
        }
        match expect {
            Some(Expect::FieldType) => {
                state.expect = Some(Expect::FieldName);
                return Some("type");
            }
            Some(Expect::FieldName) | Some(Expect::Def) => return Some("def"),
            Some(Expect::Use) => return Some("variable"),
            Some(Expect::Route) => {
                state.expect = Some(Expect::RouteDot);
                return Some("variable");
            }
            Some(Expect::RouteField) | Some(Expect::Is) => return Some("property"),
            _ => {}
        }
        if is_in(FIELD_TYPES, word) {
            return Some("type");
        }
        if is_in(NODE_TYPES, word) {
            return Some("tag");
        }
        // stream.match(/^\s*\{/, false)
        let rest = &stream.string[stream.pos..];
        let ws = rest.iter().take_while(|&&c| is_js_space(c)).count();
        if rest.get(ws) == Some(&(b'{' as u16)) {
            return Some("variable-2");
        }
        Some("property")
    }
}

impl Mode for VrmlMode {
    fn name(&self) -> &str {
        "vrml"
    }
    fn start_state(&self, base: JsArg, _a2: JsArg) -> Option<State> {
        Some(State::new(VrmlState {
            in_string: false,
            context: None,
            expect: None,
            indented: base.or_zero(),
            base,
            at_start: true,
        }))
    }
    fn token(&self, stream: &mut StringStream<'_>, state: &mut State) -> Option<String> {
        let state = state.get_mut::<VrmlState>();
        if stream.sol() {
            state.indented = stream.indentation() as f64;
        }
        let style = if state.in_string {
            token_string(stream, state)
        } else if stream.eat_space() {
            None
        } else {
            self.token_base(stream, state)
        };
        state.at_start = false;
        style.map(str::to_string)
    }
    fn blank_line(&self, state: &mut State) -> Option<()> {
        state.get_mut::<VrmlState>().at_start = false;
        Some(())
    }
    fn has_blank_line(&self) -> bool {
        true
    }
    fn indent(&self, state: &State, text_after: &[u16], _line: &[u16]) -> Option<f64> {
        let s = state.get::<VrmlState>();
        if s.in_string {
            return None;
        }
        let Some(cx) = &s.context else {
            // `state.base` is `base || 0`; a Pass base is returned as is.
            return if s.base == JsArg::Pass {
                None
            } else {
                Some(s.base.or_zero())
            };
        };
        let ch = text_after.first().copied();
        if ch == Some(b'}' as u16) && cx.ty == b'{' as u16
            || ch == Some(b']' as u16) && cx.ty == b'[' as u16
        {
            return Some(cx.base);
        }
        Some(cx.indent)
    }
    fn has_indent(&self) -> bool {
        true
    }
    fn electric_chars(&self) -> Option<&str> {
        Some("}]")
    }
    fn line_comment(&self) -> Option<&str> {
        Some("#")
    }
    fn prop(&self, name: &str) -> Option<ConfigValue> {
        match name {
            "electricChars" => Some(ConfigValue::Str("}]".into())),
            "lineComment" => Some(ConfigValue::Str("#".into())),
            "fold" => Some(ConfigValue::Str("brace".into())),
            "closeBrackets" => Some(ConfigValue::Str("[]{}\"\"".into())),
            _ => None,
        }
    }
}

/// `defineMode("vrml", ...)` and its MIME types.
pub fn register(r: &mut Registry) {
    r.define_mode(
        "vrml",
        Rc::new(|_, options: &ModeOptions, _| {
            Rc::new(VrmlMode {
                indent_unit: options.indent_unit as f64,
            }) as Rc<dyn Mode>
        }),
    );
    for mime in ["model/vrml", "x-world/x-vrml", "model/x3d+vrml"] {
        r.define_mime(mime, MimeTarget::Name("vrml".into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Spec, run_mode_collect};
    use cm_core::text::to_u16;

    #[test]
    fn numbers() {
        let n = |s: &str| re_number(&to_u16(s)).map(|m| m.len);
        assert_eq!(n("0x1F,"), Some(4));
        assert_eq!(n("0x"), Some(1));
        assert_eq!(n("-1.5e-3x"), Some(7));
        assert_eq!(n("1.e"), Some(2));
        assert_eq!(n(".5"), Some(2));
        assert_eq!(n("+."), None);
        assert_eq!(n("-"), None);
    }

    #[test]
    fn small_scene() {
        let r = Registry::with_all_modes();
        let toks = run_mode_collect(
            &r,
            "#VRML V2.0 utf8\nDEF T Transform { children [ USE S ] }\nROUTE A.b TO C.d",
            &Spec::from("model/vrml"),
        );
        let styled: Vec<(String, String)> = toks
            .into_iter()
            .filter_map(|(t, s)| s.map(|s| (t, s)))
            .collect();
        let want = [
            ("#VRML V2.0 utf8", "meta"),
            ("DEF", "keyword"),
            ("T", "def"),
            ("Transform", "tag"),
            ("{", "bracket"),
            ("children", "property"),
            ("[", "bracket"),
            ("USE", "keyword"),
            ("S", "variable"),
            ("]", "bracket"),
            ("}", "bracket"),
            ("ROUTE", "keyword"),
            ("A", "variable"),
            ("b", "property"),
            ("TO", "keyword"),
            ("C", "variable"),
            ("d", "property"),
        ];
        assert_eq!(
            styled,
            want.iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect::<Vec<_>>()
        );
    }
}
