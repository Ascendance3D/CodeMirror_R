/**
 * P10-00 oracle: the CodeMirror 5 mode infrastructure (src/modes.js, mode/meta.js,
 * addon/mode/simple.js, overlay.js, multiplex.js) and JS RegExp semantics, recorded
 * from a built pinned tree. The Rust side is crates/cm-modes/tests/mode_infra_oracle.rs.
 *
 *   node oracle/cm5-mode-infra.mjs <built-cm5-dir> [out.json]
 *        default out: crates/cm-modes/tests/data/cm5-mode-infra.json
 *
 * <built-cm5-dir> is a disposable copy of oracle/cm5-build after `npm run build`
 * (addon/runmode/runmode.node.js is a build output). Never build in oracle/cm5-build.
 *
 * Sections:
 *   regexp   exec results (group spans via the `d` flag) or the SyntaxError, per case;
 *   harvest  every rule regexp of the six upstream simple modes, compiled as
 *            simple.js's toRegex does, matched at every position of every line of
 *            the mode's own demo (index.html), as a SHA-256 digest of the records;
 *   meta     the whole modeInfo list and findModeBy* results;
 *   resolve  resolveMode results;
 *   runs     token runs of the test modes defined below (twins in the Rust test):
 *            text, style, line, start, a state probe after each token, blankLine
 *            results, and indent() at each line start.
 */
