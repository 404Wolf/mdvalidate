#import "@preview/typslides:1.3.0": *

// Project configuration
#show: typslides.with(
  ratio: "16-9",
  theme: "bluey",
  font: "Fira Sans",
  font-size: 20pt,
  link-style: "color",
  show-progress: true,
)

// Front slide
#front-slide(
  title: "mdvalidate",
  subtitle: [Markdown Schema Validation],
  authors: "Wolf Mermelstein and Alessandro Mason",
  info: [#link("https://github.com/404wolf/mdvalidate")],
)

// Table of contents
/* #table-of-contents() */

// Title slide for introduction
#title-slide[
  What is _mdvalidate_?
]

// Slide 1: What is mdvalidate?
#slide[
  #stress("A tool that validates Markdown documents against schemas")
  
  - Write schemas in Markdown itself
  - Validate structure and content patterns
  - Extract structured data (JSON) from matches
  - Built for streaming validation (perfect for LLMs)
]

// Title slide for distinguishing features
#title-slide[
  What _Distinguishes_ mdvalidate?
]

// Slide 2: What Makes It Special?
#slide(title: "Key Differentiators")[
  #cols(columns: (1fr, 1fr), gutter: 1.5em)[
    #framed(title: "Schemas ARE Markdown")[
      No separate syntax to learn. Your schema files are valid Markdown that render beautifully.
    ]
  ][
    #framed(title: "Streaming Validation")[
      Validates as data streams in via stdin. Perfect for real-time LLM output validation.
    ]
  ]
  
  #cols(columns: (1fr, 1fr), gutter: 1.5em)[
    #framed(title: "Incremental Parsing")[
      Uses Tree-sitter for efficient incremental AST updates. Fast failure on errors.
    ]
  ][
    #framed(title: "Data Extraction")[
      Automatically extracts matched content as structured JSON. Two-way data flow.
    ]
  ]
]

// Title slide for architecture
#title-slide[
  _Architecture_
]

// Slide 3: Architecture Overview
#slide(title: "System Architecture", outlined: true)[
  *Core Components:*
  
  1. *Schema Parser* - Parses MDS schema files using Tree-sitter Markdown
  
  2. *Input Parser* - Incrementally parses input Markdown stream
  
  3. *Zipper Tree Validator* - Walks both ASTs simultaneously, comparing structure
  
  4. *Matcher Engine* - Applies regex patterns and extracts matches
  
  5. *Error Reporter* - Generates human-readable error messages with locations
  
  #grayed[Built in 100% safe Rust for performance and reliability]
]

// Slide 4: Zipper Tree Approach
#slide(title: "Zipper Tree Validation")[
  We walk the schema AST and input AST *in parallel*:
  
  #cols(columns: (1fr, 1fr), gutter: 2em)[
    #framed(title: "Schema AST", back-color: rgb(240, 245, 255))[
      Root
      └─ Heading
           └─ Text("Title")
      └─ Paragraph
    ]
  ][
    #framed(title: "Input AST", back-color: rgb(245, 255, 245))[
      Root
      └─ Heading
           └─ Text("My Title")
      └─ Paragraph
    ]
  ]
  
  As we walk, we validate structure matches and apply matcher patterns.
]

// Title slide for language features
#title-slide[
  MDS _Language_ Features
]

// Slide 5: Key Language Features
#slide(title: "Schema Language Features")[
  *Matchers:* `` `label:/regex/` ``
  
  *Optional:* Add `?` suffix
  
  *Repeated:* Add `+` or `{n,m}` suffix
  
  *Lists:* Validate nested structures with depth control
  
  *Special Types:* `text`, `number`, `html`, `ruler`
  
  #framed(title: "Example Schema", back-color: rgb(255, 250, 240))[
    Heading with name matcher
    Optional bio paragraph
    List with item matchers
  ]
]

// Title slide for lessons learned
#title-slide[
  _Lessons_ Learned
]

