// VRML97 (ISO/IEC 14772-1) classic-encoding mode for CodeMirror 5.
// New code for the Rust port lane (CM5 has no VRML mode). The Rust twin is
// crates/cm-modes/src/vrml.rs; `node oracle/cm5-modes-tokens.mjs vrml` diffs them.
// Usable in a real CodeMirror 5: drop it in as mode/vrml/vrml.js.
// MIT license, like CodeMirror.
//
// Styles:
//   meta       the "#VRML V2.0 utf8" / "#X3D V3.x" header comment on the first line
//   comment    "# ..." to the end of the line
//   keyword    DEF USE PROTO EXTERNPROTO IS ROUTE TO (and the X3D classic statements
//              PROFILE COMPONENT META UNIT IMPORT EXPORT AS); field access types
//              field exposedField eventIn eventOut (and X3D initializeOnly inputOnly
//              outputOnly inputOutput)
//   atom       TRUE FALSE NULL
//   type       field type names (SFBool ... MFVec3f, X3D additions)
//   tag        built-in VRML97 node type names
//   variable-2 any other name directly followed by "{" (PROTO instances)
//   def        the name after DEF, PROTO, EXTERNPROTO, or after a field's type
//   variable   the name after USE, and the node names of a ROUTE
//   property   field names (after "." in a ROUTE, after IS, and other plain names)
//   string     "..." with backslash escapes; may span lines
//   number     integers, floats, hex
//   bracket    { } [ ] (a mismatched closer is "error")
//   error      characters VRML does not allow outside strings and comments
// Commas are whitespace in VRML and get no style.

