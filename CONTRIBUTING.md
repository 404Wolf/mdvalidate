Right now `mdvalidate` is in a super beta state. Contributions will be considered but the direction of the project is still variable and your changes may not get accepted.

# Debugging

You might find it useful to initialize the logger in tests temporarily to get useful logs and traces. You can do so with the following macro (from `utils.rs`):

```rs
use crate::test_logging;

test_logging!();
```
