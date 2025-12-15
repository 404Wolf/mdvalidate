#import "template.typ": *

#let raw_block(raw: raw) = {
  block(
    raw, fill: rgb(250, 250, 250), radius: 0.3em, inset: 1.2em, width: 100%,
  )
}

#show: template.with(
  title: [`mdvalidate`: \ Design Document], author: "Wolf Mermelstein and Alessandro Mason",
)

#let schema-block(title, stroke-color, content) = align(
  center, block(
    stroke: stroke-color + 1pt, inset: 8pt, radius: 4pt, fill: rgb(250, 250, 250), breakable: false, width: 70%, align(left, [
      #text(weight: "bold", fill: stroke-color)[#title]
      #content
    ]),
  ),
)

#let schema(content) = schema-block("Schema", gray, content)

#let good-example(content) = schema-block("✓", green, content)

#let bad-example(content, reason: none) = schema-block("✗", red, [
  #content
  #if reason != none [
    #v(0.5em)
    #text(size: 0.9em, style: "italic")[#reason]
  ]
])

= System Architecture

At a high level, `mdvalidate` takes a URI to a schema file (typically a file
path), and a stream of text (typically via stdin, but possibly specified as a
URI as well), and incrementally parses the stream as Markdown using Treesitter,
checking to see if the stream conforms to a specific "shape."

We will be initially building `mdvalidate` in the form of a CLI (Command Line
Interface)

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
indeterminate length via `stdin` (as specified in our Software Requirements
Specification). While we are reading input, `mdvalidate` will _reject_ the input
stream immediately after we reach a point where we can for sure know that the
input will not be able to conform to the provided schema.

To achieve this, we will use
#link("https://tree-sitter.github.io/tree-sitter/", "treesitter"), an
incremental parsing library designed for use in editors like Neovim, Helix, and
Zed. Treesitter lets us very easily start with some specific source code, and
then update its AST dynamically as we learn of more (see
@fig:treesitter-example). Even though treesitter is written in `C`, they provide
a very nice `Rust` API.

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
validate the ASTs that treesitter gives us and check to see if the input AST (so
far) conforms.

The most powerful feature in our language is the `matcher` statement, which is
where most of our core logi will go.

We will define a `Validator` struct which will have a `Validate` receiver.

Since we are going to be making our language also be valid markdown, we will
define a struct called `ZipperTree` that walks two ASTs at the same time.

We will walk down the AST for the schema file at the same rate that we walk the
AST for the input file. For example, at a point in time when we are doing a
validation check, we may have two ASTs that are iterated to this point:

