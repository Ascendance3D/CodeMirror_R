// C4 inputs for the new VRML mode (oracle/cm5-vrml-mode.js vs crates/cm-modes/src/vrml.rs).
// Files: every .wrl of the frozen Mall item snapshot oracle/corpus-items (gzip in memory).
import { listFiles } from "./_files.mjs";

const samples = [
   "",
   "#VRML V2.0 utf8\n",
   "#VRML V2.0 utf8\n# a comment\nShape { geometry Box { size 1 2 3 } }\n",
   "#X3D V3.3 utf8\nPROFILE Immersive\nCOMPONENT Geospatial:1\nMETA \"title\" \"x\"\nUNIT length km 1000\n",
   "  #VRML V2.0 utf8 (indented: not a header)\n",
   "\n#VRML V2.0 utf8 (second line: not a header)\n",
   "#VRML V1.0 ascii\nSeparator { Cube {} }\n",
   "DEF Root Transform {\n  translation 0 1.5 -2\n  rotation 0 1 0 1.5708\n  children [\n    DEF S Shape {\n      appearance Appearance { material Material { diffuseColor 1 0 0 } }\n      geometry Sphere { radius .5 }\n    }\n    USE S\n  ]\n}\n",
   "PROTO Door [\n  field SFVec3f size 1 2 0.1\n  exposedField SFColor color 0.5 0.3 0.1\n  eventIn SFBool open\n  eventOut SFTime openedAt\n  field MFString url [ \"a.wrl\" \"b.wrl\" ]\n]\n{\n  Transform { scale IS size }\n}\nDoor { size 2 2 2 }\n",
   "EXTERNPROTO Ext [ field SFNode n exposedField MFNode kids ] [ \"protos.wrl#Ext\", \"urn:web3d:x\" ]\n",
   "ROUTE Timer.fraction_changed TO Interp.set_fraction\nROUTE A .b TO C. d\nROUTE x.y TO\nROUTE . TO .\n",
   "Script {\n  eventIn SFTime touched\n  field SFNode me USE ME\n  directOutput TRUE\n  url \"javascript:\n    function touched(v, t) {\n      print(\\\"hi \\\\ there\\\");\n    }\n  \"\n}\n",
   "Text { string [ \"line one\", \"line \\\"two\\\"\" , \"unterminated\n continues\" ] }\n",
   "Coordinate { point [ 0 0 0, 1 0 0,, 1 1 0, -1e3 +2.5E+2 .5 5. 0x1F 0XAB 0x 1e 1e+ 3.2.1 ] }\n",
   "IndexedFaceSet { coordIndex [ 0 1 2 -1 2 3 0 -1 ] solid FALSE ccw TRUE convex NULL }\n",
   "Group { children [ ] ] } } [ { ] }\n",
   "Transform { children [ Inline { url \"x.wrl\" } ] }\n  ] trailing\n",
   "weird 'chars' \\ backslash \u0001 control \u007f del\n",
   "ünïcödé Ñame { fïeld 1 } 😀 \ud83d lone 名前\n",
   "a+b c-d e1 f.g h#comment\n+x -y .z\n",
   "Anchor{url\"x\"description\"y\"}Billboard[]Box{}\n",
   "\tTransform {\n\t\tchildren [\n\t\t\tShape {}\n\t\t]\n\t}\n",
   "Transform {\r\n  children [\r\n  ]\r\n}\r\n",
   "DEF\n  Name\n  Transform\n  {\n  }\nUSE # comment between\n  Name\n",
   "field\nexposedField SFFloat\n x\ninitializeOnly MFVec3d y\ninputOutput SFColorRGBA z\n",
   "IS\nIS x\nTO TO x\nIMPORT a.b AS c\nEXPORT d AS e\n",
   "Foo{}\nFoo {}\nFoo\n{\n}\nfoo { bar }\n",
   "\"string at start\"\n\"open\n\n\nstill open\" done\n",
   "SFBool MFNode Transform TimeSensor WorldInfo NavigationInfo",
];

const ALPHA = ["DEF", "USE", "PROTO", "EXTERNPROTO", "IS", "ROUTE", "TO", "TRUE", "FALSE", "NULL", "field", "exposedField", "eventIn", "eventOut",
               "SFFloat", "MFString", "Transform", "Shape", "Foo", "x", "y_1", "a.b", ".", ",", " ", " ", " ", "\t", "\n", "\n", "\n", "\r\n",
               "{", "}", "[", "]", "\"", "\\", "#", "#VRML V2.0 utf8", "0", "1.5", "-2e3", "+.5", "0x1F", "é", "😀", "'", " "];

export default {
   samples: samples .map (text => ({ spec: "vrml", text })) .concat ([{ spec: "model/vrml", text: samples [7] }, { spec: "x-world/x-vrml", text: samples [8] }]),
   random: [
      { seed: 9701, count: 500, maxLen: 60, alphabet: ALPHA, specs: ["vrml"] },
   ],
   files: [
      { spec: "vrml", paths: listFiles ("oracle/corpus-items", [".wrl"]) },
   ],
};
