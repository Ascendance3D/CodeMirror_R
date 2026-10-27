//! Port of `mode/meta.js` (CodeMirror 5.65.21): `CodeMirror.modeInfo` and the
//! `findModeBy*` lookups.
//!
//! `modeInfo` is metadata only: it names a mode and the MIME types, extensions,
//! aliases and file-name patterns that select it. An entry does not mean the mode
//! is implemented. Use [`ModeInfo::is_available`] (is the mode registered?) before
//! treating an entry as usable; an entry whose mode is not registered resolves to
//! the `null` mode through [`Registry::get_mode`].
//!
//! The JS list is a mutable global; here it is a value ([`ModeInfoList`]) that
//! starts as the upstream list ([`ModeInfoList::upstream`]) and can be extended.
//! Entries keep the JS shape after meta.js's own fix-up: an entry with `mimes` has
//! `mime == mimes[0]`.

use std::rc::Rc;

use crate::js::to_lower;
use crate::{RegExp, Registry};

/// One `modeInfo` entry.
#[derive(Clone, Debug)]
pub struct ModeInfo {
    pub name: String,
    /// `mime` (for entries with `mimes`, the first of them).
    pub mime: Option<String>,
    /// `mimes`; empty = absent.
    pub mimes: Vec<String>,
    /// The `defineMode` name.
    pub mode: String,
    /// `ext`; empty = absent.
    pub ext: Vec<String>,
    /// `alias`; empty = absent.
    pub alias: Vec<String>,
    /// `file`: a file-name regexp.
    pub file: Option<Rc<RegExp>>,
}

impl ModeInfo {
    /// Whether `registry` defines this entry's mode. Metadata alone is not an
    /// implementation.
    pub fn is_available(&self, registry: &Registry) -> bool {
        registry.has_mode(&self.mode)
    }
}

/// `CodeMirror.modeInfo`.
#[derive(Clone, Debug, Default)]
pub struct ModeInfoList {
    pub infos: Vec<ModeInfo>,
}

fn js_lower(s: &str) -> String {
    String::from_utf16_lossy(&to_lower(&s.encode_utf16().collect::<Vec<_>>()))
}

type Raw = (
    &'static str,
    Option<&'static str>,
    &'static [&'static str],
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    Option<(&'static str, &'static str)>,
);

const fn raw(
    name: &'static str,
    mime: Option<&'static str>,
    mimes: &'static [&'static str],
    mode: &'static str,
    ext: &'static [&'static str],
    alias: &'static [&'static str],
    file: Option<(&'static str, &'static str)>,
) -> Raw {
    (name, mime, mimes, mode, ext, alias, file)
}

