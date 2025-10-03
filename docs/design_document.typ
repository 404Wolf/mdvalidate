#import "template.typ": *

#show: template.with(
  title: "Design Document",
  author: "Wolf Mermelstein and Alessandro Mason",
)

#let schema(content) = block(
  stroke: gray + 1pt,
  inset: 8pt,
  radius: 4pt,
  fill: rgb(250, 250, 250),
  content,
)

#let good-example(content) = block(
  stroke: green + 1pt,
  inset: 8pt,
  radius: 4pt,
  fill: rgb(250, 250, 250),
  content,
)

#let bad-example(content) = block(
  stroke: red + 1pt,
  inset: 8pt,
  radius: 4pt,
  fill: rgb(250, 250, 250),
  content,
)

= System Architecture

= Language Specification

== Literals

By default, regular Markdown is treated as a literal match.

#schema(
  ```markdown
  # Hi

  Test
  ```,
)

#bad-example(
  ```markdown
  # HHi
  ```,
)

#bad-example(
  ```markdown
  # Hi

  Test
  ```,
)

#good-example(
  ```markdown
  # Hii
  ```,
)

But Markdown that has a sequence that begins with a \` character, like 

```markdown
# Hi

`test:/4/`
```

We only match on to the literal Markdown

#bad-example(
  ```markdown
  # Hi

  // Something that matches `test:/4/`
  ```,
)

= Glossary

= History of Changes