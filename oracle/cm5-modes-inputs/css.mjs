/**
 * C4 inputs for mode/css/css.js (css, text/x-scss, text/x-less, text/x-gss, inline).
 * Used by `node oracle/cm5-modes-tokens.mjs css`.
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = normalize (join (fileURLToPath (import.meta.url), "..", "..", ".."));
const CM5  = join (ROOT, "upstream", "codemirror5");

function walk (dir, ext, out = [])
{
   for (const name of readdirSync (dir) .sort ())
   {
      const p = join (dir, name);
      if (name == "node_modules" || name == ".git") continue;
      if (statSync (p) .isDirectory ()) walk (p, ext, out);
      else if (name .endsWith (ext)) out .push (p);
   }
   return out;
}

// The demo documents of the scss/less/gss/css pages, as samples (textarea bodies).
function demo (file)
{
   const html = readFileSync (join (CM5, "mode", "css", file), "utf8");
   const m = /<textarea[^>]*>([\s\S]*?)<\/textarea>/ .exec (html);
   return m [1] .replace (/&lt;/g, "<") .replace (/&gt;/g, ">") .replace (/&amp;/g, "&");
}

const CSS = [
   "",
   "\n\n",
   "body { color: red; background: #fff url(\"x.png\") no-repeat; }",
   "a:hover, a:focus > b + c ~ d { text-decoration: underline !important; }",
   "@media screen and (max-width: 600px) and (orientation: landscape) {\n  .x { display: none }\n}\n",
   "@media print, foobarhello { }\n@media (min-resolution: 2dppx) { a { b: c } }\n",
   "@supports (display: grid) and (not (display: inline-grid)) {\n  div { display: grid; }\n}",
   "@font-face {\n  font-family: \"Foo\";\n  src: url(foo.woff2) format(\"woff2\");\n  foo-bar: 1;\n}\n",
   "@counter-style thumbs {\n  system: cyclic;\n  symbols: \"\\1F44D\";\n  suffix: \" \";\n  bogus: 1;\n}",
   "@keyframes spin {\n  from { transform: rotate(0deg) }\n  50% { opacity: .5 }\n  to { transform: rotate(360deg) }\n}",
   "@-webkit-keyframes x { 0% { top: 0 } }\n@-moz-document url-prefix() { .x { y: z } }",
   "@document url(http://a.b), url-prefix(\"http://c\"), domain(d.e), regexp(\"f.*\") { a { b: c } }",
   "@import url(\"foo.css\") screen;\n@import 'bar.css';\n@charset \"utf-8\";\n@namespace svg url(http://www.w3.org/2000/svg);",
   "@page :first { margin: 1in; }\n@viewport { width: device-width }",
   "a { background: url(  'x'  ) url( \"y\" ) url(z w) url() url(a\\)b) }",
   "a { color: #abc; color: #abcd; color: #aabbcc; color: #aabbccdd; color: #ab; color: #abcde; color: #ggg; }",
   "a { width: 10px; height: 1.5em; margin: -2px .5% -.25rem 0; z-index: -1; line-height: 1e3; x: 10px10 }",
   "a { -webkit-transition: all 1s; -moz-box-sizing: border-box; --main-color: #06c; color: var(--main-color); }",
   ":root { --a: 1; --b-c : 2; }\na { --x:{ y: z }; }",
   "input[type=\"text\"], a[href^='http'], a[href$=\".pdf\"], a[title~=x], a[lang|=en], a[x*=y] { }",
   "li:nth-child(2n+1)::before { content: \"\\201C\"; }\np::first-line:not(.x):hover { }",
   "a { b: c } /* comment */ d { e: f }\n/* multi\n   line\n   comment */\n/* unterminated\ncomment continues",
   "a { content: \"unterminated\n  b: c; }\nd { content: 'esc\\'aped\\\\' }\ne { content: \"line\\\ncontinued\" }",
   "a { b: c; ; ; }\n}\n}}\n{{\n{ x }",
   "a { - : x; -x: y; -1: z; -.5: w; --: v; -% }\nb - c { -}\n-\n- -",
   "a { b: c -% d }\nfoo -= 1\nfoo { x -& }",
   "foo { bar - }",
   "h1 b { color - x }",
   "x { y: calc(100% - (2 * 3px)); z: rgba(0, 0, 0, .5) hsl(120deg 100% 50% / 50%); }",
   "a { font: italic bold 12px/30px Georgia, serif; font-family: \"Helvetica Neue\", sans-serif }",
   "a { foo: bar; colour: red; scrollbar-base-color: red; zoom: 1; }",
   "a{b:c}d{e:f}g:h{i:j}",
   ".a.b#c .d-e ._f .-g .1h { }\n#id.class { }\n*{}\n#",
   "a { b: c !  important; d: e !ie; f: g!}",
   "a { color: \u00e9t\u00e9; } \u00e9 { x: y } \ud83d\ude00 { }\n.\u4e2d\u6587 { }",
   "lone \ud83d { x: \ude00 }",
   "a\t{\tb\t:\tc\t;\t}\n\t\td { }\n  \t e { f: g }",
   "a { b: c\r\nd: e }\r\n@media x {\r\n}\r\n",
   "a { b: c ~= d |= e = f ~ g | h }",
   "@media screen { a { b: c } @media print { d { e: f } } }\n@media ( { }\n@media (x) ) { }",
   "@unknown foo bar; a { }\n@foo { b { c: d } }\n@x #y z;",
   "@font-face-bogus { x: y }\n@FONT-FACE { font-family: x }\n@Counter-Style y { system: z }",
   "a { b: url(x\n) }\nc { d: url(\"y\n\") }",
   "a { b: c; }\n  e {\n    f: g;\n    h: i\n  }\n}\n  @media x {\n    y { }\n  }\n",
   "  a {\n  b: (\n  1\n  )\n  }\n",
   "a {\n  b: c(\n    d\n  );\n}\n@media (\n  x\n) {\n}\n@foo\n{\n}",
   "@media x and (y: z) {\n  a\n  {\n  }\n}\n@supports (\n",
   "@font-face { src: x }\n@font-face\n{\n  font-weight: 700\n}",
   "a { b: c }\n/*\n * x\n */\nd { e: f }",
   "a { x: -webkit-linear-gradient(top, #fff 0%, #000 100%); y: url-prefix(x) domain(y) regexp(z) }",
   "a { x: URL(X) Url( 'y' ) }\n@document URL(x) { }",
   "a b c { }\na > b { }\nhtml, body { }\n[x] { }\n:hover { }\n::selection { }",
   "a { b: c }\n:not(x) { }\n:x(y) { }\n:: { }",
   "@media only screen and (not x) or y { }\n@media aural, braille, tty and (color) { }",
   "a { display: -webkit-box; display: inline-flex; color: CornflowerBlue; color: TRANSPARENT; }",
];

