// C4 inputs for htmlmixed (mode/htmlmixed/htmlmixed.js): html with script/style bodies.
import { listFiles } from "./_files.mjs";

const docs = [
   "<!doctype html>\n<html>\n<head>\n  <style>\n    body { color: red; }\n    a:hover { x: 1 }\n  </style>\n  <script>\n    function f(a) {\n      return a < 2;\n    }\n  </script>\n</head>\n<body onload=\"f()\">\n  <p>hi</p>\n</body>\n</html>",
   "<script type=\"text/javascript\">var a = '</script>';</script>after",
   "<script type=module>import x from 'y'\nexport default x</script>",
   "<script type=\"\">x</script><script type=\"text/plain\">not js < here</script>",
   "<script type='application/x-ecmascript'>a</script><script type=\"text/x-template\">{{x}}</script>",
   "<script lang=\"babel\">let x = <div/>;</script><script lang=ts>let y: number</script>",
   "<style type=\"text/css\">a{}</style><style type=\"text/less\">@x: 1;</style><style lang=\"css\">b{}</style><style type=x-css>c{}</style>",
   "<script>\n  if (a)\n    b();\n  else {\n    c();\n  }\n</script>\n<style>\n  a {\n    color: red;\n  }\n</style>",
   "<script>x = 1 </script >y</SCRIPT>z</ScRiPt>",
   "<script>a <\n/script> b\n</script>",
   "<script>x <</script>",
   "<script>x </</script>",
   "<script>\"</scr\" + \"ipt>\"</script>",
   "<script>/* </script> */ y</script>",
   "<script>// comment </script>\nnext</script>",
   "<script\n  type=\"text/javascript\"\n  src=\"x.js\">\n  var a;\n</script>",
   "<script/>after<script />x",
   "<script src=a.js></script><style media=screen></style>",
   "<pre>\n<script>\n  var a = {\n    b: 1\n  };\n</script>\n<style>\n  a {\n    b: c;\n  }\n</style>\n</pre>",
   "<div>\n  <script>\n    var x;\n  </script>\n  <style>\n    p {}\n  </style>\n</div>",
   "<SCRIPT TYPE=\"TEXT/JAVASCRIPT\">a</SCRIPT><Style>b{}</Style>",
   "<script type=\"text/x-handlebars-template\">{{#each}}</script><script type=\"text/vbscript\">MsgBox 1</script>",
   "<style type=\"text/x-scss\">$a: 1; .b { &:hover { c: $a } }</style><custom>key = 1\n[t]</custom><custom x>a = \"b\"</custom>",
   "<script>\n`template ${\n  x\n}`\n</script>",
   "<script>if (a) {</script><p>\n  x\n</p>",
   "<script type = ' text/javascript ' >a</script><script type=\"text/javascript;version=1\">b</script>",
   "<script language=\"javascript\" type=\"text/vbscript\">x</script>",
   "<!-- <script> --> <script>a</script>",
   "<textarea><script>x</script></textarea>",
   "<script>a</script\n>b",
   "<script\ttype=\"text/javascript\">\tz</script>",
   "<scripts>x</scripts><styles>y</styles>",
   "<script>😀 é \ud83d</script>",
];

const SPECS = ["text/html", "htmlmixed", "@htmlmixed-scripttypes", "@htmlmixed-tags",
               { name: "htmlmixed", multilineTagIndentPastTag: false, multilineTagIndentFactor: 2 },
               { name: "htmlmixed", allowMissingTagName: true }];
const samples = [];
for (const spec of SPECS)
   for (const text of docs)
      samples .push ({ spec, text });

const ALPHA = ["<script>", "</script>", "<style>", "</style>", "<script type=", "\"text/plain\"", "'module'", "<style type=x>", "<custom>", "</custom>",
               "<pre>", "</pre>", "<", ">", "/", "</", "{", "}", "(", ")", ";", ":", "a", "b", "x", " ", " ", "\n", "\n", "\t", "'", "\"", "`", "${",
               "/*", "*/", "//", "=", "<!--", "-->", "var", "if", "else", "color", "red", "@media", "#id", ".c", "😀"];

export default {
   samples,
   random: [
      { seed: 2001, count: 400, maxLen: 50, alphabet: ALPHA, specs: ["text/html", "@htmlmixed-tags"] },
      { seed: 2002, count: 100, maxLen: 100, alphabet: ALPHA, specs: ["@htmlmixed-scripttypes", { name: "htmlmixed", allowMissingTagName: true }] },
   ],
   files: [
      { spec: "text/html", paths: listFiles ("upstream/codemirror5", [".html"], ["node_modules"]) },
   ],
};