/// The upstream list, in meta.js order (generated from the pinned source).
#[rustfmt::skip]
const UPSTREAM: &[Raw] = &[
    raw("APL", Some("text/apl"), &[], "apl", &["dyalog", "apl"], &[], None),
    raw("PGP", None, &["application/pgp", "application/pgp-encrypted", "application/pgp-keys", "application/pgp-signature"], "asciiarmor", &["asc", "pgp", "sig"], &[], None),
    raw("ASN.1", Some("text/x-ttcn-asn"), &[], "asn.1", &["asn", "asn1"], &[], None),
    raw("Asterisk", Some("text/x-asterisk"), &[], "asterisk", &[], &[], Some(("^extensions\\.conf$", "i"))),
    raw("Brainfuck", Some("text/x-brainfuck"), &[], "brainfuck", &["b", "bf"], &[], None),
    raw("C", Some("text/x-csrc"), &[], "clike", &["c", "h", "ino"], &[], None),
    raw("C++", Some("text/x-c++src"), &[], "clike", &["cpp", "c++", "cc", "cxx", "hpp", "h++", "hh", "hxx"], &["cpp"], None),
    raw("Cobol", Some("text/x-cobol"), &[], "cobol", &["cob", "cpy", "cbl"], &[], None),
    raw("C#", Some("text/x-csharp"), &[], "clike", &["cs"], &["csharp", "cs"], None),
    raw("Clojure", Some("text/x-clojure"), &[], "clojure", &["clj", "cljc", "cljx"], &[], None),
    raw("ClojureScript", Some("text/x-clojurescript"), &[], "clojure", &["cljs"], &[], None),
    raw("Closure Stylesheets (GSS)", Some("text/x-gss"), &[], "css", &["gss"], &[], None),
    raw("CMake", Some("text/x-cmake"), &[], "cmake", &["cmake", "cmake.in"], &[], Some(("^CMakeLists\\.txt$", ""))),
    raw("CoffeeScript", None, &["application/vnd.coffeescript", "text/coffeescript", "text/x-coffeescript"], "coffeescript", &["coffee"], &["coffee", "coffee-script"], None),
    raw("Common Lisp", Some("text/x-common-lisp"), &[], "commonlisp", &["cl", "lisp", "el"], &["lisp"], None),
    raw("Cypher", Some("application/x-cypher-query"), &[], "cypher", &["cyp", "cypher"], &[], None),
    raw("Cython", Some("text/x-cython"), &[], "python", &["pyx", "pxd", "pxi"], &[], None),
    raw("Crystal", Some("text/x-crystal"), &[], "crystal", &["cr"], &[], None),
    raw("CSS", Some("text/css"), &[], "css", &["css"], &[], None),
    raw("CQL", Some("text/x-cassandra"), &[], "sql", &["cql"], &[], None),
    raw("D", Some("text/x-d"), &[], "d", &["d"], &[], None),
    raw("Dart", None, &["application/dart", "text/x-dart"], "dart", &["dart"], &[], None),
    raw("diff", Some("text/x-diff"), &[], "diff", &["diff", "patch"], &[], None),
    raw("Django", Some("text/x-django"), &[], "django", &[], &[], None),
    raw("Dockerfile", Some("text/x-dockerfile"), &[], "dockerfile", &[], &[], Some(("^Dockerfile$", ""))),
    raw("DTD", Some("application/xml-dtd"), &[], "dtd", &["dtd"], &[], None),
    raw("Dylan", Some("text/x-dylan"), &[], "dylan", &["dylan", "dyl", "intr"], &[], None),
    raw("EBNF", Some("text/x-ebnf"), &[], "ebnf", &[], &[], None),
    raw("ECL", Some("text/x-ecl"), &[], "ecl", &["ecl"], &[], None),
    raw("edn", Some("application/edn"), &[], "clojure", &["edn"], &[], None),
    raw("Eiffel", Some("text/x-eiffel"), &[], "eiffel", &["e"], &[], None),
    raw("Elm", Some("text/x-elm"), &[], "elm", &["elm"], &[], None),
    raw("Embedded JavaScript", Some("application/x-ejs"), &[], "htmlembedded", &["ejs"], &[], None),
    raw("Embedded Ruby", Some("application/x-erb"), &[], "htmlembedded", &["erb"], &[], None),
    raw("Erlang", Some("text/x-erlang"), &[], "erlang", &["erl"], &[], None),
    raw("Esper", Some("text/x-esper"), &[], "sql", &[], &[], None),
    raw("Factor", Some("text/x-factor"), &[], "factor", &["factor"], &[], None),
    raw("FCL", Some("text/x-fcl"), &[], "fcl", &[], &[], None),
    raw("Forth", Some("text/x-forth"), &[], "forth", &["forth", "fth", "4th"], &[], None),
    raw("Fortran", Some("text/x-fortran"), &[], "fortran", &["f", "for", "f77", "f90", "f95"], &[], None),
    raw("F#", Some("text/x-fsharp"), &[], "mllike", &["fs"], &["fsharp"], None),
    raw("Gas", Some("text/x-gas"), &[], "gas", &["s"], &[], None),
    raw("Gherkin", Some("text/x-feature"), &[], "gherkin", &["feature"], &[], None),
    raw("GitHub Flavored Markdown", Some("text/x-gfm"), &[], "gfm", &[], &[], Some(("^(readme|contributing|history)\\.md$", "i"))),
    raw("Go", Some("text/x-go"), &[], "go", &["go"], &[], None),
    raw("Groovy", Some("text/x-groovy"), &[], "groovy", &["groovy", "gradle"], &[], Some(("^Jenkinsfile$", ""))),
    raw("HAML", Some("text/x-haml"), &[], "haml", &["haml"], &[], None),
    raw("Haskell", Some("text/x-haskell"), &[], "haskell", &["hs"], &[], None),
    raw("Haskell (Literate)", Some("text/x-literate-haskell"), &[], "haskell-literate", &["lhs"], &[], None),
    raw("Haxe", Some("text/x-haxe"), &[], "haxe", &["hx"], &[], None),
    raw("HXML", Some("text/x-hxml"), &[], "haxe", &["hxml"], &[], None),
    raw("ASP.NET", Some("application/x-aspx"), &[], "htmlembedded", &["aspx"], &["asp", "aspx"], None),
    raw("HTML", Some("text/html"), &[], "htmlmixed", &["html", "htm", "handlebars", "hbs"], &["xhtml"], None),
    raw("HTTP", Some("message/http"), &[], "http", &[], &[], None),
    raw("IDL", Some("text/x-idl"), &[], "idl", &["pro"], &[], None),
    raw("Pug", Some("text/x-pug"), &[], "pug", &["jade", "pug"], &["jade"], None),
    raw("Java", Some("text/x-java"), &[], "clike", &["java"], &[], None),
    raw("Java Server Pages", Some("application/x-jsp"), &[], "htmlembedded", &["jsp"], &["jsp"], None),
    raw("JavaScript", None, &["text/javascript", "text/ecmascript", "application/javascript", "application/x-javascript", "application/ecmascript"], "javascript", &["js"], &["ecmascript", "js", "node"], None),
    raw("JSON", None, &["application/json", "application/x-json"], "javascript", &["json", "map"], &["json5"], None),
    raw("JSON-LD", Some("application/ld+json"), &[], "javascript", &["jsonld"], &["jsonld"], None),
    raw("JSX", Some("text/jsx"), &[], "jsx", &["jsx"], &[], None),
    raw("Jinja2", Some("text/jinja2"), &[], "jinja2", &["j2", "jinja", "jinja2"], &[], None),
    raw("Julia", Some("text/x-julia"), &[], "julia", &["jl"], &["jl"], None),
    raw("Kotlin", Some("text/x-kotlin"), &[], "clike", &["kt"], &[], None),
    raw("LESS", Some("text/x-less"), &[], "css", &["less"], &[], None),
    raw("LiveScript", Some("text/x-livescript"), &[], "livescript", &["ls"], &["ls"], None),
    raw("Lua", Some("text/x-lua"), &[], "lua", &["lua"], &[], None),
    raw("Markdown", Some("text/x-markdown"), &[], "markdown", &["markdown", "md", "mkd"], &[], None),
    raw("mIRC", Some("text/mirc"), &[], "mirc", &[], &[], None),
    raw("MariaDB SQL", Some("text/x-mariadb"), &[], "sql", &[], &[], None),
    raw("Mathematica", Some("text/x-mathematica"), &[], "mathematica", &["m", "nb", "wl", "wls"], &[], None),
    raw("Modelica", Some("text/x-modelica"), &[], "modelica", &["mo"], &[], None),
    raw("MUMPS", Some("text/x-mumps"), &[], "mumps", &["mps"], &[], None),
    raw("MS SQL", Some("text/x-mssql"), &[], "sql", &[], &[], None),
    raw("mbox", Some("application/mbox"), &[], "mbox", &["mbox"], &[], None),
    raw("MySQL", Some("text/x-mysql"), &[], "sql", &[], &[], None),
    raw("Nginx", Some("text/x-nginx-conf"), &[], "nginx", &[], &[], Some(("nginx.*\\.conf$", "i"))),
    raw("NSIS", Some("text/x-nsis"), &[], "nsis", &["nsh", "nsi"], &[], None),
    raw("NTriples", None, &["application/n-triples", "application/n-quads", "text/n-triples"], "ntriples", &["nt", "nq"], &[], None),
    raw("Objective-C", Some("text/x-objectivec"), &[], "clike", &["m"], &["objective-c", "objc"], None),
    raw("Objective-C++", Some("text/x-objectivec++"), &[], "clike", &["mm"], &["objective-c++", "objc++"], None),
    raw("OCaml", Some("text/x-ocaml"), &[], "mllike", &["ml", "mli", "mll", "mly"], &[], None),
    raw("Octave", Some("text/x-octave"), &[], "octave", &["m"], &[], None),
    raw("Oz", Some("text/x-oz"), &[], "oz", &["oz"], &[], None),
    raw("Pascal", Some("text/x-pascal"), &[], "pascal", &["p", "pas"], &[], None),
    raw("PEG.js", Some("null"), &[], "pegjs", &["jsonld"], &[], None),
    raw("Perl", Some("text/x-perl"), &[], "perl", &["pl", "pm"], &[], None),
    raw("PHP", None, &["text/x-php", "application/x-httpd-php", "application/x-httpd-php-open"], "php", &["php", "php3", "php4", "php5", "php7", "phtml"], &[], None),
    raw("Pig", Some("text/x-pig"), &[], "pig", &["pig"], &[], None),
    raw("Plain Text", Some("text/plain"), &[], "null", &["txt", "text", "conf", "def", "list", "log"], &[], None),
    raw("PLSQL", Some("text/x-plsql"), &[], "sql", &["pls"], &[], None),
    raw("PostgreSQL", Some("text/x-pgsql"), &[], "sql", &[], &[], None),
    raw("PowerShell", Some("application/x-powershell"), &[], "powershell", &["ps1", "psd1", "psm1"], &[], None),
    raw("Properties files", Some("text/x-properties"), &[], "properties", &["properties", "ini", "in"], &["ini", "properties"], None),
    raw("ProtoBuf", Some("text/x-protobuf"), &[], "protobuf", &["proto"], &[], None),
    raw("Python", Some("text/x-python"), &[], "python", &["BUILD", "bzl", "py", "pyw"], &[], Some(("^(BUCK|BUILD)$", ""))),
    raw("Puppet", Some("text/x-puppet"), &[], "puppet", &["pp"], &[], None),
    raw("Q", Some("text/x-q"), &[], "q", &["q"], &[], None),
    raw("R", Some("text/x-rsrc"), &[], "r", &["r", "R"], &["rscript"], None),
    raw("reStructuredText", Some("text/x-rst"), &[], "rst", &["rst"], &["rst"], None),
    raw("RPM Changes", Some("text/x-rpm-changes"), &[], "rpm", &[], &[], None),
    raw("RPM Spec", Some("text/x-rpm-spec"), &[], "rpm", &["spec"], &[], None),
    raw("Ruby", Some("text/x-ruby"), &[], "ruby", &["rb"], &["jruby", "macruby", "rake", "rb", "rbx"], None),
    raw("Rust", Some("text/x-rustsrc"), &[], "rust", &["rs"], &[], None),
    raw("SAS", Some("text/x-sas"), &[], "sas", &["sas"], &[], None),
    raw("Sass", Some("text/x-sass"), &[], "sass", &["sass"], &[], None),
    raw("Scala", Some("text/x-scala"), &[], "clike", &["scala"], &[], None),
    raw("Scheme", Some("text/x-scheme"), &[], "scheme", &["scm", "ss"], &[], None),
    raw("SCSS", Some("text/x-scss"), &[], "css", &["scss"], &[], None),
    raw("Shell", None, &["text/x-sh", "application/x-sh"], "shell", &["sh", "ksh", "bash"], &["bash", "sh", "zsh"], Some(("^PKGBUILD$", ""))),
    raw("Sieve", Some("application/sieve"), &[], "sieve", &["siv", "sieve"], &[], None),
    raw("Slim", None, &["text/x-slim", "application/x-slim"], "slim", &["slim"], &[], None),
    raw("Smalltalk", Some("text/x-stsrc"), &[], "smalltalk", &["st"], &[], None),
    raw("Smarty", Some("text/x-smarty"), &[], "smarty", &["tpl"], &[], None),
    raw("Solr", Some("text/x-solr"), &[], "solr", &[], &[], None),
    raw("SML", Some("text/x-sml"), &[], "mllike", &["sml", "sig", "fun", "smackspec"], &[], None),
    raw("Soy", Some("text/x-soy"), &[], "soy", &["soy"], &["closure template"], None),
    raw("SPARQL", Some("application/sparql-query"), &[], "sparql", &["rq", "sparql"], &["sparul"], None),
    raw("Spreadsheet", Some("text/x-spreadsheet"), &[], "spreadsheet", &[], &["excel", "formula"], None),
    raw("SQL", Some("text/x-sql"), &[], "sql", &["sql"], &[], None),
    raw("SQLite", Some("text/x-sqlite"), &[], "sql", &[], &[], None),
    raw("Squirrel", Some("text/x-squirrel"), &[], "clike", &["nut"], &[], None),
    raw("Stylus", Some("text/x-styl"), &[], "stylus", &["styl"], &[], None),
    raw("Swift", Some("text/x-swift"), &[], "swift", &["swift"], &[], None),
    raw("sTeX", Some("text/x-stex"), &[], "stex", &[], &[], None),
    raw("LaTeX", Some("text/x-latex"), &[], "stex", &["text", "ltx", "tex"], &["tex"], None),
    raw("SystemVerilog", Some("text/x-systemverilog"), &[], "verilog", &["v", "sv", "svh"], &[], None),
    raw("Tcl", Some("text/x-tcl"), &[], "tcl", &["tcl"], &[], None),
    raw("Textile", Some("text/x-textile"), &[], "textile", &["textile"], &[], None),
    raw("TiddlyWiki", Some("text/x-tiddlywiki"), &[], "tiddlywiki", &[], &[], None),
    raw("Tiki wiki", Some("text/tiki"), &[], "tiki", &[], &[], None),
    raw("TOML", Some("text/x-toml"), &[], "toml", &["toml"], &[], None),
    raw("Tornado", Some("text/x-tornado"), &[], "tornado", &[], &[], None),
    raw("troff", Some("text/troff"), &[], "troff", &["1", "2", "3", "4", "5", "6", "7", "8", "9"], &[], None),
    raw("TTCN", Some("text/x-ttcn"), &[], "ttcn", &["ttcn", "ttcn3", "ttcnpp"], &[], None),
    raw("TTCN_CFG", Some("text/x-ttcn-cfg"), &[], "ttcn-cfg", &["cfg"], &[], None),
    raw("Turtle", Some("text/turtle"), &[], "turtle", &["ttl"], &[], None),
    raw("TypeScript", Some("application/typescript"), &[], "javascript", &["ts"], &["ts"], None),
    raw("TypeScript-JSX", Some("text/typescript-jsx"), &[], "jsx", &["tsx"], &["tsx"], None),
    raw("Twig", Some("text/x-twig"), &[], "twig", &[], &[], None),
    raw("Web IDL", Some("text/x-webidl"), &[], "webidl", &["webidl"], &[], None),
    raw("VB.NET", Some("text/x-vb"), &[], "vb", &["vb"], &[], None),
    raw("VBScript", Some("text/vbscript"), &[], "vbscript", &["vbs"], &[], None),
    raw("Velocity", Some("text/velocity"), &[], "velocity", &["vtl"], &[], None),
    raw("Verilog", Some("text/x-verilog"), &[], "verilog", &["v"], &[], None),
    raw("VHDL", Some("text/x-vhdl"), &[], "vhdl", &["vhd", "vhdl"], &[], None),
    raw("Vue.js Component", None, &["script/x-vue", "text/x-vue"], "vue", &["vue"], &[], None),
    raw("XML", None, &["application/xml", "text/xml"], "xml", &["xml", "xsl", "xsd", "svg"], &["rss", "wsdl", "xsd"], None),
    raw("XQuery", Some("application/xquery"), &[], "xquery", &["xy", "xquery"], &[], None),
    raw("Yacas", Some("text/x-yacas"), &[], "yacas", &["ys"], &[], None),
    raw("YAML", None, &["text/x-yaml", "text/yaml"], "yaml", &["yaml", "yml"], &["yml"], None),
    raw("Z80", Some("text/x-z80"), &[], "z80", &["z80"], &[], None),
    raw("mscgen", Some("text/x-mscgen"), &[], "mscgen", &["mscgen", "mscin", "msc"], &[], None),
    raw("xu", Some("text/x-xu"), &[], "mscgen", &["xu"], &[], None),
    raw("msgenny", Some("text/x-msgenny"), &[], "mscgen", &["msgenny"], &[], None),
    raw("WebAssembly", Some("text/webassembly"), &[], "wast", &["wat", "wast"], &[], None),
];

