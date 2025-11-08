// For this assignment your team should produce a functional testing document.
// After introductory material (descriptions of the product to be tested and the
// testing environment, a record of changes, special considerations. etc.), the
// bulk of the document should consist of functional test case descriptions
// based on requirements in your SRS.  Your document can be modeled on any of
// the example testing documents or formats posted on Canvas.  It will be graded
// on its completeness, clarity, organization, and "professional" appearance.
// (If you choose to use the tabular format in the Test-Cases-for-OrangeHRM
// (Excel) document, you will need to add introductory material.)

// See also
// https://medium.com/@iamfaisalkhatri/best-examples-of-functional-test-cases-agilitest-blog-424260298b5


// exercises all the srs requirements at least once

// - if its wrong in the reuqirements docs prob wrong in requirements doc.
// - could be right in the srs but wrong in the implementation.
// - requirement could be omitted
// - tricky if its missed in both

// can be applied to any component of the system.



#import "template.typ": *

#let schema(content) = block(
  stroke: gray + 1pt,
  inset: 2pt,
  radius: 4pt,
  fill: rgb(250, 250, 250),
  breakable: false,
  content,
)

#let good-example(content) = block(
  stroke: green + 1pt,
  inset: 2pt,
  radius: 4pt,
  fill: rgb(250, 250, 250),
  breakable: false,
  content,
)

#let bad-example(content) = block(
  stroke: red + 1pt,
  inset: 2pt,
  radius: 4pt,
  fill: rgb(250, 250, 250),
  breakable: false,
  content,
)

#show: template.with(
  title: "Functional Test Case Description",
  author: "Wolf Mermelstein and Alessandro Mason",
)

= Introduction

== Product Description

`mdvalidate` ia a tool that allows the user to validate the shape of a Markdown by
declaring a schema and an input Markdown file.

== Testing Environment

x86_64 Linux github action test runner in a Nix development environment

We are developing in a Nix development shell defined by the following (subject to change)

```nix
devShells.default = pkgs.mkShell {
  packages = (
    with pkgs;
    [
      perf
      nil
      nixd
      nixfmt
      typst
      cargo
      rustc
      mermaid-cli
      rust-analyzer
      fira-mono
    ]
  );
  shellHook = ''
    export PATH=$PATH:target/debug
    export LLVM_COV=${pkgs.llvmPackages_latest.llvm}/bin/llvm-cov
    export LLVM_PROFDATA=${pkgs.llvmPackages_latest.llvm}/bin/llvm-profdata
  '';
};
```

We also properly build our program for production via the following `nix` builder

```nix
{ lib, rustPlatform }:
rustPlatform.buildRustPackage {
  pname = "mdvalidate";
  version = "0.1.0";

  src = ../.;

  cargoHash = "sha256-cujUmddyLvt0gMNYFXug9jDN+D6QUyzYQ542mVEYYnE=";

  meta = {
    description = "Markdown Schema validator";
    homepage = "https://github.com/404Wolf/mdvalidate";
    license = lib.licenses.mit;
  };
}
```


We are using Rust's
#link("https://doc.rust-lang.org/rust-by-example/testing/unit_testing.html", "standard practice") of defining a test module, with test functions like
`#[test]` via their #link("https://doc.rust-lang.org/beta/test/index.html", "testing framework"), which we run with `cargo test`.

For completeness, we provide our full `Cargo.toml`, but highly encourage that if
you decide to build our project that you make use of our `Cargo.lock` to ensure
reformability.

```toml
[package]
name = "mdv"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow = "1.0.100"
ariadne = "0.5.1"
clap = { version = "4.5.48", features = ["derive"] }
colored = "3.0.0"
env_logger = "0.10.0"
line-col = "0.2.1"
log = "0.4.28"
regex = "1.12.2"
tree-sitter = "0.25.10"
tree-sitter-markdown = { git = "https://github.com/404wolf/tree-sitter-markdown.git" }

[build-dependencies]
cc="*"

[dev-dependencies]
cargo-llvm-cov = "0.6.21"
flamegraph = "0.6.9"
```

== Record of Changes

Initially we planned to have multiple matchers in the same line, for example

```markdown
# Hi `matcher1:/test+/` t `matcher2:/test+/`
```