import { createRequire } from "node:module";
import { readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const HERE  = normalize (join (fileURLToPath (import.meta.url), ".."));
const ROOT  = normalize (join (HERE, ".."));
const BUILD = process .argv [2];
if (!BUILD) { console .error ("usage: node oracle/cm5-mode-infra.mjs <built-cm5-dir> [out.json]"); process .exit (2); }
const OUT   = process .argv [3] || join (ROOT, "crates", "cm-modes", "tests", "data", "cm5-mode-infra.json");
const CASES = JSON .parse (readFileSync (join (ROOT, "crates", "cm-modes", "tests", "data", "mode-infra-cases.json"), "utf8"));

const require = createRequire (import.meta.url);
const CM      = require (join (BUILD, "addon", "runmode", "runmode.node.js"));
CM .Pass = { toString: function () { return "CodeMirror.Pass"; } };

const enc = s => s .isWellFormed () ? s : Array .from ({ length: s .length }, (_, i) => s .charCodeAt (i));
const dec = v => typeof v == "string" ? v : String .fromCharCode (... v);

// --- regexp ---------------------------------------------------------------------

function execCase ([source, flags, input, from])
{
   let re;
   try { re = new RegExp (dec (source), flags + "d"); }
   catch (e) { return { error: true }; }
   re .lastIndex = from;
   const m = re .exec (dec (input));
   if (!m) return { match: null };
   return { match: m .indices .map (x => x ? [x [0], x [1]] : null) };
}

// --- harvest --------------------------------------------------------------------

const SIMPLE_MODES = ["dockerfile", "factor", "handlebars", "nsis", "rust", "wast"];
// runmode.node.js makes registerHelper a no-op; keep what the modes register.
const helpers = {};
CM .registerHelper = function (type, name, value) { (helpers [type] = helpers [type] || {}) [name] = value; };
require (join (BUILD, "addon", "mode", "simple.js"));
require (join (BUILD, "addon", "mode", "overlay.js"));
require (join (BUILD, "addon", "mode", "multiplex.js"));
require (join (BUILD, "mode", "meta.js"));
for (const m of ["xml/xml.js", "javascript/javascript.js", "css/css.js", "htmlmixed/htmlmixed.js"])
   require (join (BUILD, "mode", m));

function toRegex (val, caret)   // addon/mode/simple.js
{
   if (!val) return /(?:)/;
   let flags = "";
   if (val instanceof RegExp) { if (val .ignoreCase) flags = "i"; if (val .unicode) flags += "u"; val = val .source; }
   else val = String (val);
   return new RegExp ((caret === false ? "" : "^") + "(?:" + val + ")", flags);
}

function harvest ()
{
   const save = CM .defineSimpleMode;
   const regexps = [];
   CM .defineSimpleMode = function (name, states)
   {
      for (const s in states)
      {
         if (s == "meta" || !Array .isArray (states [s])) continue;
         for (const rule of states [s])
         {
            const r = toRegex (rule .regex);
            regexps .push ([enc (r .source), r .flags]);
            if (rule .mode && rule .mode .end)
            {
               const e = toRegex (rule .mode .end);
               regexps .push ([enc (e .source), e .flags]);
            }
         }
      }
      return save .apply (this, arguments);
   };
   for (const m of SIMPLE_MODES) require (join (BUILD, "mode", m, m + ".js"));
   CM .defineSimpleMode = save;
   const lines = [];
   for (const m of SIMPLE_MODES)
   {
      const html = readFileSync (join (BUILD, "mode", m, "index.html"), "utf8");
      const code = html .match (/<textarea[^>]*id="code"[^>]*>([\s\S]*?)<\/textarea>/);
      if (code) lines .push (... code [1] .split (/\r?\n|\r/));
   }
   const h = createHash ("sha256");
   let records = 0, matched = 0;
   for (const [source, flags] of regexps)
   {
      const re = new RegExp (dec (source), flags + "d");
      for (const line of lines)
         for (let pos = 0; pos <= line .length; ++pos)
         {
            const m = re .exec (line .slice (pos));
            const rec = m ? m .indices .map (x => x ? x [0] + "-" + x [1] : "u") .join (",") : "-";
            if (m) ++matched;
            h .update (Buffer .from (rec + "\n", "utf16le"));
            ++records;
         }
   }
   return { regexps, lines: lines .map (enc), records, matched, digest: h .digest ("hex") };
}

// --- test modes (twins in crates/cm-modes/tests/mode_infra_oracle.rs) -------------

CM .defineSimpleMode ("p10simple", {
   start: [
      { regex: /"(?:[^\\]|\\.)*?(?:"|$)/, token: "string" },
      { regex: /(function)(\s+)([a-z$][\w$]*)/, token: ["keyword", null, "variable-2"] },
      { regex: /(?:function|var|return|if|for|while|else|do|this)\b/, token: "keyword" },
      { regex: /true|false|null|undefined/, token: "atom" },
      { regex: /0x[a-f\d]+|[-+]?(?:\.\d+|\d+\.?\d*)(?:e[-+]?\d+)?/i, token: "number" },
      { regex: /\/\/.*/, token: "comment" },
      { regex: /\/\*/, token: "comment", next: "comment" },
      { regex: /<</, token: "meta", push: "heredoc" },
      { regex: /<script>/, token: "tag", mode: { spec: "javascript", end: /<\/script>/ } },
      { regex: /<style>/, token: ["tag", "tag.end"], mode: { spec: "css", end: /<\/style>/, persistent: true } },
      { regex: /<raw>/, token: "tag", mode: { spec: "javascript", end: /<\/raw>/, forceEnd: false } },
      { regex: /[-+\/*=<>!]+/, token: "operator" },
      { regex: /[\{\[\(]/, indent: true },
      { regex: /[\}\]\)]/, dedent: true },
      { regex: /@(\w+)(\.)(\w+)/, token: ["tag.name", "punct", "attr.x"] },
      { regex: /(\[\[)(\w+)(\]\])/, token: ["bracket", "attribute", "bracket"] },
      { regex: /%(\w+)(-?)(\w*)/, token: ["a", "b", "c"] },
      // All groups empty: the stream backs up to its start and never advances.
      { regex: /&(\w*)(\w*)/, token: ["amp", "x", "y"] },
      { regex: "#[A-Z]+", token: "hash.tag" },
      { regex: /\^(\w+)(!)/, token: function (m) { return m [1] == "up" ? "up" : ["fn-a", "fn-b"]; } },
      { regex: /~~/, sol: true, token: "solmark" },
      { regex: /é+|\u{1F600}/u, token: "uni" },
      { regex: /[A-Z]{2,}/i, token: "caps" },
      { regex: /[a-z$][\w$]*/, token: "variable" },
   ],
   comment: [
      { regex: /.*?\*\//, token: "comment", next: "start" },
      { regex: /.*/, token: "comment" },
   ],
   heredoc: [
      { regex: />>/, token: "meta", pop: true },
      { regex: /[^>]+|>/, token: "string-2" },
   ],
   meta: { dontIndentStates: ["comment"], lineComment: "//", electricInput: /^\s*\}$/ },
});
CM .defineMIME ("text/x-p10simple", "p10simple");

const todoToken = function (stream)
{
   if (stream .match ("TODO")) return "todo";
   while (stream .next () != null && !stream .match ("TODO", false)) {}
   return null;
};
const todoOverlay  = { token: todoToken };
const todoCombine  = { startState: () => ({ combineTokens: true }), token: todoToken, blankLine: () => "line-blank" };
CM .defineMode ("p10overlay", function (config, cfg)
{
   return CM .overlayMode (CM .getMode (config, cfg .backdrop || "javascript"), cfg .combine2 ? todoCombine : todoOverlay, cfg .combine);
});

CM .defineMode ("p10multi", function (config)
{
   return CM .multiplexingMode (CM .getMode (config, "text/html"),
      { open: "{{", close: "}}", mode: CM .getMode (config, "javascript"), delimStyle: "delimit" },
      { open: /<%=?/, close: "%>", mode: CM .getMode (config, "text/css"), innerStyle: "inner" },
      { open: "[[", close: /\]\]/, mode: CM .getMode (config, "p10simple"), parseDelimiters: true },
      { open: "!!", mode: CM .getMode (config, "javascript"), delimStyle: "bang" });
});
CM .defineMode ("p10multinl", function (config)
{
   return CM .multiplexingMode (CM .getMode (config, "javascript"),
      { open: "\n", close: "\n", mode: CM .getMode (config, "text/css"), delimStyle: "nl" });
});