impl ModeInfoList {
    /// The 157 entries of the pinned `mode/meta.js`.
    pub fn upstream() -> ModeInfoList {
        let strings = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        ModeInfoList {
            infos: UPSTREAM
                .iter()
                .map(|&(name, mime, mimes, mode, ext, alias, file)| ModeInfo {
                    name: name.to_string(),
                    // `if (info.mimes) info.mime = info.mimes[0]`
                    mime: mime.or(mimes.first().copied()).map(str::to_string),
                    mimes: strings(mimes),
                    mode: mode.to_string(),
                    ext: strings(ext),
                    alias: strings(alias),
                    file: file.map(|(src, flags)| {
                        Rc::new(RegExp::new(src, flags).expect("meta.js file regexp"))
                    }),
                })
                .collect(),
        }
    }

    /// `CodeMirror.findModeByMIME(mime)`.
    pub fn find_mode_by_mime(&self, mime: &str) -> Option<&ModeInfo> {
        let mime = js_lower(mime);
        for info in &self.infos {
            if info.mime.as_deref() == Some(mime.as_str()) {
                return Some(info);
            }
            if info.mimes.contains(&mime) {
                return Some(info);
            }
        }
        if mime.ends_with("+xml") {
            return self.find_mode_by_mime("application/xml");
        }
        if mime.ends_with("+json") {
            return self.find_mode_by_mime("application/json");
        }
        None
    }