// Slide 6: Lessons Learned
#slide(title: "Key Insights", outlined: true)[
  #framed(title: "1. Incremental Parsing is Powerful", back-color: rgb(240, 250, 255))[
    Tree-sitter's incremental parsing enabled streaming validation, a key differentiator. The ability to validate as data arrives is crucial for LLM integration.
  ]
  
  #framed(title: "2. Making Schemas Valid Markdown Was Worth It", back-color: rgb(255, 250, 240))[
    By designing MDS to be valid Markdown, we gained syntax highlighting, editor support, and readability for free. Users can preview schemas like any Markdown file.
  ]
  
  #framed(title: "3. Zipper Trees Simplify Validation", back-color: rgb(250, 255, 240))[
    Walking two ASTs in parallel with a zipper pattern made the validation logic clean and maintainable. The cursor-based approach naturally handles incremental updates.
  ]
]

// Title slide for demo
#title-slide[
  _Demo_: Main Features
]

// Slide 7: Demo Overview
#slide(title: "Live Demonstration")[
  *Live demonstration of:*
  
  #cols(columns: (1fr, 1fr), gutter: 1.5em)[
    #framed(title: "1. Basic Validation", back-color: rgb(230, 255, 230))[
      Simple schema with literal and regex matchers
    ]
  ][
    #framed(title: "2. List Validation", back-color: rgb(255, 230, 230))[
      Nested lists with constraints and optional items
    ]
  ]
  
  #cols(columns: (1fr, 1fr), gutter: 1.5em)[
    #framed(title: "3. JSON Extraction", back-color: rgb(230, 230, 255))[
      Extracting structured data from validated documents
    ]
  ][
    #framed(title: "4. Error Reporting", back-color: rgb(255, 255, 230))[
      Clear, actionable error messages with locations
    ]
  ]
]

// Slide 8: Demo Script
#slide(title: "Demo Examples")[
  #framed(title: "Example 1: Contact Card Schema", back-color: rgb(250, 250, 250))[
    Heading with name regex
    Optional bio text
    Contact section with email pattern
  ]
  
  #framed(title: "Example 2: Grocery List with Constraints", back-color: rgb(250, 250, 250))[
    List items with capital letters
    Up to 2 optional notes per item
  ]
  
  *Show:* Validation success, failure, JSON output, and error messages
]

// Title slide for use cases
#title-slide[
  _Use Cases_
]

// Slide 9: Use Cases
#slide(title: "Real-World Applications")[
  #cols(columns: (1fr, 1fr), gutter: 1.5em)[
    #framed(title: "LLM Output Validation", back-color: rgb(240, 250, 255))[
      Ensure AI-generated Markdown conforms to expected structure in real-time
    ]
  ][
    #framed(title: "Documentation Standards", back-color: rgb(255, 250, 240))[
      Enforce consistent structure across technical documentation
    ]
  ]
  
  #cols(columns: (1fr, 1fr), gutter: 1.5em)[
    #framed(title: "CI/CD Integration", back-color: rgb(250, 255, 240))[
      Validate Markdown files in automated pipelines
    ]
  ][
    #framed(title: "Data Extraction", back-color: rgb(255, 240, 250))[
      Convert semi-structured Markdown to JSON for processing
    ]
  ]
]

// Title slide for performance
#title-slide[
  Performance & _Future_
]

// Slide 10: Performance & Future
#slide(title: "Performance & Future Work", outlined: true)[
  #framed(title: "Current Performance", back-color: rgb(250, 250, 250))[
    - Validates 1.3k characters in < 20ms
    - Handles streaming input efficiently
    - Fast-fail mode for early termination
  ]
  
  #framed(title: "Future Enhancements", back-color: rgb(245, 255, 245))[
    - Language Server Protocol (LSP) for editor integration
    - Template generation (JSON → Markdown)
    - JSON Schema export from MDS schemas
    - Enhanced error recovery and suggestions
  ]
]

// Conclusion slide
#focus-slide[
  *Thank you for your attention!*
  
]
