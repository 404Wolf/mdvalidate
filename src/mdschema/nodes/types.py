#!/usr/bin/env python3
import json
import sys
from typing import Any, Dict, List


def escape_str(str: str) -> str:
    return str.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")


def make_variant_name(node_type: str) -> str:
    """
    Convert a node type string into a valid Rust enum variant name.

    Makes names like test_foo into TestFoo
    """
    specials = {
        "!": "ExclamationMark",
        '"': "DoubleQuote",
        "#": "Hash",
        "$": "Dollar",
        "%": "Percent",
        "&": "Ampersand",
        "'": "SingleQuote",
        "(": "LeftParen",
        ")": "RightParen",
        "{": "LeftCurly",
        "}": "RightCurly",
        "|": "AbsoluteValue",
        "~": "Tilde",
        "*": "Asterisk",
        "+": "Plus",
        ",": "Comma",
        "-": "Minus",
        "-->": "ArrowRight",
        ".": "Dot",
        "/": "Slash",
        ":": "Colon",
        ";": "Semicolon",
        "<": "LeftAngle",
        "=": "Equal",
        ">": "RightAngle",
        "?": "Question",
        "?>": "QuestionRight",
        "@": "At",
        "[": "LeftBracket",
        "\\": "Backslash",
        "]": "RightBracket",
        "]]>": "DoubleRightBracketGreater",
        "^": "Caret",
        "`": "Backtick",
        "": "Nothing",
        "_": "Underscore",
    }

    if node_type in specials:
        return specials[node_type]

    return "".join(part.capitalize() for part in node_type.split("_"))


def main() -> None:
    try:
        data: List[Dict[str, Any]] = json.load(sys.stdin)
    except json.JSONDecodeError as e:
        print(f"// Failed to parse JSON: {e}", file=sys.stderr)
        sys.exit(1)

    # Collect unique type strings
    types = sorted({entry["type"] for entry in data if "type" in entry})

    # Build mapping from original type string -> Rust variant name
    type_to_variant = {t: make_variant_name(t) for t in types}

    # Sanity check: ensure uniqueness of variant names
    variants = list(type_to_variant.values())
    if len(variants) != len(set(variants)):
        print("// Error: generated duplicate Rust variant names", file=sys.stderr)
        sys.exit(1)

    out = []

    out.append("// Generated from JSON node type description")
    out.append("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]")
    out.append("pub enum NodeType {")
    for t in types:
        out.append(f"    {type_to_variant[t]},")
    out.append("}")
    out.append("")

    # Make a function to go from a string to the enum variant
    out.append("impl NodeType {")
    out.append("    pub fn from_str(s: &str) -> Option<Self> {")
    out.append("        match s {")
    for t in types:
        variant = type_to_variant[t]
        out.append(f'            "{escape_str(t)}" => Some(NodeType::{variant}),')
    out.append("            _ => None,")
    out.append("        }")
    out.append("    }")
    out.append("}")

    # as_str() mapping back to the original Tree-sitter node type string
    out.append("impl NodeType {")
    out.append("    pub fn as_str(&self) -> &'static str {")
    out.append("        match self {")
    for t in types:
        variant = type_to_variant[t]
        out.append(f'            NodeType::{variant} => "{escape_str(t)}",')
    out.append("        }")
    out.append("    }")
    out.append("}")
    out.append("")

    sys.stdout.write("\n".join(out))


if __name__ == "__main__":
    main()
