#import "template.typ": *

#show: template.with(
  title: "Design Document", author: "Wolf Mermelstein and Alessandro Mason",
)

#let schema(content) = block(
  stroke: gray + 1pt, inset: 8pt, radius: 4pt, fill: rgb(250, 250, 250), breakable: false, content,
)

#let good-example(content) = block(
  stroke: green + 1pt, inset: 8pt, radius: 4pt, fill: rgb(250, 250, 250), breakable: false, content,
)

#let bad-example(content) = block(
  stroke: red + 1pt, inset: 8pt, radius: 4pt, fill: rgb(250, 250, 250), breakable: false, content,
)

= System Architecture

= MDS (MarkDown Schema) Language Specification

== Literals

By default, regular Markdown is treated as a literal match.

#schema(```markdown
  # Hi

  Test
  ```)

#bad-example(```markdown
  # Hi
  ```)

#bad-example(```markdown
  # hi

  Test
  ```)

#good-example(```markdown
  # Hi

  Test
  ```)

== `Matchers`

To require that certain content matches a specific pattern, users can define `matchers`.
A matcher is a snippet delimited by backticks (\`) that consists of a label,
followed by a colon, and then a regular expression. This defines a pattern that
the corresponding Markdown content must match.
#schema(```markdown
`test:/4/`
```)

#bad-example(```markdown
  `test:/4/`
  ```)

#good-example(```markdown
  4
  ```)
#bad-example(```markdown
  `/4/`
  ```)
=== Empty Labels

To specify an empty label, use an underscore (`_`) as the label.

#good-example(```markdown
  `_:/4/`
  ```)
#bad-example(```markdown
  `/4/`
  ```)
#bad-example(```markdown
  `:/4/`
  ```)

=== Escaping `Matchers`

To force a section to be treated literally (instead of using regex), add an
exclamation mark (`!`) at the end of the section. This tells the system to treat
the content as a literal string.

#schema(```markdown
`test:/4/`!
```)

#bad-example(```markdown
  4
  ```)

#good-example(```markdown
  `test:/4/`
  ```)

If the user wants to have a literal exclamation mark at the end of a `matcher`,
they should add a second exclamation mark (i.e., use `!!` at the end).
#schema(```markdown
`test:/4/`!!
```)

#good-example(```markdown
  `test:/4/`!
  ```)

== Lists

To define a list, users should start each list with the `-` character, followed
by either a literal value or a `matcher`. To indicate the number of items in the
list, add a `+` after the `matcher` for multiple items or use curly braces with
the desired range.
#schema(```markdown
- `grocery_list_item:/Hello \w+/`+
```)

#good-example(```markdown
  - Hello 12
  - Hello 1
  - Hello 233
  ```)
#schema(```markdown
- `grocery_list_item:/Hello \w+/`{1,2}
```)

#bad-example(```markdown
  - Hello 12
  - Hello 1
  - Hello 233
  ```)
#bad-example(```markdown
  ```)

=== Sublists

In a similar way is possible to define sublists, by using and indented `-`character
and the same rules as for lists.
#schema(```markdown
  - `grocery_list_item:/Hello \w+/`+
    - `grocery_item_notes:text`?{,3}
```)

#good-example(```markdown
  - Hello 12
    - text
    - text
  - Hello 44
    -text
  ```)
#bad-example(```markdown
  - Hello 12
    - text
    - text
    - text
    - text
  ```)

== Optional specifications
To make a `matcher` optional the user can appenda questionmark (`?`) at the end
of it.

#schema(```markdown
- `grocery_list_item:/Hello \w+/`+?
```)

#good-example(```markdown
  - Hello 12
  - Hello 1
  - Hello 233

  ```)#good-example(```markdown

  ```)

= Glossary

= History of Changes