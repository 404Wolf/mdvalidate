#import "template.typ": *

#let raw_block(raw: raw) = {
  set text(font: ("Iosevka", "Fira Mono"), size: 9pt)
  block(
    raw, fill: rgb(250, 250, 250), radius: 0.3em, inset: 1.2em, width: 100%,
  )
}

#show: template.with(
  title: "Design Document", author: "Wolf Mermelstein and Alessandro Mason",
)

#let schema(content) = block(
  stroke: gray + 1pt, inset: 8pt, radius: 4pt, fill: rgb(250, 250, 250) ,content,
)

#let good-example(content) = block(
  stroke: green + 1pt, inset: 8pt, radius: 4pt, fill: rgb(250, 250, 250), content,
)

#let bad-example(content) = block(
  stroke: red + 1pt, inset: 8pt, radius: 4pt, fill: rgb(250, 250, 250), content,
)

= System Architecture

At a high level, `mdvalidate` takes a URI to a schema file (typically a file
path), and a stream of text (typically via stdin, but possibly specified as a
URI as well), and incrementally parses the stream as Markdown using Treesitter,
checking to see if the stream conforms to a specific "shape."

By design, the `mdvalidate` language "looks" like Markdown. This is not a
coincidence -- `mds` itself is valid Markdown so that we can let users describe
the shape of their documents and outputs in a way that is familiar, use tools
that they already are familiar with. Additionally, by using Markdown to define
the shape of Markdown, we are able to better make use of existing language
parsing tooling.

To actually create `mdvalidate`, we have broken down the project into a few
components:

== Incremental parsing logic

Many times when we use `mdvalidate`, we will be reading a stream of
indeterminate length via `stdin`. While we are reading input, `mdvalidate` will
_reject_ the input stream immediately after we reach a point where we can for
sure know that the input will not be able to conform to the provided schema.

To achieve this, we will use
#link("https://tree-sitter.github.io/tree-sitter/", "treesitter"), an
incremental parsing library designed for use in editors like Neovim, Helix, and
Zed. Treesitter lets us very easily start with some specific source code, and
then update its AST dynamically as we learn of more (see
@fig:treesitter-example). Even though treesitter is written in `C`, they
provide a very nice `Rust` API.

#figure(raw_block(raw: ```rs
  let mut parser = Parser::new();
  parser.set_language(tree_sitter_markdown::language()).unwrap();

  let mut source = String::from("# Hello");
  let mut tree = parser.parse(&source, None).unwrap();

  let edit = InputEdit {
      start_byte: byte,
      old_end_byte: byte,
      new_end_byte: byte + insert_text.len(),
      start_position: Point::new(0, byte),
      old_end_position: Point::new(0, byte),
      new_end_position: Point::new(0, byte + insert_text.len()),
  };
  tree.edit(&edit);

  let tree = parser.parse(&source, Some(&tree)).unwrap();
  ```), caption: "Incremental parsing with treesitter") <fig:treesitter-example>

=== Schema Enforcement

To actually provide the core function of `mdvalidate` we need to be able to
validate the ASTs that treesitter gives us and check to see if the input AST
(so far) conforms.

The most powerful feature in our language is the `matcher` statement, which is
where most of our core logi will go.

We will define a `Validator` struct which will have a `Validate` receiver.

Since we are going to be making our language also be valid markdown, we will
define a struct called `BiWalker` that walks two ASTs at the same time. We
will walk down the AST for the schema file at the same rate that we walk the
AST for the input file. For example, at a point in time when we are doing a
validation check, we may have two ASTs that are iterated to this point:

#figure(
  raw_block(raw: ```
Schema AST                     Input AST
-----------                    -----------
Root                           Root
 |-- Heading                   |-- Heading
 |    |-- Text("Title") <--    |    |-- Text("XTitle") <-- Error!
 |-- Paragraph                 |-- Paragraph
      |-- Text("...")
```
  ), caption: [Walking two ASTs side by side],
)

We will design the `BiWalker` to use treesitter's #link("https://docs.rs/tree-sitter/latest/tree_sitter/struct.TreeCursor.html", `TreeCursor`) struct, 

As markdown flows in we will walk both the schema's AST and the input stream
(so far)'s AST at the same "pace."

= Language Specification

== Literals

By default, regular Markdown is treated as a literal match.

#schema(```markdown
  # Hi

  Test
  ```)

#bad-example(```markdown
  # HHi
  ```)

#bad-example(```markdown
  # Hi

  Test
  ```)

#good-example(```markdown
  # Hii
  ```)

But Markdown that has a sequence that begins with a \` character, like

```markdown
# Hi

`test:/4/`
```

We only match on to the literal Markdown

#bad-example(```markdown
  # Hi

  // Something that matches `test:/4/`
  ```)

= Glossary

= History of Changes