(function(mod) {
  if (typeof exports == "object" && typeof module == "object") // CommonJS
    mod(require("../../lib/codemirror"));
  else if (typeof define == "function" && define.amd) // AMD
    define(["../../lib/codemirror"], mod);
  else // Plain browser env
    mod(CodeMirror);
})(function(CodeMirror) {
"use strict";

CodeMirror.defineMode("vrml", function(config) {
  var indentUnit = config.indentUnit;

  function set(words) {
    var o = {};
    for (var i = 0; i < words.length; i++) o[words[i]] = true;
    return o;
  }

  var keywords = set(["DEF", "USE", "PROTO", "EXTERNPROTO", "IS", "ROUTE", "TO",
                      "PROFILE", "COMPONENT", "META", "UNIT", "IMPORT", "EXPORT", "AS"]);
  var atoms = set(["TRUE", "FALSE", "NULL"]);
  var accessTypes = set(["field", "exposedField", "eventIn", "eventOut",
                         "initializeOnly", "inputOnly", "outputOnly", "inputOutput"]);
  var fieldTypes = set([
    "SFBool", "SFColor", "SFColorRGBA", "SFDouble", "SFFloat", "SFImage", "SFInt32",
    "SFMatrix3d", "SFMatrix3f", "SFMatrix4d", "SFMatrix4f", "SFNode", "SFRotation",
    "SFString", "SFTime", "SFVec2d", "SFVec2f", "SFVec3d", "SFVec3f", "SFVec4d", "SFVec4f",
    "MFBool", "MFColor", "MFColorRGBA", "MFDouble", "MFFloat", "MFImage", "MFInt32",
    "MFMatrix3d", "MFMatrix3f", "MFMatrix4d", "MFMatrix4f", "MFNode", "MFRotation",
    "MFString", "MFTime", "MFVec2d", "MFVec2f", "MFVec3d", "MFVec3f", "MFVec4d", "MFVec4f"]);
  // The 54 VRML97 node types.
  var nodeTypes = set([
    "Anchor", "Appearance", "AudioClip", "Background", "Billboard", "Box", "Collision",
    "Color", "ColorInterpolator", "Cone", "Coordinate", "CoordinateInterpolator",
    "Cylinder", "CylinderSensor", "DirectionalLight", "ElevationGrid", "Extrusion", "Fog",
    "FontStyle", "Group", "ImageTexture", "IndexedFaceSet", "IndexedLineSet", "Inline",
    "LOD", "Material", "MovieTexture", "NavigationInfo", "Normal", "NormalInterpolator",
    "OrientationInterpolator", "PixelTexture", "PlaneSensor", "PointLight", "PointSet",
    "PositionInterpolator", "ProximitySensor", "ScalarInterpolator", "Script", "Shape",
    "Sound", "Sphere", "SphereSensor", "SpotLight", "Switch", "Text", "TextureCoordinate",
    "TextureTransform", "TimeSensor", "TouchSensor", "Transform", "Viewpoint",
    "VisibilitySensor", "WorldInfo"]);

  var has = Object.prototype.hasOwnProperty;
  // VRML97 IdFirstChar / IdRestChar: anything but controls, space, and " # ' + , - .
  // digits [ \ ] { } DEL. "+", "-" and digits may follow the first character.
  var identifier = /^[^\x00-\x20\x22\x23\x27\x2b-\x2e\x30-\x39\x5b-\x5d\x7b\x7d\x7f][^\x00-\x20\x22\x23\x27\x2c\x2e\x5b-\x5d\x7b\x7d\x7f]*/;
  var number = /^[+\-]?(?:0[xX][0-9a-fA-F]+|(?:\d+\.?\d*|\.\d+)(?:[eE][+\-]?\d+)?)/;

  function tokenString(stream, state) {
    var ch;
    while ((ch = stream.next()) != null) {
      if (ch == "\\") stream.next();
      else if (ch == "\"") { state.inString = false; break; }
    }
    return "string";
  }

  function tokenBase(stream, state) {
    var ch = stream.peek();
    if (ch == "#") {
      var header = state.atStart && stream.pos == 0 && stream.match(/^#(VRML|X3D) V/, false);
      stream.skipToEnd();
      return header ? "meta" : "comment";
    }
    var expect = state.expect;
    state.expect = null;
    if (ch == "\"") {
      stream.next();
      state.inString = true;
      return tokenString(stream, state);
    }
    if (ch == "{" || ch == "[") {
      stream.next();
      state.context = {prev: state.context, type: ch, base: state.indented, indent: state.indented + indentUnit};
      return "bracket";
    }
    if (ch == "}" || ch == "]") {
      stream.next();
      if (state.context && state.context.type == (ch == "}" ? "{" : "[")) {
        state.context = state.context.prev;
        return "bracket";
      }
      return "error";
    }
    if (ch == ",") {
      stream.eatWhile(",");
      state.expect = expect;
      return null;
    }
    if (stream.match(number)) return "number";
    if (ch == ".") {
      stream.next();
      if (expect == "routeDot") state.expect = "routeField";
      return null;
    }
    var m = stream.match(identifier);
    if (!m) {
      stream.next();
      return "error";
    }
    var word = m[0];
    if (has.call(keywords, word)) {
      if (word == "DEF" || word == "PROTO" || word == "EXTERNPROTO") state.expect = "def";
      else if (word == "USE") state.expect = "use";
      else if (word == "ROUTE" || word == "TO") state.expect = "route";
      else if (word == "IS") state.expect = "is";
      return "keyword";
    }
    if (has.call(atoms, word)) return "atom";
    if (has.call(accessTypes, word)) { state.expect = "fieldType"; return "keyword"; }
    if (expect == "fieldType") { state.expect = "fieldName"; return "type"; }
    if (expect == "fieldName" || expect == "def") return "def";
    if (expect == "use") return "variable";
    if (expect == "route") { state.expect = "routeDot"; return "variable"; }
    if (expect == "routeField" || expect == "is") return "property";
    if (has.call(fieldTypes, word)) return "type";
    if (has.call(nodeTypes, word)) return "tag";
    if (stream.match(/^\s*\{/, false)) return "variable-2";
    return "property";
  }

  return {
    startState: function(base) {
      return {inString: false, context: null, expect: null, indented: base || 0, base: base || 0, atStart: true};
    },

    token: function(stream, state) {
      if (stream.sol()) state.indented = stream.indentation();
      var style;
      if (state.inString) style = tokenString(stream, state);
      else if (stream.eatSpace()) style = null;
      else style = tokenBase(stream, state);
      state.atStart = false;
      return style;
    },

    blankLine: function(state) {
      state.atStart = false;
    },

    indent: function(state, textAfter) {
      if (state.inString) return CodeMirror.Pass;
      var cx = state.context, ch = textAfter && textAfter.charAt(0);
      if (!cx) return state.base;
      if (ch == "}" && cx.type == "{" || ch == "]" && cx.type == "[") return cx.base;
      return cx.indent;
    },

    electricChars: "}]",
    lineComment: "#",
    fold: "brace",
    closeBrackets: "[]{}\"\""
  };
});

CodeMirror.defineMIME("model/vrml", "vrml");
CodeMirror.defineMIME("x-world/x-vrml", "vrml");
CodeMirror.defineMIME("model/x3d+vrml", "vrml");

});