const SCSS = [
   demo ("scss.html"),
   "$var: 1px;\n$other : $var * 2;\na { width: $var; #{$prop}-top: 1px; }",
   "@mixin foo($a, $b: 10px) { width: $a; }\n.x { @include foo(1px); &:hover { color: red; } }",
   ".a { .b { c: d; } &-e { f: g } > h { i: j } }\n// line comment\n/* block */ x { y: z } // trailing",
   "@each $name, $glyph in $icons {\n  .icon-#{$name}:before { content: $glyph; }\n}",
   "@function double($n) { @return $n * 2; }\n@if $a == b { c: d } @else { e: f }\n@for $i from 1 through 3 { .x-#{$i} { w: 2em * $i; } }",
   "a { b: { c: d; e: f; } }\nfoo:bar { }\nfoo: bar { }\nfoo :{ x }",
   "a { b: c / d; e: 10px/2; }\n#{x} { }\n# { }\n#{ unterminated",
   "%placeholder { x: y }\n.a { @extend %placeholder; }\n@use 'sass:math';\n@debug x;",
   "a { b: url(x/y.png); c: url(#{$x}); }\n@media #{$q} and (min-width: $w) { a { b: c } }",
   "$map: (key1: value1, key2: value2);\na { b: map-get($map, key1); }\n$-x: 1; $ x",
];

const LESS = [
   demo ("less.html"),
   "@var: 1px;\n@other : @var * 2;\na { width: @var; @{prop}-top: 1px; }",
   ".mixin(@a; @b: 2) { width: @a }\n.x { .mixin(1px; 2px); &:hover { color: red; } &-y { } }",
   "@media screen { @media (min-width: 768px) { a { b: c } } }\n@import (reference) \"foo\";\n@charset 'x';",
   "@plugin \"x\"; @-webkit-keyframes a { } @keyframes b { } @keyframesx { } @mediax: 1;",
   ".guard when (@mode = huge) { width: 100% }\n.a when (iscolor(@c)) { color: @c }",
   "// comment\n/* block\n comment */ a { b: ~\"escape\"; c: e(\"x\"); d: @@name; }",
   "@detached: { background: red; };\n.call { @detached(); }\n@r: { a: b }",
   "a { .b; #c > .d(); } @{sel} { x: y } @ { } @1 { }",
   "@page { } @font-face { font-family: x } @supports (x: y) { } @document url(x) { } @namespace x;",
];

