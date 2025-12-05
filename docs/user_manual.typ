#import "template.typ": *

#show: template.with(
  title: [`mdvalidate`: \ User Manual], author: "Wolf Mermelstein and Alessandro Mason",
)

= Introduction

== What is `mdvalidate`?

`mdvalidate` is a command-line tool that validates Markdown documents against
schemas written in the Markdown Schema Language (MDS). It allows you to define
the expected structure and content patterns of Markdown files, then verify that
actual documents conform to those specifications.

== Key Features

- *Streaming Validation*: Validates Markdown as it streams in via stdin, making
  it perfect for validating LLM output in real-time
- *Schema Language*: Write schemas in Markdown itself — schemas are valid
  Markdown files
- *Pattern Matching*: Use regex patterns to validate dynamic content
- *JSON Output*: Extract matched content as structured JSON data
- *Fast Performance*: Built in Rust for blazing-fast validation
- *Incremental Parsing*: Uses Tree-sitter for efficient incremental validation

= Installation

== Building from Source

`mdvalidate` can be built using the Nix build system:

```bash
nix build github:404wolf/mdvalidate
```

The resulting binary will be available at `result/bin/mdv`.

== Development Build

For development, you can build using Cargo:

```bash
cargo build --release
```

The binary will be available at `target/release/mdv`.

= Basic Usage

== Command Syntax

The basic command syntax is:

```bash
mdv <schema-file> <input-file>
```

Or to read from stdin:

```bash
mdv <schema-file> -
```

== Command-Line Options

- `--fast-fail` or `-f`: Stop validation on the first error encountered
- `--quiet` or `-q`: Suppress non-error output (useful for scripts)
- `--output <file>` or `-o <file>`: Write JSON matches to a file (use `-` for
  stdout)
- `--help` or `-h`: Display help information
- `--version` or `-V`: Display version information

== Exit Codes

- `0`: Validation succeeded
- `1`: Validation failed or an error occurred

= Writing Schemas

== Schema Basics

A schema in MDS is itself a valid Markdown file. This means you can read and
edit schemas with any Markdown editor, and they will render correctly.

The fundamental principle is: *literal Markdown in the schema must match
exactly in the input*.

== Literal Matching

By default, all Markdown content in a schema is treated as a literal match.
This means the following schema:

```markdown
# Title

This is a paragraph.
```

Will only validate documents that contain exactly:

```markdown
# Title

This is a paragraph.
```

== Matchers

Matchers allow you to define patterns for dynamic content. The syntax is:

`` `label:/regex/` ``

Where:
- `label` is an identifier for the matched content (used in JSON output)
- `/regex/` is a Perl-compatible regular expression

Example:

```markdown
# `name:/[A-Z][a-z]+ [A-Z][a-z]+/`
```

This schema will match any heading that contains two capitalized words separated
by a space, such as:

```markdown
# John Smith
# Mary Johnson
```

== Special Matcher Types

=== Text Matcher

Use `text` to match any text content:

```markdown
`description:text`
```

=== Number Matcher

Use `number` to match numeric values:

```markdown
`count:number`
```

=== HTML Matcher

Use `html` to match HTML content:

```markdown
`content:html`
```

You can specify depth with `dn` suffix:

```markdown
`content:html`d2  // Matches HTML up to depth 2
```

=== Ruler Matcher

Match horizontal rules (dividers):

```markdown
`ruler`
```

This matches `---`, `***`, or `___`.

== Optional Content

Add a `?` suffix to make a matcher optional:

```markdown
`optional_title:text`?
```

== Repeated Content

Add a `+` suffix to match one or more occurrences:

```markdown
- `item:/[A-Z][a-z]+/`+
```

Add `{min,max}` to specify exact counts:

```markdown
- `item:/[A-Z][a-z]+/`{2,5}  // Between 2 and 5 items
```

== Lists

Lists can be validated with matchers:

```markdown
- `grocery_item:/[A-Z][a-z]+/`+
```

For nested lists, use indentation:

```markdown
- `item:/[A-Z][a-z]+/`+
  - `note:/\w+/`?{,2}  // Up to 2 optional notes per item
```

Add `dn` to specify maximum nesting depth:

```markdown
- `item:/[A-Z][a-z]+/`+d2  // Maximum depth of 2
```

== Escaping Matchers

To match a literal backtick code block (instead of treating it as a matcher),
add an exclamation mark:

```markdown
`example:/test/`!  // Matches the literal text "`example:/test/`"
```

Use `!!` to match a literal exclamation mark at the end:

```markdown
`example:/test/`!!  // Matches "`example:/test/`!"
```

== Empty Labels

Use an underscore `_` for matchers you don't need to extract:

```markdown
`_:/regex/`  // Matches but doesn't appear in JSON output
```

= Examples

== Example 1: Simple Contact Card

Schema (`contact.mds`):

```markdown
# `name:/[A-Z][a-z]+ [A-Z][a-z]+/`

`bio:text`?

## Contact Information

- Email: `email:/[a-z]+@[a-z]+\.[a-z]+/`
- Phone: `phone:/\(\d{3}\) \d{3}-\d{4}/`?
```

Valid input:

```markdown
# John Doe

Software engineer passionate about Rust.

## Contact Information

- Email: john@example.com
- Phone: (555) 123-4567
```

== Example 2: Assignment Template

Schema (`assignment.mds`):

