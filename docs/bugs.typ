#import "template.typ": *

#show: template.with(
  title: "Known Bugs",
  author: "Wolf Mermelstein and Alessandro Mason",
)

= Recursive List Validation

*Status:* In Progress (PR #46)

The validator currently lacks proper support for recursive list validation. We want to use recursive logic to handle nested list structures, but the implementation doesn't work yet.

== Expected Behavior

Given a schema with nested matchers:
```markdown
- `test:/test2/`{,}
  - `foo:/test/`{,}
```

The validator should be able to match inputs and produce results shaped like:
```json
[test2, test2, {foo: [test, test]}]
```

== Current Issues

- The recursive matching logic from PR #46 is not yet functional
- Nested list structures cannot be properly validated
- Output shape does not match the expected nested structure

= Size Checking for Nested Arrays

*Status:* Known Issue

Size constraints (e.g., `{,2}` for "up to 2 items") do not work correctly for nested arrays within lists.

== Impact

- Quantifier validation fails on nested list items
- Schemas cannot reliably enforce length constraints on sublists
- Over-validation or under-validation may occur on nested structures

= Unnecessary Validation Restarts During Streaming

*Status:* Known Issue

When validating streaming input, the validator sometimes restarts validation from the beginning even when incremental validation would be sufficient.

== Impact

- Redundant work is performed
- Performance degradation on large streaming inputs
- CPU cycles wasted re-validating already-processed content

== Cause

The validator may not properly track which portions of the input have been validated, causing it to restart from the top-level schema node when new input arrives.

= Missing Markdown Feature Support

*Status:* Not Implemented

Several standard Markdown features are not yet supported by the validator.

== Unsupported Features

- *Tables:* Markdown table syntax is not recognized or validated
- *HTML:* Inline HTML and HTML blocks cannot be matched or validated
- Other features may also be missing

== Impact

- Schemas cannot describe documents that use tables or HTML
- Documents with these features may fail validation unexpectedly
- Limits the validator's applicability to real-world Markdown documents

= Error Messages Missing Expected Regex

*Status:* Known Issue (Issue #32)

When a matcher with a regex pattern fails to match input, the error message does not always display the expected regex pattern.

== Expected Behavior

If a schema has `` `item:/[A-Z][a-z]+/` `` and the input is `"123"`, the error should show:
```
Expected pattern: /[A-Z][a-z]+/
Got: 123
```

== Current Behavior

The error message may omit the expected regex pattern, making it difficult to debug why validation failed.

== Impact

- Harder to diagnose validation failures
- Users cannot see what pattern was expected
- Debugging schemas becomes more difficult