However, since this would create ambiguity we decided to not support it (and
added a corresponding test case expecting it to fail).

= Features to be Tested

For all the following test cases the steps are:

+ Create schema with regex matcher
+ Validate against matching input

=== Literal Matching


#table(
  columns: (1fr, 1fr, 1.5fr),
  align: (left, left, left),
  table.header([*Test Case*], [*Test Data*], [*Result*]),

  [Two identical text nodes],
  [#schema(```markdown
    Hello, world!
    ```)
    #good-example(```markdown
    Hello, world!
    ```)],
  [Validation passes with no errors, indices match],

  [Two different text nodes],
  [#schema(```markdown
    Hello, everyone!
    ```)
    #bad-example(```markdown
    Hello, world!
    ```)],
  [Validation fails with "Literal mismatch: expected \"Hello, everyone!\", found \"Hello, world!\""],

  [Two paragraph nodes with same text],
  [#schema(```markdown
    This is a paragraph.
    ```)
    #good-example(```markdown
    This is a paragraph.
    ```)],
  [Validation passes with no errors, indices match],

  [H1 heading with paragraph (same text)],
  [#schema(```markdown
    # Heading

    This is a paragraph.
    ```)
    #good-example(```markdown
    # Heading

    This is a paragraph.
    ```)],
  [Validation passes with no errors, indices match],

  [Different heading levels (H1 vs H2)],
  [#schema(```markdown
    ## Heading
    ```)
    #bad-example(```markdown
    # Heading
    ```)],
  [Validation fails with "Node mismatch" error],

  [Not at EOF - final characters mismatch],
  [#schema(```markdown
    # Test
    Hello, world
    ```)
    #bad-example(```markdown
    # Test
    Hello, wor
    ```)],
  [With eof: false passes, with eof: true fails due to incomplete content],

  [Mismatched content structure],
  [#schema(```markdown
    # Test

    fooobar

    test
    ```)
    #bad-example(```markdown
    fooobar

    testt
    ```)],
  [Validation fails with mismatch error (missing heading, different text)],
)


=== Schema Definition Language

#set text(size: 9pt)

#table(
  columns: (1fr, 1fr, 1.5fr),
  align: (left, left, left),
  table.header([*Test Case*], [*Test Data*], [*Result*]),
  [Valid regex matcher in inline code],
  [ #schema(```markdown
    `id:/test/`
    ```)
    #good-example(```markdown
    test
    ```)],
  [Validation passes with no errors],

  [Invalid regex matcher - pattern mismatch],
  [#schema(```markdown
    `id:/test/`
    ```)
    #bad-example(```markdown
    testttt
    ```)],
  [Validation fails with "Matcher mismatch: input 'testttt' does not" error],

  [Multiple matchers in single node],
  [#schema(```markdown
    `id:/test/` `id:/example/`
    ```)
    #bad-example(```markdown
    test example
    ```)],
  [Validation fails with "Multiple matchers in a single node are not supported" error],

  [List item with regex matcher],
  [#schema(```markdown
    - `id:/item\d/`
    - `id:/item2/`
    ```)
    #good-example(```markdown
    - item1
    - item2
    ```)],
  [Validation passes for matching list items],

  [Mismatched node types (list vs paragraph)],
  [#schema(```markdown
    `id:/item1/`
    - `id:/item3/`
    ```)
    #bad-example(```markdown
    - item1
    - item2
    ```)],
  [Validation fails with "Node mismatch" error],

  [Mismatched list item content],
  [#schema(```markdown
    - `id:/item1/`
    - `id:/item3/`
    ```)
    #bad-example(```markdown
    - item1
    - item2
    ```)],
  [Validation fails with "Matcher mismatch: input 'item2' does not" error],

  [Different list types (ordered vs unordered)],
  [#schema(```markdown
    1. `id:/item1/`
    2. `id:/item2/`
    ```)
    #bad-example(```markdown
    - item1
    - item2
    ```)],
  [Validation fails with "Node mismatch" error],

  [Bad schema file],
  [#schema(```markdown
  # `
  ```)],
  [The validation does not even begin because the schema is invalid],

  [Bad input file],
  [#schema(```markdown
  # `
  ```)],
  [The validation does not even begin because the input is invalid],
)

We will also have a small test to make sure that we can generate JSON output based on labels:

#schema(
  ```markdown
  # Hello
  `test:/\d+/`
  ```
)

#good-example(
  ```markdown
  # Hello
  123
  ```
)

Should produce the following JSON:

```json
{
  "test": "123"
}
```

As an end to end test using the CLI. All of the above tests in our table will
also have their output key-value verified.

#pagebreak()


== End to End CLI testing

We will also have a overall end to end test to actually ensure the functionality of the command line tool. This will take the form of a simple test script that executes `mdv` on a schema file and conforming input file:

#schema(
  ```markdown
  # CSDS 999 Assignment `assignment_number:\d`

  # `title:(([A-Z][a-z]+ )|and |the )+([A-Z][a-z]+)`

  ## `first_name:[A-Z][a-z]+`
  ## `last_name:[A-Z][a-z]+`

  Example code:

  `m!foo = 12`!

  This is a shopping list:

  - `grocery_list_item:/Hello \w+/`+
      - `grocery_item_notes:text`?{,3}
  ```,
)

#good-example(```markdown
# CSDS 999 Assignment 5

# Test Test

## Wolf
## Alessandro

Example code:

`m!foo = 12`

This is a shopping list:

- Eggs
    - Avocados
```)

We will run the command with:
- `mdv schema.md input.md` (where `schema.md` is the schema file, shown above,
  and `input.md` the input file, also shown above).
- `mdv foobar` (making sure that the program exits with code `1` and terminates).

We will also test it with this input file and the above schema:

#bad-example(```markdown
# CSDS 999 Assignment 5

# Test Test

## Wolf
## Alessandro

Example code:

`m!foo = 12`

This is a shopping list:

- Eggs
+ Avocados
```)

Making sure there is an error on the last line (since the list type is
mismatched), making sure the error is accurate. We will test it on an empty
Markdown file and also assert failure.

Finally, we will have one end-to-end streaming example. In this example we will
use the following nodejs program to pipe input to `mdvalidate` via standard in,
which we will validate.

```js
const data = "# Hi there\n";
let i = 0;

function writeNext() {
  if (i < data.length) {
    const chunk = data.slice(i, i + 2);
    process.stdout.write(chunk);
    console.error(chunk);
    i += 2;
    setTimeout(writeNext, 250);
  }
}

writeNext();
```

And we will also test to make sure that the errors are accurate, and for valid
and invalid input, a broken input, and with a broken schema.


== EOF Handling Tests

EOF tests are to make sure that streaming works properly. Streaming is an
important core feature of `mdvalidate` that lets us read Markdown input
incrementally. In order to make streaming possible we need to be able to read
input that is "partial" --- that does not include an EOF.

#table(
  columns: (1fr, 1fr, 1.5fr),
  align: (left, left, left),
  table.header([*Test Case*], [*Test Data*], [*Result*]),

  [Initial validate with EOF works],
  [#schema(```markdown
    Hello World
    ```)
    #good-example(```markdown
    Hello World
    ```)],
  [Validation passes with no errors when EOF is true],

  [Initial validate without EOF - incomplete text],
  [#schema(```markdown
    Hello World
    ```)
    #good-example(```markdown
    Hello Wo
    ```)],
  [Validation passes with no errors when EOF is false (incomplete input allowed)],

  [Initially empty then read input],
  [#schema(```markdown
    Hello

    World
    ```)
    Initial: ```markdown

    ```
    Updated: ```markdown
    Hello

    TEST World
    ```],
  [Empty input passes, then after reading "Hello\n\nTEST World" validation fails],

  [Validate, read input, validate again],
  [#schema(```markdown
    Hello World
    ```)
    Initial (EOF: false): ```markdown
    Hello Wo
    ```
    Updated (EOF: true): ```markdown
    Hello World
    ```],
  [First validation passes with incomplete input, second validation passes with complete input],

  [Validation fails with mismatched content],
  [#schema(```markdown
    # Test

    fooobar

    test
    ```)
    #bad-example(```markdown
    # Test

    fooobar

    testt
    ```)],
  [Validation fails due to text mismatch ("test" vs "testt")],

  [Validation passes with different whitespace],
  [#schema(```markdown
    # Test

    fooobar

    test
    ```)
    #good-example(```markdown
    # Test


    fooobar



    test

    ```)],
  [Validation passes - extra whitespace is ignored],

  [Validation fails with escaped newlines],
  [#schema(```markdown
    # Test

    fooobar

    test
    ```)
    #bad-example(```markdown
    # Test

    fooobar

    testt
    ```)],
  [Validation fails due to text mismatch with escaped newlines],
)

== Command Line Streaming

// Table 1: Reader/IO Tests
#table(
  columns: (1fr, 1fr, 1.5fr),
  align: (left, left, left),
  table.header([*Test Case*], [*Test Data*], [*Result*]),

  [Limited reader actually limits bytes],
  [Input: ```markdown
    Hello, world! This is a longer string.
    ```
    Max bytes per read: 3],
  [First read returns 3 bytes ("Hel"), subsequent reads return 3 bytes each until EOF],

  [Validate with cursor (basic)],
  [#schema(```markdown
    # Hi there!
    ```)
    #good-example(```markdown
    # Hi there!
    ```)],
  [Validation passes with cursor reader],

  [Validate with two-byte reads],
  [#schema(```markdown
    # Hi there!
    ```)
    #good-example(```markdown
    # Hi there!
    ```)
    Max bytes per read: 2],
  [Validation completes successfully with limited reader (2 bytes at a time)],

  [Validate with thousand-byte reads],
  [#schema(```markdown
    # Hi there!
    ```)
    #good-example(```markdown
    # Hi there!
    ```)
    Max bytes per read: 1000],
  [Validation completes successfully with large buffer reader (1000 bytes at a time)],
)


#pagebreak()

== Language Server (LSP) Integration Tests

We haven't implemented an LSP for `mdvalidate` yet, but when we do, we will
expect to incorporate the following tests:

#table(
  columns: (1fr, 2fr),
  align: (left, left),
  table.header([*LSP Method*], [*Test Description*]),

  [`textDocument/didOpen`],
  [
    Server receives document open notification and begins validation. Initial diagnostics are computed and published for the opened document.

    - Opening a blank file
    - Opening a file with just one line of valid Markdown
    - Opening a file with just one line of invalid Markdown
    - Opening and closing a file rapidly (sending many `textDocument/didOpen`s for the same file in quick succession).
  ],

  [`textDocument/didChange`],
  [
    Server receives incremental changes to document content. Validation is re-run on each change and updated diagnostics are published. Tests include adding characters incrementally, deleting content, and making multi-line edits.

    - User is typing at the top of the file letter by letter
    - User is typing at the bottom of the file letter by letter
    - User is typing at the middle of the file letter by letter
    - User deletes a chunk of text all of a sudden and the remaining text is invalid Markdown (*does not* conform to schema but also just is bad Markdown)
    - User deletes a chunk of text all of a sudden and the remaining text is valid Markdown (*does not* conform to schema but is bad Markdown)
    - User deletes a chunk of text all of a sudden and the remaining text is valid Markdown (*does* conform to schema but is bad Markdown)
    - User adds a chunk of text all of a sudden and the remaining text is invalid Markdown (*does not* conform to schema but also just is bad Markdown)
    - User adds a chunk of text all of a sudden and the remaining text is valid Markdown (*does not* conform to schema but is bad Markdown)
    - User adds a chunk of text all of a sudden and the remaining text is valid Markdown (*does* conform to schema but is bad Markdown)
  ],

  [`textDocument/didClose`],
  [
    Server receives document close notification and cleans up resources. Diagnostics are cleared for the closed document.

    - User closes a file that has invalid Markdown (*does not* conform to schema but also just is bad Markdown)
    - User closes a file that has valid Markdown (*does not* conform to schema but is bad Markdown)
    - User closes a file that has valid Markdown (*does* conform to schema but is bad Markdown)
    - User tries to close a file that has already been closed
    - User tries to close a file that has never even been opened
  ],

  [`textDocument/publishDiagnostics`],
  [
    Server publishes diagnostics (errors/warnings) to client. Tests verify correct error positions, messages, and severity levels. Includes testing red squiggles for schema violations, matcher mismatches, and structural errors.

    - Publishing diagnostics for a schema violation (literal text mismatch)
    - Publishing diagnostics for a matcher regex mismatch
    - Publishing diagnostics for a node type mismatch (e.g., expected heading, found paragraph)
    - Publishing diagnostics for multiple errors in a single document
    - Publishing diagnostics with correct line and column positions
    - Publishing diagnostics with correct severity levels (error vs warning)
    - Clearing diagnostics when document becomes valid after edit
    - Publishing diagnostics when document becomes invalid after edit
    - Publishing diagnostics for nested structural errors (e.g., list item within wrong list type)
    - Publishing diagnostics for EOF-related errors when streaming is disabled
  ],
)

We will also add a performance test for the LSP, where we simulate very fast
typing into a document that is about 2,000 lines, and make sure that
`mdvalidate` is able to keep up at under `50ms` per keystroke and `100ms` at the
end.

= Performance testing

We will will also create `json schema` examples

In our initial SRS we stated that we would like to be able to validate 1.3k
characters of Markdown Input in 20ms. We will make a Markdown file that has 1.3k
lines of literal headings:

#schema(
  ```markdown
  # `testing:Testing\d`+
  ```,
)

#good-example(
  ```markdown
  # Testing 1
  # Testing 2
  # Testing 3
  # Testing 4
  ```,
)

#bad-example(
  ```markdown
  # Testing one
  # Testing two
  # Testing three
  # Testing four
  ```,
)

We will make sure that validation for valid input and failure for bad input is
correct and accurate during this stress test.

We will run our stress tests via the command line so we have an accurate picture
of overhead, and with a release build.

= Structural Testing

We also implemented structural testing to ensure that the codebase is covered by tests.

We set up code coverage checking using
#link("https://llvm.org/docs/CommandGuide/llvm-cov.html", "llvm-cov").

You can run the code coverage checking by using

```bash
./scripts/coverage.sh
```

The current code coverage is shown in @fig:code-coverage.

#figure(
  table(
    columns: (2fr, 1fr, 1fr, 1fr),
    align: (left, center, center, center),
    table.header([*Filename*], [*Function Coverage*], [*Line Coverage*], [*Region Coverage*]),
    [build/rustc-1.89.0-src/library/std/src/sys/thread_local/native/mod.rs],
    [66.67% (2/3)],
    [66.67% (2/3)],
    [57.14% (4/7)],
    [mdvalidate/src/cmd.rs], [72.73% (8/11)], [91.20% (114/125)], [85.99% (221/257)],
    [mdvalidate/src/main.rs], [0.00% (0/2)], [0.00% (0/35)], [0.00% (0/69)],
    [mdvalidate/src/mdschema/reports/errors.rs], [75.00% (3/4)], [87.50% (28/32)], [87.88% (29/33)],
    [mdvalidate/src/mdschema/reports/pretty_print.rs], [50.00% (1/2)], [17.86% (5/28)], [13.04% (6/46)],
    [mdvalidate/src/mdschema/reports/validation_report.rs],
    [100.00% (3/3)],
    [100.00% (13/13)],
    [100.00% (9/9)],
    [mdvalidate/src/mdschema/validator/binode_validator.rs],
    [100.00% (16/16)],
    [87.14% (305/350)],
    [86.55% (579/669)],
    [mdvalidate/src/mdschema/validator/matcher.rs], [85.71% (6/7)], [71.43% (25/35)], [85.92% (61/71)],
    [mdvalidate/src/mdschema/validator/utils.rs], [33.33% (1/3)], [22.58% (7/31)], [16.36% (9/55)],
    [mdvalidate/src/mdschema/validator/validator.rs],
    [100.00% (13/13)],
    [88.18% (194/220)],
    [90.70% (351/387)],
    table.cell(colspan: 4, []),
    [*Totals*], [*82.81% (53/64)*], [*79.47% (693/872)*], [*79.16% (1269/1603)*],
  ),
) <fig:code-coverage>

We also began adding some performance tests, starting with a flame graph, so
that we would be better able to find bottlenecks in `mdvalidate`'s performance.
We set up a flamegraph using
#link("https://github.com/flamegraph-rs/flamegraph", "rust flamegraph"), and our
current flamegraph can be seen in @fig:flamegraph.

#figure(
  image("images/flamegraph.svg"),
) <fig:flamegraph>
