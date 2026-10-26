/**
 * P10-07 oracle: mode/clike/clike.js recorded from a built pinned tree. The Rust side
 * is crates/cm-modes/tests/clike_oracle.rs.
 *
 *   node oracle/cm5-clike-mode.mjs <built-cm5-dir> [out.json]
 *        default out: crates/cm-modes/tests/data/cm5-clike-mode.json
 *
 * <built-cm5-dir> is a disposable copy of oracle/cm5-build after `npm run build`
 * (addon/runmode/runmode.node.js is a build output). Never build in oracle/cm5-build.
 *
 * Sections:
 *   config   every def(mimes, mode) call: the MIME list and the parserConfig object
 *            (word tables as key lists or the predicate's name, regexps as source and
 *            flags, hooks as key -> function name, modeProps), helperType, and the
 *            registerHelper("hintWords", ...) value;
 *   props    per MIME, the mode object after getMode: keys, non-function values and
 *            start states (indentUnit 4, 2, and startState(mode, 6));
 *   resolve  resolveMode / getMode(...).name / mimeModes / meta.js findModeBy*;
 *   electric electricInput.test results for both electricInput regexps;
 *   runs     per input line: the state before, indent(state, textAfter, line), the
 *            tokens from highlight.js's readToken loop ([text, style, start]), the
 *            state after, electricInput.test(line); a "failed to advance" error ends
 *            the run. Variants: indentUnit 4 / 2, and a copyState at each line start.
 *            After each run, a restart from a copy of the state saved at each line
 *            start; `restarts` lists the lines where a restart differs from the
 *            carried run (shallow copies share contexts, so `ctx.align` set later shows
 *            through a saved copy);
 *   runmode  CM.runMode callbacks ([text, style, line, start], line break ["\n"]), for
 *            the inputs whose runs ended without an error (runMode has no 10-try guard).
 *
 * clike.js keeps Ceylon's `stringTokenizer` in its module scope. Each section loads
 * the file afresh (one load = one Rust Registry), and runs in a fixed order.
 *
 * Text that is not well-formed UTF-16 is an array of code units.
 */
