# MDS (Markdown Schema) Language Specification

MDS is a **tiny language for describing how Markdown *should look***. With
`mdvalidate`, you write *schemas* that define what Markdown patterns are
allowed, optional, or repeated,  and MDS checks real documents against them.

It's designed for validating a stream of Markdown via stdin, so you can pipe
LLM output and validate the shape of its response. `mdvalidate` is written in
100% safe rust and is 🔥 blazingly fast 🔥.

---

## ✨ Some examples of what you can match 

- **Literal Matching:** Regular Markdown stays literal — if it says `# Title`,
  it must match exactly.
- **Matchers:** Use `` `label:/regex/` `` to define rules for dynamic content.
- **Optional or Repeated Items:** Add `?` for optional things, `+` for one or
  more.
- **Lists & Sublists:** Validate nested lists with pattern control.
- **Escaping:** Add `!` to disable regex interpretation — great for examples.

---

## Mini Example

Here’s a simple schema that will validate all grocery lists of a specific shape.


```markdown
# Grocery List

- `item:/[A-Z][a-z]+/`+         <!-- one or more items; each starts with a capital letter -->
  - `note:/\w+/`?{,2}           <!-- up to two optional sub-notes per item -->

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

## Render the Typst Design Doc

```bash
typst compile docs/design_document.typ
# or
apndoc docs/design_document.typ -o design_document.md
```

By *Wolf Mermelstein* and *Alessandro Mason*, for Software Engineering class @
Case Western
