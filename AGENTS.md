# Agent Guidelines

## ts_types imports
- Always import `ts_types` via wildcard (`use crate::mdschema::validator::ts_types::*;`) so we do not list individual members.

## Test imports
- Prefer `super::...` imports inside `#[cfg(test)]` modules (e.g., `super::test_utils::ValidatorTester` or `super::TextualVsTextualValidator`) so the tests stay concise.
- Keep using wildcard `ts_types::*` in tests as well.

## Documentation
- When a doc block lists both `schema_str` and `input_str`, use the exact wording:
  - `schema_str`: The full input document (so far).
  - `input_str`: The full schema document.
- For any doc line that mentions `got_eof`, use `/// * `got_eof`: Whether we have received the full input document.` verbatim.

## Walker usage
- Never add aliases such as `let schema_str = walker.schema_str()` or `let input_str = walker.input_str()`; call the walker methods directly.

## Contributing-from-CONTRIBUTING.md
- When we talk about a data structure that stores references to schema and input, keep the schema entry first.
- Prefer `get_node_text` from `ts_utils` over calling `utf8_text` directly.
- In tests, keep assertion order consistent: position assertions first, followed by errors, then values.
- Avoid `ValidationResult::destruct`; use accessors like `result.errors()`, `result.value()`, or `result.farthest_reached_pos()`.
- When debugging tests, call `test_logging!();` (from `utils.rs`) at the top of the suite to hydrate logs and trace output.