#figure(raw_block(raw: ```
  Schema AST                     Input AST
  -----------                    -----------
  Root                           Root
   |-- Heading                   |-- Heading
   |    |-- Text("Title") <--    |    |-- Text("XTitle") <-- Error!
   |-- Paragraph                 |-- Paragraph
        |-- Text("...")
  ```), caption: [Walking two ASTs side by side])

We will design the `ZipperTree` to use treesitter's #link(
  "https://docs.rs/tree-sitter/latest/tree_sitter/struct.TreeCursor.html", `TreeCursor`,
) struct,

As markdown flows in we will walk both the schema's AST and the input stream (so
far)'s AST at the same "pace."

Note that sometimes one tree's node walking "cursor" will get slightly ahead or
behind the others. All that matters is that semantically they are at the same
"object."

For example, we may enter a situation where there is "code" (the literal
\`\<matcher\>\`) in the Markdown, and we only care about its contents, which in
the AST representation are an inner paragraph (see @fig:might-not-be-same-spot
for an example).

#figure(
  raw_block(
    raw: `````
                                          Schema AST                     Input AST
                                          -----------                    -----------
                                          Root                           Root
                                           |-- Paragraph                 |-- Paragraph
                                           |    |-- Code <--             |    |-- Text() <-- (the schema
                                           |         |-- Text()          |                   does not contain
                                                                                             the codeblock)
                                        `````,
  ), caption: [matcher code blocks (left) contain inner content we care about (right)],
) <fig:might-not-be-same-spot>

To demonstrate our high level model, we present figure @fig:class-diagram, a
high level class diagram for the project. Note that this diagram represents the
core logic organization for our actual validation logic, but does not include
the actual public library or CLI interface.

#figure(
  image("images/classDiagram.png"), caption: [Class diagram for `mdvalidate`],
) <fig:class-diagram>

=== CLI component

To actually use `mdvalidate`, for now you will interface via a CLI interface.

We will name the binary `mdv` (for Markdown_Validate). To use our program,

+ You will provide the name of the input file, or "-" to specify that the input
  will be read via stdin. This file may contain `$schema: <URI to schema>` in the
  Markdown yaml frontmatter, or you can be explicit (2).
+ You can explicitly specify the name of the schema file with
  `--schema=./path/to/schema.md`. (You can use any extension you would like. For
  now we will use `.md` since editors will do syntax highlighting as if it were
  Markdown, and all `mds` is valid Markdown.)
+ You can specify any of the following options for customized output:
  - `--fast-fail` To exit as soon as the input stream does not conform to the
    schema. This means if any error is found during processing, any remaining errors
    in the input stream will go ignored if present.
  - `--json` To get machine-readable JSON output.
  - `-q` or `--quiet` To get NO output. Instead, the program will exit with code
    `1` if the input does not validate, or `0` if it does validate. You will still
    receive output if you fail to specify arguments correctly or for other errors.

We will use Rust's logging library, and by default all logs will get silenced.
If you would like to view logs (e.g. for development), you can use
`RUST_LOG=<log-level> mdv`.

= Language Specification

Here we present the actual specification for the schema definition language,
"Markdown Schema Language," or `mds`.

Before presenting strict definitions of components of the language, it is
important to understand that, by design, our language is _valid markdown_. That
means that any `mds` file can be directly interpreted as `md`.

There were a few considerations that went into this decision:
- Everyone already knows how to write Markdown, and Markdown is designed to be
  extremely easy to read and similar to the end result. What this means for
  `mds` is that if you write a `mds` file, it will look very similar to the thing
  that it will successfully validate.
- There is already a great Treesitter
  #link(
    "https://github.com/tree-sitter-grammars/tree-sitter-markdown", "grammar",
  )
  for Markdown. By having `mds` be valid Markdown, we are able to easily parse it
  with a regular `Markdown` parser without having to define our own (e.g. making a
  custom language with a parser library like #link("https://github.com/zesterer/chumsky", "Chumsky")).

For our language specification below, we provide various important aspects of
the language with detailed text explanation, followed by a `schema`, and
examples of inputs that that schema successfully or unsuccessfully validates.
Then we provide a short rational.

As we evolve, these specifications may change. The core concepts though will
remain the same.

== Literals

By default, regular Markdown is treated as a literal match. What this means is
that all Markdown files validate themselves if you interpret the schema file as
a `mds` file!

#schema(```markdown
# Hi

Test
```)

#bad-example(```markdown
  # Hi
  ```, reason: [This does not match the exact contents of the schema])

#good-example(```markdown
# Hi

Test
```)

== Matchers

`matcher`s are the core component of dynamic Markdown validation rules. To
require that certain content matches a specific pattern, users can define
`matchers`.

A matcher is a snippet delimited by backticks (\`) that consists of a label,
followed by a colon, and then:
- A regular expression (using Pearl-flavored regex)
- The literal "text," which means _anything_.
- The literal "html," which matches all HTML (more on this in @sec:lang-html).

In the future we may add additional matcher symbols.

Labels are used so that you can refer to fields in the output later.

Below are some examples of matchers.

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

#schema(```markdown
`name:text`
```)

#good-example(```markdown
Hello World
```)

#bad-example(```markdown
123
```)

#schema(```markdown
`count:number`
```)

#good-example(```markdown
42
```)

#bad-example(```markdown
not a number
```)

=== Special Matchers: Ruler

The ruler matcher is a special matcher that dosn't require an id and will match
a standard markdown ruler line.

#schema(```markdown
`ruler`
```)

#good-example(```markdown
---
```)
#good-example(```markdown
***
```)
#good-example(```markdown
___
```)

#bad-example(```markdown
ruler
```)

== HTML <sec:lang-html>

Markdown is a superset of HTML. HTML is a subset of XML. For example, the
following is perfectly valid Markdown.

#raw_block(raw: ```markdown
<image src="./hello.png" />
```)

If you would like to specify _any type of HTML_, you can use the matcher pattern

#schema(```markdown
`some_html:html`
```)

#good-example(```markdown
<image src="./hello.png" />
```)

#bad-example(```markdown
this is text
```)

Yes, plain text not in a tag is technically RFC-valid HTML, but we will only
consider `html` to match text _inside_ tags. This includes arbitrarily deep
tags.

#good-example(```markdown
<div>
  <image src="./hello.png" />
</div>
```)

But you can specify the depth with the suffix `dn`.

#schema(```markdown
`some_html:html`d2
```)

#good-example(```markdown
<div>
  <image src="./hello.png" />
