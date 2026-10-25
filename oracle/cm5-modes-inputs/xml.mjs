// C4 inputs for the xml mode (mode/xml/xml.js), xml and html configurations.
import { listFiles } from "./_files.mjs";

const HTML = { name: "xml", htmlMode: true };

const xmlSamples = [
   "",
   "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root a=\"1\" b='2'>\n  <child/>\n  text &amp; &#169; &#x1F600; &bad; & alone\n</root>\n",
   "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0//EN\" \"x\" [\n  <!ENTITY a \"b\">\n  <!ELEMENT x (y)>\n]>\n<x/>",
   "<!doctype foo <nested <deeper>> more> after",
   "<a>\n  <![CDATA[ x < y\n  ]] still ]]> tail\n</a>",
   "<!-- comment\n spanning -- lines -->\n<b><!-- one --></b>",
   "<a>\n  <b>\n    <c attr=\"multi\n      line\" d='x'\n       e=\"y\">\n  </b>\n</a>",
   "<top>\n  <inner>\n  </tip>\n</top>\n</extra>",
   "<a b=c d e=\"f\" = g / >\n<a/ >\n< a>\n</ a >\n<a<b>\n</>",
   "<x:y xmlns:x=\"u\" x:z='1'>\n<x:w/>\n</x:y>",
   "<a\n  b=\"1\"\n  c=\"2\">\n  <d\n    e=\"3\"/>\n</a>",
   "<?php echo 1 ?>\n<?pi\n stays meta ?> after",
   "<a>&#;&#x;&;&a.b-c:d;&#12a;</a>",
   "<![CDATA[x]]><!x><![x><!--x",
   "<a>\t\ttabs\t<b\t\tc='\t'>\t</b></a>",
   "<é attr=\"ü\">😀 \ud83d lone</é>",
   "<A></a><B></B >",
   "text only\nno tags\n\n   indented",
   "<a b='unterminated\nnext line\n\nstill' c=1>",
   "<a\n>\n</a\n>",
];

const htmlSamples = [
   "<html>\n  <head>\n    <link rel=stylesheet href=\"/foo\">\n    <meta charset=utf-8>\n  </head>\n  <body>\n    <p>para\n    <p>second\n    <ul>\n      <li>one\n      <li>two\n    </ul>\n    <table><tr><td>a<td>b<tr><td>c</table>\n    <br><hr><img src=x>\n  </body>\n</html>",
   "<p>foo\n<article>bar\n<dl><dt>a<dd>b<dt>c</dl>",
   "<pre>\n  keep\n    as is\n</pre>\n<div>\n  <pre>x\n  <b>y</b>\n  </pre>\n</div>",
   "<input type=checkbox checked disabled>\n<option selected>a<option>b\n<select><optgroup><option>x</optgroup></select>",
   "<DIV CLASS=a>\n  <SPAN>x</span>\n</DIV>",
   "<a href=x/y/>z</a>\n<a href=/>",
   "<rp>a<rt>b<rp>c\n<thead><tbody><tfoot><tbody>",
   "<li>a\n</ul>\n<p>x</div>",
   "<script>var x = 1 < 2;</script>\n<style>a{}</style>",
];

const samples = [];
for (const text of xmlSamples)
{
   samples .push ({ spec: "xml", text });
   samples .push ({ spec: HTML, text });
}
for (const text of htmlSamples)
{
   samples .push ({ spec: HTML, text });
   samples .push ({ spec: "application/xml", text });
}
const opts = [
   { name: "xml", matchClosing: false },
   { name: "xml", alignCDATA: true },
   { name: "xml", multilineTagIndentPastTag: false },
   { name: "xml", multilineTagIndentPastTag: false, multilineTagIndentFactor: 2 },
   { name: "xml", allowMissingTagName: true },
   { name: "xml", htmlMode: true, allowMissingTagName: true, matchClosing: false },
   { name: "xml", allowUnquoted: true, allowMissing: true },
   { name: "xml", htmlMode: true, allowUnquoted: false, allowMissing: false },
   "text/xml",
];
for (const spec of opts)
   for (const text of [xmlSamples [6], xmlSamples [7], xmlSamples [8], xmlSamples [4], xmlSamples [10], htmlSamples [0], "<>\n</>\n< />\n<a></>"])
      samples .push ({ spec, text });

const ALPHA = ["<", "<", ">", ">", "/", "</", "/>", "a", "b", "p", "li", "td", "pre", "br", "div", "Td", " ", " ", "  ", "\t", "\n", "\n", "\r\n",
               "=", "\"", "'", "x=\"1\"", "&", ";", "#", "amp", "!", "?", "--", "<!--", "-->", "<![CDATA[", "]]>", "<!DOCTYPE", "[", "]",
               "é", "😀", " ", ":", ".", "-", "_"];

export default {
   samples,
   random: [
      { seed: 1001, count: 300, maxLen: 60, alphabet: ALPHA, specs: ["xml", HTML] },
      { seed: 1002, count: 100, maxLen: 120, alphabet: ALPHA, specs: [{ name: "xml", allowMissingTagName: true, multilineTagIndentPastTag: false }] },
   ],
   files: [
      { spec: HTML, paths: listFiles ("upstream/codemirror5", [".html"], ["node_modules"]) },
      { spec: "xml", paths: [... listFiles ("upstream/codemirror5", [".svg"], ["node_modules"]),
                             ... listFiles ("upstream/codemirror5/demo", [".html"])] },
   ],
};