    /// `CodeMirror.findModeByExtension(ext)`. Only `ext` is lowercased: an
    /// upper-case extension in the list (`BUILD`) never matches.
    pub fn find_mode_by_extension(&self, ext: &str) -> Option<&ModeInfo> {
        let ext = js_lower(ext);
        self.infos.iter().find(|info| info.ext.contains(&ext))
    }

    /// `CodeMirror.findModeByFileName(filename)`: a `file` pattern first, then the
    /// text after the last `.` as an extension.
    pub fn find_mode_by_file_name(&self, filename: &str) -> Option<&ModeInfo> {
        let units: Vec<u16> = filename.encode_utf16().collect();
        for info in &self.infos {
            if info.file.as_ref().is_some_and(|f| f.test(&units)) {
                return Some(info);
            }
        }
        let dot = units.iter().rposition(|&c| c == '.' as u16)?;
        let ext = &units[dot + 1..];
        if ext.is_empty() {
            return None;
        }
        self.find_mode_by_extension(&String::from_utf16_lossy(ext))
    }

    /// `CodeMirror.findModeByName(name)`: the name or an alias, case-insensitively.
    pub fn find_mode_by_name(&self, name: &str) -> Option<&ModeInfo> {
        let name = js_lower(name);
        self.infos.iter().find(|info| {
            js_lower(&info.name) == name || info.alias.iter().any(|a| js_lower(a) == name)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_list() {
        let l = ModeInfoList::upstream();
        assert_eq!(l.infos.len(), 157);
        let pgp = l.find_mode_by_name("pgp").unwrap();
        assert_eq!(pgp.mime.as_deref(), Some("application/pgp"));
        assert_eq!(l.find_mode_by_mime("TEXT/X-C++SRC").unwrap().name, "C++");
        assert_eq!(l.find_mode_by_mime("image/svg+xml").unwrap().name, "XML");
        assert_eq!(
            l.find_mode_by_mime("application/foo+json").unwrap().name,
            "JSON"
        );
        assert!(l.find_mode_by_mime("text/nope").is_none());
        assert_eq!(l.find_mode_by_extension("RS").unwrap().mode, "rust");
        assert!(l.find_mode_by_extension("build").is_none());
        assert_eq!(
            l.find_mode_by_file_name("CMakeLists.txt").unwrap().name,
            "CMake"
        );
        assert_eq!(
            l.find_mode_by_file_name("README.md").unwrap().name,
            "GitHub Flavored Markdown"
        );
        assert_eq!(
            l.find_mode_by_file_name("my-nginx-site.conf").unwrap().name,
            "Nginx"
        );
        assert_eq!(l.find_mode_by_file_name("a.b.py").unwrap().name, "Python");
        assert!(l.find_mode_by_file_name("noext").is_none());
        assert!(l.find_mode_by_file_name("trailing.").is_none());
        assert_eq!(l.find_mode_by_name("C++").unwrap().mode, "clike");
        assert_eq!(l.find_mode_by_name("cpp").unwrap().name, "C++");
    }

    #[test]
    fn metadata_is_not_an_implementation() {
        let r = Registry::with_all_modes();
        let l = ModeInfoList::upstream();
        let python = l.find_mode_by_name("Python").unwrap();
        assert!(!python.is_available(&r));
        assert!(l.find_mode_by_name("Rust").unwrap().is_available(&r));
        assert!(l.find_mode_by_name("JavaScript").unwrap().is_available(&r));
    }
}