</div>
```)

#bad-example(```markdown
  <div>
    <div>
      <div>
        <image src="./hello.png" />
      </div>
    </div>
  </div>
  ```, reason: [Too deep])

To be more specific, you can use #link("https://www.w3.org/TR/xmlschema11-1/", "W3 XML schema language").

To do so, in \`\`\`, use "mds"

#schema(````markdown
```mds
<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="h1" type="xs:string"/>
</xs:schema>
```
````)

#good-example(```markdown
<h1> Hello, world! </h1>
```)

#bad-example(```markdown
  <h2> Hello, world! </h2>
  ```, reason: [Doesn't match XML schema specified])

XML schema is "hard" to read and more complicated, so we advise in all cases to
prefer Markdown equivalents.

Additionally, Markdown will match to its HTML equivalent. So this means that the
following works:

#schema(```markdown
# Hi

Text
```)

#good-example(```markdown
<h1> Hi </h1>

Text
```)

=== Comments

Markdown supports HTML style comments, like

#raw_block(raw: ```markdown
Some markdown content.

<!-- This is a single-line comment. -->

Some more markdown content.
```)

There are also more complicated tactics to add comments to Markdown. For
example, with this syntax

#raw_block(raw: ```markdown
[This is a comment that will be hidden.]: #
```)

In general, we will not do schema enforcement for comments. This is an
intentional limitation of our schema declaration language. When doing automatic
processing of Markdown it is not important to understand "human" comments.

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

Lists are defined as a `matcher`, with a few special suffix options.

- An *_enumeration_ suffix*, `{from,to}`, similar to regex. If you want to specify
  the number of repeats of occurances of the `matcher` pattern, use
  `<matcher statement>{from, to}` to specify the number of occurances of that list
  item.
- A *depth marker*, `dn`, to specify the maximum depth of sublists where each
  sublist conforms to the matcher pattern.

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
  ```, reason: [Too many items])

#bad-example(```markdown
  + Hello 12
  + Hello 22
  ```, reason: [Numbered list when bulleted list expected])

In a similar way is possible to define sublists. Use typical Markdown
indentation for sublists, and the list prefix character, along with the same
rules as for lists.

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

To specify the depth of sub-lists, you can add "d\<number\>" to your suffix
string.

#schema(```markdown
  - `grocery_list_item:/Hello \w+/`+d2
```)

#good-example(```markdown
- Hello 12
  - Hello 1
    - Hello deep item
- Hello 44
  - Hello Subitem 2
```)

#bad-example(```markdown
  - Hello 12
    - Hello Subitem 1
      - Hello Deep item
        - Hello Too deep!
  ```, reason: [Exceeds maximum depth of 2])

#bad-example(```markdown
  - Hello 12
    - Hello Subitem 1
      - This doesn't start with hello
  ```, reason: [A sublist doesn't match the pattern of the `matcher`])

== Optional Matcher Pattern

To make a `matcher` optional the user can add a question mark (`?`) at the end
of it.

#schema(```markdown
- `grocery_list_item:/Hello \w+/`+?
```)

#good-example(```markdown
- Hello 12
- Hello 1
- Hello 233
```)

#good-example(```markdown
```)

#schema(```markdown
`optional_title:text`?
```)

#good-example(```markdown
A Title
```)

#good-example(```markdown
```)

#schema(```markdown
# Required Heading

`optional_content:text`?
```)

#good-example(```markdown
# Required Heading

Some optional content here
```)

#good-example(```markdown
# Required Heading
```)

#bad-example(```markdown
  Some optional content here
  ```, reason: [Missing required heading])

= Glossary

- *MDS*: Markdown Schema Language. This is the language you use to define the
  shape of Markdown, which our tool can read and then validate against.
- *CLI*: Command line interface. A program that is interacted with via terminal
  stdio.
- *MDV*: Markdown Validate (the name of our program).

= History of Changes

*(Inspection Report)*

- 10/6, Wolf Mermelstein, Finish language specification, continue polishing
  overview section
- 10/5, Alessandro Mason, Add examples for optional matchers and list depth
  specifications
- 10/5, Wolf Mermelstein, Create class diagram and refine system architecture
  section
- 10/4, Alessandro Mason, Add language specification section and add important
  language features
- 10/3, Wolf Mermelstein, Document incremental parsing logic and TreeSitter
  integration
- 10/3, Alessandro Mason, Define matcher syntax and validation rules
- 10/2, Wolf Mermelstein, Design CLI component and command-line interface
  specifications
- 10/2, Wolf Mermelstein and Alessandro Mason, Begin working on Design Document
- 10/1, Wolf Mermelstein, Draft initial system architecture overview on whiteboard
- 10/1, Wolf Mermelstein, Research TreeSitter capabilities and Rust API
