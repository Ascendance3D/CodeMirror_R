/**
 * Inputs for `node oracle/cm5-modes-tokens.mjs javascript`: mode/javascript
 * (javascript, json, json-ld, typescript).
 */
import { readdirSync, statSync } from "node:fs";
import { join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = normalize (join (fileURLToPath (import.meta.url), "..", "..", ".."));

function walk (dir, ext, out = [])
{
   for (const name of readdirSync (dir) .sort ())
   {
      const p = join (dir, name);
      if (statSync (p) .isDirectory ()) { if (name != "node_modules") walk (p, ext, out); }
      else if (name .endsWith (ext)) out .push (p);
   }
   return out;
}

const JS = [
   "",
   "\n\n",
   "var a = 1, b = a / 2 / 3;\nvar re = /ab+c/gi.test(s);\nx = a\n/b/g\n",
   "if (a) /re/.exec(b); else c = d /e/ f;\nreturn /x[/]y/i\n",
   "a = b\n++c\nd = e\n(f)\nlet g = h\n[1, 2].forEach(i)\n",
   "const f = (a, b = 2, ...c) => a + b;\nconst g = x => x * 2, h = async (y) => { await y; };\n",
   "foo(a => b, (c, {d, e: [f]}) => d, \"=>\");\nx = (a /* ) */, b) => 1;\n(a, `(`) => a;\n",
   "let s = `plain ${a + `nested ${b}`} more\nline two ${ {x: 1}.x } end`;\ntag`t${1}`;\n",
   "class A extends B {\n  #priv = 1;\n  static #s() { return this.#priv; }\n  get x() { return 1 }\n  set x(v) {}\n  async *gen() { yield* other(); }\n  static { init(); }\n}\n",
   "async function f() { for await (const x of xs) { await x; } }\nfunction* g() { yield 1; yield* h(); }\nconst o = { async m() {}, *n() {}, get p() { return 1 }, [k]: v, ...rest, 'q': 1, 2: 3 };\n",
   "import a, { b as c, d } from \"mod\";\nimport * as ns from 'ns';\nimport('dyn').then(m => m);\nimport.meta.url;\nexport default function () {}\nexport { a as b, c };\nexport * from \"x\";\nexport const k = 1;\n",
   "outer: for (var i = 0; i < 3; i++) {\n  inner: while (true) { break outer; continue inner; }\n}\n",
   "switch (x) {\ncase 1:\n  a();\n  break;\ncase \"two\": {\n  b();\n}\ndefault:\n  c();\n}\n",
   "/* block\n   comment */ x = 1; // line\n/** doc */\nvar y; /* a */ /* b */ z\n<!-- html comment\n--> also comment\nx --> not\n",
   "var ünïcödé = 1, $ = 2, _x = 3, 日本 = 4,  x;\nπ = Math.PI;\n",
   "lone = \"\ud83d\"; \ude00x = 1; s = '\ud800';\n",
   "\\ a \\\nx \\ y\n/* c */ \\ d\n\"str\\\ncontinued\" + 'x\\\\'\n",
   "x = 0x1F + 0o17 + 0b101 + 1_000_000n + .5e-3 + 1.e5 + 5..toString() + 0xn;\n",
   "a ?? b; a?.b?.(c)?.[d]; a ||= b; a &&= c; a ??= d; a **= 2; a >>>= 1; a !== b; a === c;\n",
   "new.target; new Foo; new Foo.Bar(); new (a())(); super.x(); delete a.b; void 0; typeof x; x instanceof Y; 'k' in o;\n",
   "try { a() } catch { b() } finally { c() }\ntry { a() } catch (e) { e } \ndebugger;\nwith (o) { p }\ndo { x++ } while (x < 3)\n",
   "if (a)\n  b();\nelse if (c)\n  d();\nelse\n  e();\nif (x) {\n} else {\n}\n",
   "function f(a,\n         b) {\n  return a +\n    b;\n}\nvar x = {\n  a: 1,\n  b: [\n    1,\n    2\n  ]\n};\nfoo(1,\n    2);\n",
   "var a = 1,\n    b = 2;\nlet c =\n  3;\nconst d = function () {\n  return\n}\n",
   "#!/usr/bin/env node\nconsole.log(1)\n",
   "@decorator class X {}\n@dec(1) export class Y { @prop x; @m() method() {} }\n",
   "label:\n{ a }\nx = { a, b }\n({ a } = b);\n[a, b] = [b, a];\nfor (const [k, v] of Object.entries(o)) {}\nfor (let k in o) {}\nfor (;;) {}\n",
   "function f() { var x = 1; { let y = 2; var z = 3; } return x + y + z; }\n(function () { var q; q; })();\nx => x\n",
   "a = b ? c : d ? e : f;\nx = y\n  ? 1\n  : 2;\n",
   "if (a) return\nelse b\n}}}]])\n((((\n",
   "`unterminated ${ x\n} still`\n`${`${`${1}`}`}`\n",
   "var x = /[/\\]]+/g, y = /\\//, z = a / b / c;\nreturn /=/.test(s) /= 2;\n",
   "x\n.foo()\n.bar();\nvar s = 'a' +\n  'b';\n",
   "function* g(){}\nasync () => {}\nasync x => x\nasync\nfunction h(){}\nasync /* c */ (a) => a\nasync = 1;\n",
];

const TS = [
   "class Foo<T extends Bar = Baz> extends Some<Type> implements I, J {\n  private readonly x: number = 1;\n  public static y?: string;\n  protected abstract z!: Map<string, Array<number[]>>;\n  constructor(private a: A, public b?: B) { super(); }\n}\n",
   "interface I<T> {\n  a: string;\n  b?(x: number): void;\n  [key: string]: any;\n  readonly c: T[];\n  new (x: number): I<T>;\n}\n",
   "enum E { A = 1, B, C = \"c\" }\nconst enum F { X }\ndeclare module \"m\" { export function f(): void; }\nnamespace N.M { export const x = 1; }\ntype T = A | B & C | keyof D | typeof e | infer U;\ntype Fn = (a: number, ...rest: string[]) => void;\n",
   "let x = y as any as Foo<Bar>;\nlet z = <Foo>w;\nfoo<number>(1);\nnew Map<string, number>();\nfunction isFoo(x): x is Foo { return true }\nconst f = (a: number, b: string): Promise<void> => {};\nconst g = async <T,>(x: T): T => x;\n",
   "abstract class A { abstract m(): void; }\nexport type { T } from './t';\nlet t: [a: string, b?: number];\nlet u: `prefix-${string}`;\nlet v: { a: 1; b: 2 }[];\nfunction f(this: Window, x?: number) {}\n",
   "@Component({ selector: 'x' })\nexport class C { @Input() name: string; }\nlet a!: number; a!.b; x satisfies Y;\nfor (const k of keys) {}\n",
];

const JSON_ = [
   "{\"a\": 1, \"b\": [true, false, null], \"c\": {\"d\": \"e\\\"f\"}, \"g\": -1.5e10}\n",
   "[\n  1,\n  2,\n  {\n    \"x\": \"y\"\n  }\n]\n",
   "{ bad: 'json', // comment\n /* c */ \"k\": undefined, \"n\": NaN }\n",
   "\"just a string\"\n42\n",
   "{\"unterminated\": \"abc\n\"next\": 1}\n",
];

const LD = [
   "{\n  \"@context\": {\n    \"@base\": \"http://example.com\",\n    \"@vocab\": \"http://xmlns.com/foaf/0.1/\"\n  },\n  \"@graph\": [ { \"@id\": \"x\", \"@type\": \"T\", \"@fake\": 1 } ],\n  \"@ID\": \"@@ID\"\n}\n",
   "{\"@reverse\": {\"@index\": \"@list\"}, \"@set\": [\"@value\", \"@language\"], \"@container\": \"@graph\"}\n",
];

const samples = [];
for (const text of JS)    samples .push ({ spec: "javascript", text });
for (const text of JS .slice (0, 12)) samples .push ({ spec: "text/javascript", text });
for (const text of TS)    samples .push ({ spec: "application/typescript", text });
for (const text of JS .slice (0, 12)) samples .push ({ spec: "text/typescript", text });
for (const text of JSON_) samples .push ({ spec: "application/json", text });
for (const text of LD)    samples .push ({ spec: "application/ld+json", text }, { spec: { name: "javascript", jsonld: true }, text });
// Config variants.
for (const text of JS .slice (8, 24))
{
   samples .push ({ spec: { name: "javascript", statementIndent: 4 }, text });
   samples .push ({ spec: { name: "javascript", doubleIndentSwitch: false }, text });
   samples .push ({ spec: { name: "javascript", trackScope: false }, text });
}

const JS_ALPHA = ["a", "b", "x", "$", "_", " ", " ", "\t", "\n", "\n", "\r\n", "1", "0x1", ".5", "e3", "n", "(", ")", "{", "}", "[", "]",
                  ";", ",", ".", "...", ":", "?", "?.", "=", "=>", "==", "+", "++", "-", "--", "*", "/", "//", "/*", "*/", "%",
                  "<", ">", "<!--", "-->", "!", "&&", "||", "\\", "\"", "'", "`", "${", "#", "@", "é", "\ud83d", " ",
                  "var ", "let ", "const ", "function ", "return ", "if ", "else ", "for ", "while ", "do ", "switch ", "case ",
                  "default", "break", "new ", "this", "class ", "extends ", "async ", "await ", "yield ", "import ", "export ",
                  "from ", "as ", "of ", "in ", "typeof ", "try ", "catch ", "finally ", "get ", "set ", "static ", "super",
                  "true", "null", "target", "/re/g"];
const TS_ALPHA = JS_ALPHA .concat (["interface ", "type ", "enum ", "namespace ", "module ", "declare ", "abstract ", "private ",
                  "readonly ", "public ", "keyof ", "infer ", "is ", "implements ", "void", "<T>", ": number", "string", "|", "&"]);
const JSON_ALPHA = ["{", "}", "[", "]", ":", ",", " ", "\n", "\"a\"", "\"@id\"", "\"@context\"", "\"@type\"", "\"x\\\"y\"", "1", "-2.5e3",
                    "true", "false", "null", "\"", "\\", "@", "'k'", "/*", "//", "x"];

const random = [
   { seed: 1001, count: 300, maxLen: 60, alphabet: JS_ALPHA,   specs: ["javascript"] },
   { seed: 1002, count: 150, maxLen: 50, alphabet: JSON_ALPHA, specs: ["application/json", { name: "javascript", jsonld: true }] },
   { seed: 1003, count: 250, maxLen: 60, alphabet: TS_ALPHA,   specs: ["application/typescript"] },
];

const items = join (ROOT, "oracle", "corpus-items");
const up    = join (ROOT, "upstream", "codemirror5");
const upJs  = ["src", "mode", "addon", "keymap", "lib"] .flatMap (d => walk (join (up, d), ".js"));

const files = [
   { spec: "javascript",             paths: walk (items, ".js") },
   { spec: "application/json",       paths: walk (items, ".json") },
   { spec: "javascript",             paths: upJs },
   { spec: "application/typescript", paths: upJs .filter ((_, i) => i % 25 == 0) },
];

export default { samples, random, files };