const GSS = [
   demo ("gss.html"),
   "@component {\n  @def FOO 10px;\n  .x { width: FOO; }\n}\n@component x { a { b: c } }\n@COMPONENT { }",
   "@def BG_COLOR #fff;\n@defmixin size(W, H) { width: W; height: H; }\n.x { @mixin size(1px, 2px); }",
   "@if (A) { a { b: c } } @elseif (B) { } @else { }\n@provide 'x';\n@require 'y';",
   "@component { } }\n@component {\n  a {\n  }\n}\n@not-a-component@component { }",
];

const INLINE = [
   "color: red; background: blue",
   "width: 10px;\n  height: 20px;\nfoo: bar",
   "a { b: c } color: red",
   "#fff .x color: red; font-family: \"x\"; }",
];

const samples = [];
for (const text of CSS) samples .push ({ spec: "css", text });
for (const text of CSS .slice (0, 30)) samples .push ({ spec: "text/css", text });
for (const text of SCSS) samples .push ({ spec: "text/x-scss", text });
for (const text of LESS) samples .push ({ spec: "text/x-less", text });
for (const text of GSS) samples .push ({ spec: "text/x-gss", text });
for (const text of INLINE) samples .push ({ spec: { name: "css", inline: true }, text });
// Cross-dialect: every hand-written document through every dialect.
for (const text of [... SCSS .slice (1), ... LESS .slice (1), ... GSS .slice (1)])
   for (const spec of ["css", "text/x-scss", "text/x-less", "text/x-gss"])
      samples .push ({ spec, text });
for (const text of CSS)
   for (const spec of ["text/x-scss", "text/x-less", "text/x-gss", { name: "css", inline: true }])
      samples .push ({ spec, text });
// A spec that overrides MIME options (createObj(found, spec)).
samples .push ({ spec: { name: "text/x-scss", allowNested: false }, text: SCSS [3] });
samples .push ({ spec: { name: "text/css", allowNested: true, supportsAtComponent: true }, text: CSS [4] + GSS [1] });
samples .push ({ spec: { name: "css", allowNested: true }, text: SCSS [3] });

const ALPHA = [
   "a", "b", "x", "-", "-", "--", "_", ".", "#", "@", "!", "$", "&", "%", "~", "|", "=", "+", ">", "*", "/", "\\",
   ":", ":", ";", ";", "{", "{", "}", "}", "(", ")", "[", "]", ",", "\"", "'", " ", " ", " ", "\t", "\n", "\n", "\r\n",
   "0", "1", "9", "e", "px", "em", "%", "fff", "abc", "url", "url(", "domain(", "regexp(", "url-prefix(",
   "color", "width", "red", "none", "important", "media", "@media", "@font-face", "@counter-style", "@keyframes",
   "@-webkit-keyframes", "@supports", "@import", "@document", "@-moz-document", "@component", "@page", "@charset",
   "font-family", "system", "symbols", "screen", "and", "only", "not", "print", "min-width", "landscape", "hover",
   "/*", "*/", "//", "#{", "!important", "zoom", "scrollbar-face-color", "transparent", "inherit", "é", "\u00a0",
   "\u3000", "\ud83d\ude00", "\ud800", "K",
];

export default {
   samples,
   random: [
      { seed: 4141, count: 250, maxLen: 50, alphabet: ALPHA, specs: ["css"] },
      { seed: 4242, count: 150, maxLen: 50, alphabet: ALPHA, specs: ["text/x-scss"] },
      { seed: 4343, count: 150, maxLen: 50, alphabet: ALPHA, specs: ["text/x-less"] },
      { seed: 4444, count: 100, maxLen: 50, alphabet: ALPHA, specs: ["text/x-gss"] },
      { seed: 4545, count: 100, maxLen: 50, alphabet: ALPHA, specs: [{ name: "css", inline: true }] },
      { seed: 4646, count: 150, maxLen: 120, alphabet: ALPHA, specs: ["css", "text/x-scss", "text/x-less", "text/x-gss"] },
   ],
   files: [
      { spec: "css", paths: walk (CM5, ".css") },
      { spec: "text/x-scss", paths: walk (join (CM5, "theme"), ".css") .slice (0, 20) .concat (walk (join (CM5, "addon"), ".css")) },
      { spec: "text/x-less", paths: walk (join (CM5, "theme"), ".css") .slice (20, 40) .concat (walk (join (CM5, "lib"), ".css")) },
      { spec: "text/x-gss", paths: walk (join (CM5, "lib"), ".css") .concat (walk (join (CM5, "doc"), ".css")) },
   ],
};