```markdown
# CSDS 999 Assignment `assignment_number:/\d+/`

# `title:/(([A-Z][a-z]+ )|and |the )+([A-Z][a-z]+)/`

## `first_name:/[A-Z][a-z]+/`
## `last_name:/[A-Z][a-z]+/`

This is a shopping list:

- `grocery_list_item:/Hello \w+/`+
  - `grocery_item_notes:/.*/`?{,2}
```

Valid input:

```markdown
# CSDS 999 Assignment 7

# The Curious and Practical Garden

## Wolf
## Mermelstein

This is a shopping list:

- Hello Apples
  - Fresh from market
  - Organic
- Hello Bananas
  - Ripe
```

== Example 3: Grocery List with Constraints

Schema (`grocery.mds`):

```markdown
# Grocery List

- `item:/[A-Z][a-z]+/`+         <!-- one or more items -->
  - `note:/\w+/`?{,2}         <!-- up to two optional notes -->
```

Valid input:

```markdown
# Grocery List

- Apples
  - organic
  - local
- Bananas
  - ripe
```

Invalid input (too many notes):

```markdown
# Grocery List

- Apples
  - organic
  - local
  - green  <!-- Error: exceeds maximum of 2 notes -->
```

= Advanced Usage

== JSON Output

When validation succeeds, `mdvalidate` can output a JSON object containing all
matched labels:

```bash
mdv schema.mds input.md --output matches.json
```

Or to stdout:

```bash
mdv schema.mds input.md --output -
```

Example output:

```json
{
  "name": "John Doe",
  "email": "john@example.com",
  "phone": "(555) 123-4567"
}
```

== Streaming Validation

`mdvalidate` is designed to work with streaming input, making it ideal for
validating LLM output:

```bash
llm-generate | mdv schema.mds - --fast-fail
```

The `--fast-fail` flag will stop validation as soon as an error is detected,
allowing you to terminate the LLM generation early.

== Quiet Mode

Use `--quiet` for script integration:

```bash
if mdv schema.mds input.md --quiet; then
  echo "Validation passed"
else
  echo "Validation failed"
  exit 1
fi
```

== Logging

Enable debug logging by setting the `RUST_LOG` environment variable:

```bash
RUST_LOG=debug mdv schema.mds input.md
```

Available log levels: `trace`, `debug`, `info`, `warn`, `error`.

= Error Messages

== Understanding Errors

When validation fails, `mdvalidate` provides detailed error messages indicating:

- The file and line number where the error occurred
- What was expected
- What was found instead

Example error:

```
Error at input.md:5:12
Expected matcher `email:/[a-z]+@[a-z]+\.[a-z]+/` to match, but found "invalid-email"
```

== Common Issues

=== Regex Not Matching

Ensure your regex pattern correctly matches the expected content. Remember that
regex patterns are anchored — they must match the entire content of the node.

=== List Structure Mismatch

Check that list indentation in your input matches the schema. Sublists must be
properly indented.

=== Optional vs Required

Remember that without the `?` suffix, matchers are required. If content might
not be present, make it optional.

=== Escaping Special Characters

In regex patterns, escape special characters:

```markdown
`phone:/\(\d{3}\) \d{3}-\d{4}/`  // Escaped parentheses
```

= Best Practices

== Schema Design

1. *Start Simple*: Begin with literal matches, then add matchers as needed
2. *Use Descriptive Labels*: Choose clear label names for better JSON output
3. *Test Incrementally*: Validate your schema with sample inputs as you build
   it
4. *Document with Comments*: Use HTML comments in schemas to explain complex
   patterns

== Performance

- `mdvalidate` is optimized for streaming, but very large files may take
  longer
- Use `--fast-fail` when you only need to know if validation passes, not all
  errors
- Consider breaking very large documents into smaller validated sections

== Integration

=== CI/CD Pipelines

```yaml
- name: Validate Markdown
  run: |
    mdv docs/schema.mds docs/content.md || exit 1
```

=== Pre-commit Hooks

```bash
#!/bin/sh
mdv .schema.mds "$1" --quiet || exit 1
```

= Troubleshooting

== Schema Not Found

Ensure the schema file path is correct. Use absolute paths if needed:

```bash
mdv /path/to/schema.mds input.md
```

== Input File Issues

If reading from stdin, use `-`:

```bash
cat input.md | mdv schema.mds -
```

== Permission Errors

Ensure you have read permissions for the schema file and input file (or stdin).

== Build Issues

If you encounter build issues:

1. Ensure you have Rust installed (1.70+)
2. For Nix builds, ensure Nix is properly configured
3. Check that all dependencies are available

= Appendix

== Regular Expression Reference

`mdvalidate` uses Perl-compatible regular expressions. Common patterns:

- `\d` - Digit
- `\w` - Word character (letter, digit, underscore)
- `\s` - Whitespace
- `[A-Z]` - Uppercase letter
- `[a-z]` - Lowercase letter
- `+` - One or more
- `*` - Zero or more
- `?` - Zero or one
- `{n,m}` - Between n and m occurrences

== Matcher Syntax Summary

- `` `label:/regex/` `` - Regex matcher
- `` `label:text` `` - Text matcher
- `` `label:number` `` - Number matcher
- `` `label:html` `` - HTML matcher
- `` `ruler` `` - Ruler matcher
- `?` - Optional
- `+` - One or more
- `{n,m}` - Count range
- `dn` - Maximum depth
- `!` - Escape literal
- `_` - Empty label

== Further Resources

- Design Document: See `docs/design_document.typ`
- Software Requirements: See `docs/software_requirements_specification.typ`
- GitHub Repository: https://github.com/404wolf/mdvalidate

