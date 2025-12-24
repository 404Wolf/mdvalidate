# Markdown Schema

**Very early stage work in progress!!**

MDS is a **tiny language for describing how Markdown *should look***. With `mdvalidate`, you write *schemas* that define a shape of Markdown, and MDS checks real documents against them.

It's designed for validating a stream of Markdown via stdin, so you can pipe input (like LLM output) and validate the shape of its response.

`mdvalidate` schemas consist of many "matcher" patterns, and all matchers have labels. This means that all validated markdown files can produce a JSON of matches found along the way.

We plan to eventually support converting a Markdown schema into a JSON schema describing the shape of the output that it produces once it has validated some Markdown file.

`mdvalidate` is written in 100% safe rust and is 🔥 blazingly fast 🔥.

---

## Some examples of what you can match 

- **Literal Matching:** Regular Markdown stays literal — if it says `# Title`, it must match exactly.
- **Matchers:** Use `` `label:/regex/` `` to define rules for dynamic content.
- **Optional or Repeated Items:** Add `?` for optional things, `+` for one or more.
- **Lists & Sublists:** Validate nested lists with pattern control.
- **Escaping:** Add `!` to disable regex interpretation — great for examples.

---

## Mini Example

Here’s a simple schema that will validate all grocery lists of a specific shape.


```markdown
# Grocery List

- `item:/[A-Z][a-z]+/`+         <!-- one or more items; each starts with a capital letter -->
  - `note:/\w+/`?{,2}           <!-- up to two optional sub-notes per item -->
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

A failing document (too many sub-notes):

```markdown
# Grocery List

- Apples
  - organic
  - local
  - green
```

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

## Building

You can build `mdvalidate` with the `nix` build system using `nix build github:404wolf/mdvalidate`.

You can build our design document or software requirements specification with typst, using

```bash
typst compile docs/design_document.typ
typst compile docs/software_requirements_specification.typ
```

## Authors

By *Wolf Mermelstein* and *Alessandro Mason*.
