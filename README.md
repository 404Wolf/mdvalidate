# Welcome to `mdvalidate`

**Mdvalidat is an early stage work in progress!!**

MDS is a **tiny language for describing how Markdown *should look***. With `mdvalidate`, you write *schemas* that define a shape of Markdown, and MDS checks real documents against them.

It's designed for validating a stream of Markdown via stdin, so you can pipe input (like LLM output) and validate the shape of its response.

`mdvalidate` schemas consist of many "matcher" patterns, and all matchers have labels. This means that all validated markdown files can produce a JSON of matches found along the way.

We plan to eventually support converting a Markdown schema into a JSON schema describing the shape of the output that it produces once it has validated some Markdown file.

`mdvalidate` is written in 100% safe rust and is 🔥 blazingly fast 🔥.

You can find the full docs [here](https://404wolf.github.io/mdvalidate/)!

## Kitchen Sink Example

Schema:

````md
# Release Notes `version:/v\d+\.\d+\.\d+/`

> Build `build:/[A-F0-9]{7}/` by `_:/\w+/`

## Highlights

- `feature:/[A-Za-z][\w -]+/`{2,4}
  - `detail:/[a-z][\w -]+/`{,2}

## Description

`description`{2,3}

Inline: `code`! and `bang`!!

```{lang:/\w+/}
{snippet}
```

```{runtime:/\w+/}
{checked:!python -m py_compile -}
```

`html_block:html`d2

| Key | Value |
| :-- | :---- |
| `key:/\w+/` | `value:/.+/` |
| `key:/\w+/` | `value:/.+/` |{1,3}

```mds
<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="h1" type="xs:string"/>
</xs:schema>
```
````

Input:

````md
# Release Notes v1.4.2

> Build 7A9F3C1 by wolf

## Highlights

- Fast paths
  - fewer allocations
- Safer IO

## Description

This release focuses on performance improvements and safety enhancements.

Key changes include optimized memory management and stricter type checking.

We've also improved error messages throughout the codebase.

Inline: `code` and `bang`!

```rust
fn main() {}
```

```python
print("ok")
```

<div>
  <img src="./logo.png" />
</div>

| Key | Value |
| :-- | :---- |
| host | localhost |
| port | 8080 |

<h1>Hello</h1>
````

Output:

```json
{
  "build": "7A9F3C1",
  "checked": "print(\"ok\")",
  "description": [
    "This release focuses on performance improvements and safety enhancements.",
    "Key changes include optimized memory management and stricter type checking.",
    "We've also improved error messages throughout the codebase."
  ],
  "detail": [
    "fewer allocations"
  ],
  "feature": [
    "Fast paths",
    "Safer IO"
  ],
  "html_block": "<div>\n  <img src=\"./logo.png\" />\n</div>",
  "key": [
    "host",
    "port"
  ],
  "lang": "rust",
  "runtime": "python",
  "snippet": "fn main() {}",
  "value": [
    "localhost",
    "8080"
  ],
  "version": "v1.4.2"
}
```

Notes:
- Planned but not implemented yet: `html` matcher, table row repetition, `mds` XML schema blocks, execution validation.

## Mini Example

Here’s a simple schema that will validate all grocery lists of a specific shape.


```markdown
# Grocery List

- `item:/[A-Z][a-z]+/`{2,2}
  - `note:/\w+/`{,2}
```

A passing document:

```markdown
# Grocery List

- Apples
  - organic
  - local
- Bananas
  - ripe
```

A failing document (too few sub-notes):

```markdown
# Grocery List

- Apples
  - organic
  - local
```

## Some examples of what you can match 

- **Literal Matching:** By default -- if it says `# Title`, it must match exactly.
- **Matchers:** Use `` `label:/regex/` `` to define rules for dynamic content.
- **Optional or Repeated Items:** Add `?` for optional things, `+` for one or more.
- **Lists & Sublists:** Validate nested lists with pattern control.
- **Escaping:** Add `!` to disable regex interpretation -- great for examples.

---

## Crazy cool recursive schema declaration!

Here's a fun example of a schema that validates multiple list levels and collects labeled matches very deeply!

```markdown
- `test:/test\d/`{2,2}
- `barbar:/barbar\d/`{2,2}
    + `deep:/deep\d/`{1,1}
        - `deeper:/deeper\d/`{2,2}
        - `deepest:/deepest\d/`{2,}
```

A passing document:

```markdown
- test1
- test2
- barbar1
- barbar2
    + deep1
        - deeper1
        - deeper2
        - deepest1
        - deepest2
        - deepest3
        - deepest4
```

The captured matches:

```json
{
  "barbar": [
    "barbar1",
    "barbar2",
    {
      "deep": [
        "deep1",
        {
          "deeper": [
            "deeper1",
            "deeper2"
          ],
          "deepest": [
            "deepest1",
            "deepest2",
            "deepest3",
            "deepest4"
          ]
        }
      ]
    }
  ],
  "test": [
    "test1",
    "test2"
  ]
}
```

We're validating:

- All of the actual list groups, making sure the regex passes
- The number of list items for each group
- And capturing it all into a structured output object!

---

## Get started!

### Installation

You can build `mdvalidate` with `nix` using `nix build github:404wolf/mdvalidate`.

Alternatively download a pre-built (static) binary from [releases](https://github.com/404Wolf/mdvalidate/releases) for use on x86 or Mac (apple silicon).

It is not officially supported, but you can also build directly with cargo via `cargo build --bin mdv`.

### Using mdvalidate

`mdvalidate` defines a very simple language for describing the *shape* of Markdown documents that looks like Markdown itself. You use `mdvalidate` via a command line tool (CLI).

In every case, you have a schema, in `mdschema`, Mdvalidate's schema definition language, and an input, which may or may not conform to the schema. You can invoke `mdvalidate` by running:

```bash
mdv path/to/schema.md path/to/input.md
echo $?
```

Which returns `0` if the validation is successful or `1` if there were errors. Errors are reported to `stderr`.

You can use `-` instead of a path to use `stdio`. If you include a third positional argument, it will also extract data from documents that conform to the schema. For example,

```bash
echo "# Hi Wolf" | mdv path/to/schema.md - -
echo $?
```

For the schema

```md
# Hi `name:/[A-Za-z]+/`
```

Will return

```
mdv examples/cli/schema.md examples/cli/input.md - 
{"name":"Wolf"}
0
```
