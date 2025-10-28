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
)


#pagebreak()



== EOF Handling Tests

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

#set text(size: 9pt)


= Structural Testing

We also implemented structural testing to ensure that the codebase is covered by tests.

//! insert image and subsequent explanation


= Summary