// --- runs -----------------------------------------------------------------------

function indentValue (r)
{
   if (typeof r == "number") return Number .isNaN (r) ? "NaN" : String (r);
   if (r === CM .Pass) return "Pass";
   return "other:" + typeof r;
}

function probe (mode, state)
{
   const inner = CM .innerMode (mode, state);
   let p = inner .mode .name == null ? "?" : inner .mode .name;
   if (mode .name == "p10simple")
      p += "|" + state .state + "|" + JSON .stringify (state .indent) + "|" + JSON .stringify (state .stack || []) +
           "|" + (state .local ? state .local .mode .name : "") + "|" + countPersistent (state);
   return p;
}

function countPersistent (state)
{
   let n = 0;
   for (let p = state .persistentStates; p; p = p .next) ++n;
   return n;
}

function run (c)
{
   const config = Object .assign ({}, CM .defaults, { indentUnit: c .indentUnit || 2 });
   const mode   = CM .getMode (config, c .spec);
   const lines  = CM .splitLines (c .text);
   const tokens = [], indents = [], blanks = [];
   let error = null;
   try
   {
      let state = CM .startState (mode);
      for (let i = 0; i < lines .length; ++i)
      {
         if (c .copyEachLine) state = CM .copyState (mode, state);
         if (mode .indent)
         {
            const ws = lines [i] .match (/^\s*/) [0];
            indents .push (indentValue (mode .indent (state, lines [i] .slice (ws .length), lines [i])));
         }
         const stream = new CM .StringStream (lines [i], null, { lookAhead: n => lines [i + n], baseToken: function () {} });
         if (!stream .string && mode .blankLine)
         {
            const b = mode .blankLine (state);
            blanks .push ([i, b == null ? null : b]);
         }
         let guard = 0;
         while (!stream .eol ())
         {
            const style = mode .token (stream, state);
            tokens .push ([enc (stream .current ()), style == null ? null : style, i, stream .start, probe (mode, state)]);
            if (stream .pos == stream .start && ++guard > 1000) throw new Error ("runMode spins: no progress");
            stream .start = stream .pos;
         }
      }
   }
   catch (e) { error = String (e && e .message || e); }
   return { id: c .id, name: mode .name, tokens, indents: mode .indent ? indents : null, blanks, error };
}

function expandRuns ()
{
   const out = [];
   for (const r of CASES .runs)
      for (const t of r .texts)
         for (const copyEachLine of [false, true])
            for (const indentUnit of r .indentUnits || [2])
               out .push ({ id: `${r .id}/${t}/${copyEachLine ? "copy" : "keep"}/${indentUnit}`, spec: r .spec,
                            text: dec (CASES .texts [t]), copyEachLine, indentUnit });
   return out;
}

// --- meta / resolve -------------------------------------------------------------

const infoName = i => i ? i .name : null;
const meta = {
   infos: CM .modeInfo .map (i => ({ name: i .name, mime: i .mime || null, mimes: i .mimes || [], mode: i .mode,
                                     ext: i .ext || [], alias: i .alias || [], file: i .file ? [i .file .source, i .file .flags] : null })),
   byMime: CASES .meta .mime .map (x => [x, infoName (CM .findModeByMIME (x))]),
   byExtension: CASES .meta .ext .map (x => [x, infoName (CM .findModeByExtension (x))]),
   byFileName: CASES .meta .file .map (x => [x, infoName (CM .findModeByFileName (x))]),
   byName: CASES .meta .name .map (x => [x, infoName (CM .findModeByName (x))]),
};

const resolve = CASES .resolve .map (spec =>
{
   const r = CM .resolveMode (spec);
   const opts = {};
   // Object-valued options (keyword tables) are compared by presence only. `for in`:
   // createObj puts the MIME spec's options on the prototype.
   const keys = [];
   for (const k in r) keys .push (k);
   for (const k of keys .sort ())
      if (k != "name") opts [k] = r [k] !== null && typeof r [k] == "object" ? "<obj>" : r [k];
   return [spec, r .name, opts, CM .getMode (CM .defaults, spec) .name];
});

const result = {
   cm5: JSON .parse (readFileSync (join (BUILD, "package.json"), "utf8")) .version,
   regexp: CASES .regexp .map (c => [c, execCase (c)]),
   harvest: harvest (),
   meta,
   resolve,
   helpers: { hintWordsCss: helpers .hintWords .css, wordCharsJavascript: [helpers .wordChars .javascript .source, helpers .wordChars .javascript .flags] },
   runs: expandRuns () .map (run),
};
writeFileSync (OUT, JSON .stringify (result, null, 0) + "\n");
console .log (`wrote ${OUT}: ${result .regexp .length} regexp cases, ${result .harvest .regexps .length} harvested regexps x ${result .harvest .records} records, ${result .runs .length} runs`);
