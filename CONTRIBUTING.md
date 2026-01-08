# Welcome!

Right now `mdvalidate` is in a super beta state. Contributions will be considered but the direction of the project is still variable and your changes may not get accepted.

# Conventions

There's not a full list of general guidelines right now, but some worthy pointers:
- When we are talking about a data structure that stores some reference to schema and the input, store the schema first.
- Prefer `get_node_text` from `ts_utils` over direct `utf8_text` calls when reading schema node text.

# Debugging

You might find it useful to initialize the logger in tests temporarily to get useful logs and traces. You can do so with the following macro (from `utils.rs`):

```rs
use crate::test_logging;

test_logging!();
```