import { createRequire } from "node:module";
import { readFileSync, writeFileSync } from "node:fs";
import { join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const HERE  = normalize (join (fileURLToPath (import.meta.url), ".."));
const ROOT  = normalize (join (HERE, ".."));
const BUILD = process .argv [2];
if (!BUILD) { console .error ("usage: node oracle/cm5-clike-mode.mjs <built-cm5-dir> [out.json]"); process .exit (2); }
const OUT   = process .argv [3] || join (ROOT, "crates", "cm-modes", "tests", "data", "cm5-clike-mode.json");

const require = createRequire (import.meta.url);
const CM      = require (join (BUILD, "addon", "runmode", "runmode.node.js"));
CM .Pass = { toString: function () { return "CodeMirror.Pass"; } };

const enc = s => s .isWellFormed () ? s : Array .from ({ length: s .length }, (_, i) => s .charCodeAt (i));

const CLIKE = join (BUILD, "mode", "clike", "clike.js");
let DEFS = [], HELPERS = [];
function load ()
{
   delete require .cache [require .resolve (CLIKE)];
   DEFS = []; HELPERS = [];
   const defineMIME = CM .defineMIME, registerHelper = CM .registerHelper;
   CM .defineMIME = function (mime, spec)
   {
      const d = DEFS .find (d => d .spec === spec);
      if (d) d .mimes .push (mime); else DEFS .push ({ mimes: [mime], spec });
      return defineMIME .apply (this, arguments);
   };
   CM .registerHelper = function (type, name, value) { HELPERS .push ([type, name, value]); return registerHelper .apply (this, arguments); };
   require (CLIKE);
   CM .defineMIME = defineMIME; CM .registerHelper = registerHelper;
}
load ();
require (join (BUILD, "mode", "meta.js"));

// --- config ---------------------------------------------------------------------

function configValue (v)
{
   if (v instanceof RegExp) return { regexp: v .source, flags: v .flags };
   if (typeof v == "function") return { fn: v .name };
   if (v && typeof v == "object")
   {
      if (Array .isArray (v)) return v;
      return { object: Object .fromEntries (Object .keys (v) .map (k => [k, configValue (v [k])])) };
   }
   return v;
}
function wordsValue (v) { return typeof v == "function" ? { fn: v .name } : { keys: Object .keys (v) }; }

const WORD_KEYS = ["keywords", "types", "builtin", "blockKeywords", "defKeywords", "atoms"];
const config = DEFS .map (({ mimes, spec }) =>
{
   const options = {};
   for (const k of Object .keys (spec))
   {
      if (k == "name" || k == "helperType" || k == "modeProps") continue;
      if (WORD_KEYS .includes (k)) options [k] = wordsValue (spec [k]);
      else if (k == "hooks") options [k] = Object .fromEntries (Object .keys (spec [k]) .sort () .map (h => [h, spec [k] [h] .name]));
      else options [k] = configValue (spec [k]);
   }
   const hint = HELPERS .find (([t, n]) => t == "hintWords" && n == mimes [0]);
   return { mimes, name: spec .name, options, modeProps: spec .modeProps ? configValue (spec .modeProps) : null,
            helperType: spec .helperType ?? null, hintWords: hint ? hint [2] : null };
});

// --- snapshots ------------------------------------------------------------------

function probe (t, text)
{
   const state = { tokenize: t };
   const stream = new CM .StringStream (text, 4);
   t (stream, state);
   return stream .pos;
}
function tokenizeName (t)
{
   if (t == null) return null;
   if (["tokenComment", "cppHook", "tokenRawString", "tokenAtString", "tokenTripleString"] .includes (t .name)) return [t .name];
   const src = String (t);
   if (src .includes ("multiLineStrings")) return ["tokenString", probe (t, "\"x") == 1 ? 34 : 39];
   if (src .includes ("tokenNestedComment")) return ["tokenNestedComment", probe (t, "*/" .repeat (64)) / 2];
   if (src .includes ("tripleString")) return ["tokenKotlinString", probe (t, "\"x") == 2];
   if (src .includes ("stringTokenizer")) return ["tokenCeylonString", probe (t, "\"x") == 1 ? "single" : "triple"];
   throw new Error ("unknown tokenize " + src);
}
function infoValue (i) { return i === undefined ? ["undefined"] : i === null ? null : enc (i); }
function snap (s)
{
   const context = [];
   for (let c = s .context; c; c = c .prev)
      context .push ([c .indented, c .column, c .type, infoValue (c .info), c .align]);
   return {
      tokenize: tokenizeName (s .tokenize),
      context,
      indented: s .indented,
      startOfLine: s .startOfLine,
      prevToken: s .prevToken ?? null,
      typeAtEndOfLine: s .typeAtEndOfLine ?? null,
      delim: s .cpp11RawStringDelim == null ? null : enc (s .cpp11RawStringDelim),
   };
}

// --- props ----------------------------------------------------------------------

const MIMES = DEFS .flatMap (d => d .mimes);
function propValues (m)
{
   const out = {};
   for (const k of Object .keys (m) .sort ())
      if (typeof m [k] != "function") out [k] = configValue (m [k]);
   return out;
}
const props = MIMES .map (mime =>
{
   const m = CM .getMode ({ indentUnit: 4 }, mime);
   return {
      mime,
      keys: Object .keys (m) .sort (),
      values: propValues (m),
      startState: snap (CM .startState (m)),
      startStateBase6: snap (CM .startState (m, 6)),
      startStateIndentUnit2: snap (CM .startState (CM .getMode ({ indentUnit: 2 }, mime))),
   };
});

// --- resolve --------------------------------------------------------------------

const SPECS = ["clike", ...MIMES, "text/x-cpp", "text/x-objc", "c", "C", "java", { name: "clike" }, { name: "text/x-java", extra: 1 },
               { name: "text/x-csrc", allmanIndentation: true }];
const resolveName = spec => { const r = CM .resolveMode (spec); return { name: r .name, helperType: r .helperType ?? null, keys: allKeys (r) }; };
// createObj(found, spec) inherits from the MIME spec: list inherited keys too.
function allKeys (o) { const k = []; for (const p in o) k .push (p); return k .sort (); }
const META_MIMES = ["text/x-csrc", "text/x-c", "text/x-chdr", "text/x-c++src", "text/x-c++hdr", "text/x-java", "text/x-csharp", "text/x-scala",
                    "text/x-kotlin", "text/x-objectivec", "text/x-objectivec++", "text/x-squirrel", "text/x-ceylon", "text/x-nesc", "x-shader/x-vertex"];
const resolve = {
   specs: SPECS .map (spec => ({ spec, resolved: resolveName (spec), modeName: CM .getMode ({}, spec) .name })),
   modeDefined: typeof CM .modes .clike,
   findModeByMIME: Object .fromEntries (META_MIMES .map (m => [m, CM .findModeByMIME (m) ?? null])),
   findModeByExtension: Object .fromEntries (["c", "h", "ino", "cpp", "hh", "cs", "java", "kt", "m", "mm", "scala", "nut", "ceylon", "glsl"] .map (m => [m, CM .findModeByExtension (m) ?? null])),
   findModeByName: Object .fromEntries (["C", "C++", "cpp", "C#", "csharp", "cs", "Java", "Kotlin", "Objective-C", "objc", "objc++", "Scala", "Squirrel", "Ceylon"] .map (m => [m, CM .findModeByName (m) ?? null])),
   findModeByFileName: Object .fromEntries (["main.c", "x.H", "a.cpp", "b.cs", "C.java", "d.kt", "e.m", "f.mm", "g.scala", "h.nut"] .map (m => [m, CM .findModeByFileName (m) ?? null])),
};

// --- electric -------------------------------------------------------------------

const ELECTRIC = ["}", "  }", "{", "  {", "{}", "  {}", "{}}", "case 1:", "  case x: y:", "  case :", "case", "default:", "  default:", "default :",
                  "\t}", " } ", "x}", "}}", "", "  )", "}\n", "case\t1:", "  case a:b", "caseX:", "  default:x"];
const electric = ["text/x-csrc", "text/x-scala"] .map (mime =>
{
   const m = CM .getMode ({ indentUnit: 4 }, mime);
   return { mime, cases: ELECTRIC .map (s => [s, m .electricInput .test (s)]) };
});

// --- runs -----------------------------------------------------------------------

function readToken (mode, stream, state)   // src/line/highlight.js
{
   for (let i = 0; i < 10; i++)
   {
      const style = mode .token (stream, state);
      if (stream .pos > stream .start) return style;
   }
   throw new Error ("Mode " + mode .name + " failed to advance stream.");
}

function indentValue (v)
{
   if (v === CM .Pass) return "Pass";
   if (v === undefined) return "undefined";
   return v;
}

function runLines (mode, lines, copyEachLine, from, state0, starts)
{
   let state = state0;
   const out = [];
   let error = null;
   for (let i = from; i < lines .length && !error; ++i)
   {
      const line = lines [i];
      if (copyEachLine) state = CM .copyState (mode, state);
      if (starts) starts .push (CM .copyState (mode, state));
      const before = snap (state);
      const ws     = /^\s*/ .exec (line) [0] .length;
      const indent = indentValue (mode .indent (state, line .slice (ws), line));
      const stream = new CM .StringStream (line, 4, null);
      if (line == "" && mode .blankLine) mode .blankLine (state);
      const tokens = [];
      try
      {
         while (!stream .eol ())
         {
            const style = readToken (mode, stream, state);
            tokens .push ([enc (stream .current ()), style ?? null, stream .start]);
            stream .start = stream .pos;
         }
      }
      catch (e) { error = { line: i, pos: stream .pos, message: e .message }; }
      out .push ({ before, indent, tokens, after: snap (state), electric: mode .electricInput .test (line) });
   }
   return { lines: out, error };
}

function run (text, spec, indentUnit, copyEachLine)
{
   const mode   = CM .getMode ({ indentUnit }, spec);
   const lines  = CM .splitLines (text);
   const starts = [];
   const main   = runLines (mode, lines, copyEachLine, 0, CM .startState (mode), starts);
   const restarts = [];
   for (let k = 1; k < starts .length; ++k)
   {
      const again = runLines (mode, lines, copyEachLine, k, CM .copyState (mode, starts [k]), null);
      const diff = {};
      again .lines .forEach ((l, j) => { if (JSON .stringify (l) != JSON .stringify (main .lines [k + j])) diff [k + j] = l; });
      const lenDiff = again .lines .length != main .lines .length - k;
      const errDiff = JSON .stringify (again .error) != JSON .stringify (main .error);
      if (Object .keys (diff) .length || lenDiff || errDiff)
         restarts .push ({ at: k, lines: diff, count: again .lines .length, error: again .error });
   }
   return { mode: mode .name, lines: main .lines, error: main .error, restarts };
}

const demos = [];
for (const [file, re] of [["index.html", /<textarea id="([a-z]+)-code">\n?([\s\S]*?)<\/textarea>/g], ["scala.html", /<textarea id="(code)" name="code">\n?([\s\S]*?)<\/textarea>/g]])
   for (const m of readFileSync (join (BUILD, "mode", "clike", file), "utf8") .matchAll (re))
      demos .push ([file == "scala.html" ? "scala-page" : m [1],
                    m [2] .replace (/&lt;/g, "<") .replace (/&gt;/g, ">") .replace (/&quot;/g, "\"") .replace (/&amp;/g, "&")]);
const DEMO_MIME = { c: "text/x-csrc", cpp: "text/x-c++src", objectivec: "text/x-objectivec", java: "text/x-java", scala: "text/x-scala",
                    kotlin: "text/x-kotlin", ceylon: "text/x-ceylon", "scala-page": "text/x-scala" };

const LANGS = { c: "text/x-csrc", cpp: "text/x-c++src", java: "text/x-java", csharp: "text/x-csharp", scala: "text/x-scala",
                kotlin: "text/x-kotlin", shader: "x-shader/x-vertex", nesc: "text/x-nesc", objc: "text/x-objectivec",
                objcpp: "text/x-objectivec++", squirrel: "text/x-squirrel", ceylon: "text/x-ceylon" };

const COMMON = [
   ["empty", ""],
   ["blank-lines", "\n\n\n"],
   ["whitespace-lines", "   \n\t\n  \t  \nint a() {\n    \n}\n"],
   ["function", "int main(int argc, char **argv) {\n    printf(\"%d\\n\", argc);\n    return 0;\n}\n"],
   ["statements", "x = 1 +\n    2;\ny = f(a,\n      b);\nz = g(\n  c\n);\nif (a)\n  b();\nelse\n  c();\nwhile (x)\n  for (;;)\n    y++;\ndone();"],
   ["switch", "switch (x) {\ncase 1:\n  a();\n  break;\ncase 2: {\n  b();\n}\ndefault:\n  c();\n}\n"],
   ["blocks", "struct S {\n  int a;\n  struct T {\n    int b;\n  } t;\n};\nenum E { A, B,\n  C };\nunion U { int i; float f; };"],
   ["comments", "// line\nx; // trailing\n/* block */ y; /* two\nlines */ z;\n/** doc\n * more\n */\n/*/ odd */ q;\n/* unterminated"],
   ["comment-indent", "void f() {\n    /* start\n       middle\n    }\n    end */\n    x;\n}\n"],
   ["strings", "\"abc\" 'c' \"esc\\\"aped\" 'x\\'' \"\\\\\" \"tab\\t\" \"\" '' \"open\nnext\"; \"cont\\\n  line\";"],
   ["numbers", "0 1 12 1.5 .5 1. 1e10 1E-5 1.5e+3f 0x1F 0XAB 0b101 0B11 10u 10l 10ll 10UL 1f 0x 0b 1_000 10'000 0x10'000 12abc .x 1.2.3 9e"],
   ["operators", "a+b a-b a*b a/b a%b a&&b a||b !a ~a a^b a<<b a>>=b a->b a::b a?b:c a==b a!=b a<=b a/*c*/b a//c\nx+=1; y/=2; z|=3; w@=4; v#=5; u$6; t`7"],
   ["punctuation", "a(b[c{d}e]f)g, h; i: j. k\n(\n[\n{\n}\n]\n)\n})]\n"],
   ["typedefs", "typedef unsigned long size_t;\nsize_t n; uint8_t b; foo_t *p; foo_T q; _t r; int_t s;\nstatic const char *names[] = { \"a\", \"b\" };\nvoid *ptr; int * const cp; char **pp;"],
   ["defs", "int f() {}\nint *g(void) {}\nunsigned int h(int a) {}\nstruct S s() {}\nS::S() {}\nFoo::~Foo() {}\nint\nk() {}\nnamespace n { int f(); }\nclass C { void m(); };"],
   ["keywords-c", "auto if break case register continue return default do sizeof static else struct switch extern typedef union for goto while enum const volatile inline restrict asm fortran NULL true false"],
   ["keywords-other", "class public private protected try catch finally throw new this super import package interface extends implements var val def fun object trait yield in out"],
   ["preprocessor", "#include <stdio.h>\n#define X 1\n#define Y \\\n  2\n# if X // c\n#endif /* c */\n  #pragma once\nint #x;\n#\n#define Z \\\n"],
   ["reserved", "__x _X _x __ _ _1 __FILE__ _Bool _Complex _Ünder _ünder"],
   ["at-words", "@interface @implementation @end @property @synthesize @Override @Deprecated(\"x\") @ @1 @\"str\" @\"a\"\"b\" x"],
   ["backticks", "`a` ``b`` `"],
   ["allman", "void f()\n{\n  if (x)\n  {\n    y();\n  }\n  g(a,\n    b);\n}\n"],
   ["unbalanced", "}}}\n)\n{{{\n  {\n(((\n]\nx\n"],
   ["tabs", "void f() {\n\tif (x) {\n\t\ty;\n\t}\n  \tz {\n\t  w;\n}\n}"],
   ["unicode", "int café = 1; char *s = \"日本語 😀\";\nvoid 名前() {}\nint x　= 2;\n// コメント 😀\n/* 😀 */ 'é' 'ß' \" \" 𝓍 é́ Ärger ärger Σ σ ǅ"],
   ["surrogates", "'😀' '\ud83d' \"\ud83d\" \ude00x 😀😀 a😀b // 😀\n\ud800\n\udfff\nx\ud83d(y);\n"],
   ["u2028", "x  y;\n/* a  b */ c;\n// d  e\n\"f  g\" h;"],
   ["incomplete", "\"unterminated\nnext line\" ok\n'\n/*\n*/\n/* still\nint f(\n  int a,\n"],
   ["crlf", "int a() {\r\n    1;\r\n}\r\n\"x\r\ny\"\r"],
   ["long-line", ("int a[] = {" + "1, ".repeat (200) + "};") + "\n" + "/* " + "x".repeat (500) + " */"],
   ["type-first", "Foo bar() {}\nFoo<T> baz() {}\nList<String> qux(int a) {}\nfoo.bar baz() {}\nfoo->bar baz() {}\nA* f() {}\nA] g() {}\n  Foo\nbar() {}\nint\n  h() {}\nint x\n  (y);"],
   ["namespace", "namespace a {\nint f() {}\nnamespace b {\nvoid g() {}\n}\n}\nclass C {\n  int m() {}\n};\nint top() {}"],
   ["comma-eol", "f(a,\n  b, // c\n  c,\n  d);\nint x = 1,\n    y = 2;\nint z = {1,\n  2};"],
];

const SPECIFIC = {
   c: [["c-misc", "struct point { int x, y; };\nstatic inline int max(int a, int b) { return a > b ? a : b; }\nint main(void) {\n  struct point p = {1, 2};\n  goto end;\nend:\n  return 0;\n}"]],
   cpp: [["cpp-misc", "template <typename T>\nclass Foo : public Bar<T> {\npublic:\n  Foo() : x(0) {}\n  virtual ~Foo() override;\n  std::vector<int> v;\n};\nFoo::Foo() {}\nFoo::~Foo() {}\nauto l = [](int x) -> int { return x; };\nusing namespace std;\nstd::string s = u8\"x\" + U\"y\" + L'z' + u'w';"],
         ["cpp-raw", "auto a = R\"(raw)\";\nauto b = R\"delim(x)\" )delim\";\nauto c = u8R\"(a\nb\nc)\";\nauto d = LR\"--(q)--\";\nauto e = R\"x\";\nauto f = uR\"[](no)[]\" g;\nauto h = R\"*+?(z)*+?\";\nauto i = R\"12345678901234567(x)12345678901234567\";\nR\"a b(x)a b\"\nUR\"\\(x)\\\"\nR\"(\nnever closed"],
         ["cpp-literals", "10'000 0b10'000 0x10'000 1.5e3 1'2'3 3.14 0 9 'a' 100'000'000ull 0x1p3"],
         ["cpp-template", "template <class T>\nstruct S {\n  T t;\n};\ntemplate <class T>\nvoid f(T t)\n{\n}\ntemplate<>\nint g();"],
         ["cpp-ns-sep", "std::cout << a::b::c::d; ::global(); a:: b; a::~b; x::y::z(); Foo::Foo(int); Bar::Bar::Bar();"]],
   java: [["java-misc", "package a.b;\nimport java.util.*;\n@Override\npublic class A extends B implements C {\n  private final int x = 0x1_000;\n  public A() { super(); }\n  @Deprecated public static void main(String[] args) throws Exception {\n    var s = \"\"\"\n      text block\n      \"\"\";\n    long n = 1_000_000L;\n  }\n}\n@interface Ann {}\n\"\"\"\nopen\n\"\"\" end\n\"\"\"x\"\"\""]],
   csharp: [["csharp-misc", "using System;\nnamespace N {\n  public record R(int X);\n  class C {\n    var s = @\"verbatim \"\"quoted\"\" \\n\";\n    var t = @\"multi\nline\";\n    [Obsolete] async Task<int> M() { await x; return 1; }\n    @if = 1; @class;\n  }\n}"]],
   scala: [["scala-misc", "object Main extends App {\n  val x: Int = 1\n  def f(a: Int) = a match {\n    case 1 => \"one\"\n    case _ =>\n      \"other\"\n  }\n  val s = \"\"\"multi\n  line\"\"\"\n  val c = 'a' :: '\\n' :: 'sym :: '\\u0041' :: Nil\n  @tailrec def g = ()\n  /* a /* nested */ still */ x\n  xs.map { y =>\n    y + 1\n  }\n}"],
           ["scala-arrow", "foo {\n  x =>\n    y\n}\nbar { a => b }\nbaz {\n  c\n} => d\nq = r"]],
   kotlin: [["kotlin-misc", "package p\nfun main(args: Array<String>) {\n    val s = \"x ${a + b} y $c \\\" z\"\n    val t = \"\"\"raw\n    ${x}\n    \"\"\"\n    when (x) {\n        1 -> a\n        else -> b\n    }\n    list.map { it * 2 }\n        .filter { it > 0 }\n    a.* b\n    @Suppress(\"x\") val y = 0x1_FFul + 1.5f\n    /* a /* b */ c */\n}\nval z = a +\n    b\nfoo()\n    .bar()\nclass C {\n}"],
            ["kotlin-indent", "fun f() {\n    g(1,\n      2)\n    h {\n    }\n    x\n        .y\n    a = b +\n        c\n    }\n}\nval q = \"unterminated\nval r = 1"]],
   shader: [["shader-misc", "#version 120\nuniform sampler2D tex;\nvarying vec2 uv;\nvoid main() {\n  vec4 c = texture2D(tex, uv);\n  gl_FragColor = vec4(c.rgb * 0.5, 1.0);\n  float d = dot(normalize(a), b);\n  if (d < 0.0) discard;\n}"]],
   nesc: [["nesc-misc", "module BlinkC {\n  uses interface Timer<TMilli> as Timer0;\n  uses interface Leds;\n}\nimplementation {\n  event void Boot.booted() {\n    call Timer0.startPeriodic(250);\n  }\n  task void t() { post t(); atomic { x = 1; } }\n}"]],
   objc: [["objc-misc", "#import <Foundation/Foundation.h>\n@interface Foo : NSObject <Bar>\n@property (nonatomic, strong) NSString *name;\n- (instancetype)initWithName:(NSString *)name NS_DESIGNATED_INITIALIZER;\n@end\n@implementation Foo\n- (void)dealloc {\n  @try { [self x]; } @catch (NSException *e) { } @finally { }\n  @autoreleasepool {\n    id x = @\"str\"; BOOL b = YES; SEL s = @selector(foo:);\n  }\n}\n@end"]],
   objcpp: [["objcpp-misc", "@interface A\n@end\ntemplate <class T> class B {};\nstd::vector<id> v; auto r = R\"(x)\"; A::A() {}\n@implementation A\n- (void)m { try { throw 1; } catch (...) {} }\n@end"]],
   squirrel: [["squirrel-misc", "local a = 1;\nfunction foo(x) {\n  foreach (i, v in arr) {\n    print(v);\n  }\n  return clone this;\n}\nclass Bar extends Baz {\n  constructor() { base.constructor(); }\n  static s = null;\n}\n#comment-ish\nlocal t = { k = 1, \"q\": 2 };"]],
   ceylon: [["ceylon-misc", "shared class Point(shared Float x, shared Float y) {\n  shared actual String string => \"(``x``, ``y``)\";\n  value v = \"\"\"triple ``q`` \"\"\";\n  Integer h = #FF_FF; Integer b = $1010; Float f = 1.5k; Integer m = 1M;\n  value c = 'a'; value x = `Point`;\n  void f() { print(\"a\\\"b\"); }\n  @doc x.Foo y.bar Foo.Bar\n}"],
            ["ceylon-interp", "value a = \"x``y``z\";\nvalue b = \"x``y\n``z\";\nvalue c = \"\"\"x``y``z\"\"\";\nvalue d = \"open``\";\n`` x\nvalue e = \"q\";"],
            ["ceylon-numbers", "1 1_000 1k 1.5 1.5e3 1.5E-3 1.5m #ff #FF_00 $01 $10_10 1.x"],
            ["ceylon-zero", "value q = # ;"],
            ["ceylon-dollar", "value r = $x;"]],
};

function mulberry32 (a)
{
   return function ()
   {
      a |= 0; a = a + 0x6D2B79F5 | 0;
      let t = Math .imul (a ^ a >>> 15, 1 | a);
      t = t + Math .imul (t ^ t >>> 7, 61 | t) ^ t;
      return ((t ^ t >>> 14) >>> 0) / 4294967296;
   };
}
const rnd = mulberry32 (707);
const ri  = n => Math .floor (rnd () * n);
const ALPHA = ["int", "void", "char", "x", "foo", "_X", "__y", "size_t", "struct", "class", "if", "else", "for", "switch", "case", "default", "return",
               "namespace", "template", "public", "val", "def", "fun", "object", "@interface", "@end", "@", "#", "#define", "\\", " ", " ", " ", "\t",
               "1", "0x1F", "1.5e3", "10'000", "1_000", "\"", "\"", "'", "\"\"\"", "R\"(", ")\"", "u8\"", "L'", "@\"", "``", "`", "$", "${", "}",
               "//", "/*", "*/", "{", "}", "(", ")", "[", "]", "<", ">", "::", "~", "!", "=", "=>", "->", ";", ",", ":", ".", "*", "&", "?",
               "\n", "\n", "\n", "\n", "\r\n", "é", "Ä", "😀", " ", "　", "\ud83d", "Foo", "String", "NULL", "true", "nil"];
const random = [];
for (let i = 0; i < 30 * Object .keys (LANGS) .length; ++i)
{
   let s = "";
   for (let n = ri (60); n > 0; --n) s += ALPHA [ri (ALPHA .length)];
   random .push (s);
}

const OPTION_SPECS = [
   ["allman", { name: "text/x-csrc", allmanIndentation: true }, "allman"],
   ["dont-align-calls", { name: "text/x-java", dontAlignCalls: true }, "statements"],
   ["statement-indent-8", { name: "text/x-csrc", statementIndentUnit: 8 }, "statements"],
   ["style-defs-false", { name: "text/x-c++src", styleDefs: false }, "defs"],
   ["scala-indent-statements", { name: "text/x-scala", indentStatements: true }, "statements"],
   ["c-indent-switch-false", { name: "text/x-csrc", indentSwitch: false }, "switch"],
   ["c-multi-line-strings", { name: "text/x-csrc", multiLineStrings: true }, "strings"],
   ["bare-clike", { name: "clike" }, "function"],
   ["bare-clike-type-first", { name: "clike", typeFirstDefinitions: true }, "type-first"],
];

load ();
const runs = [];
const push = (kind, name, spec, indentUnit, copy, text) =>
   runs .push ({ kind, name, spec, indentUnit, copy, text: enc (text), ... run (text, spec, indentUnit, copy) });
for (const [lang, mime] of Object .entries (LANGS))
{
   for (const [name, text] of [...COMMON, ...(SPECIFIC [lang] || [])])
      for (const [indentUnit, copy] of [[4, false], [2, false], [4, true]])
         push ("sample", lang + "/" + name, mime, indentUnit, copy, text);
}
for (const [name, text] of demos)
   push ("demo", name, DEMO_MIME [name], 2, false, text);
for (const mime of MIMES)
   push ("mime", "function", mime, 4, false, COMMON .find (([n]) => n == "function") [1]);
for (const [name, spec, sample] of OPTION_SPECS)
   push ("option", name, spec, 4, false, COMMON .find (([n]) => n == sample) [1]);
const langList = Object .values (LANGS);
random .forEach ((text, i) => push ("random", "random-" + i, langList [i % langList .length], 4, i % 4 == 0, text));

// --- runmode --------------------------------------------------------------------

load ();
function runMode (text, spec)
{
   const toks = [];
   CM .runMode (text, spec, (t, style, line, start) =>
      toks .push (t == "\n" && line === undefined ? ["\n"] : [enc (t), style == null ? null : style, line, start]));
   return toks;
}
const runmode = [];
for (const [lang, mime] of Object .entries (LANGS))
   for (const [name, text] of [...COMMON, ...(SPECIFIC [lang] || [])])
      if (!runs .some (r => JSON .stringify (r .text) == JSON .stringify (enc (text)) && r .spec == mime && r .error))
         runmode .push ({ spec: mime, name, text: enc (text), tokens: runMode (text, mime) });

const out = { cm: "5.65.21", generator: "oracle/cm5-clike-mode.mjs", config, props, resolve, electric, runs, runmode };
writeFileSync (OUT, JSON .stringify (out));
const nTok = runs .reduce ((n, r) => n + r .lines .reduce ((m, l) => m + l .tokens .length, 0), 0);
const nRestart = runs .reduce ((n, r) => n + Math .max (0, r .lines .length - 1), 0);
console .log (`${config .length} defs, ${MIMES .length} MIMEs, ${runs .length} runs (${nTok} tokens, ${runs .filter (r => r .error) .length} with errors, ` +
              `${nRestart} restarts, ${runs .reduce ((n, r) => n + r .restarts .length, 0)} differing), ${runmode .length} runMode inputs -> ${OUT}`);
