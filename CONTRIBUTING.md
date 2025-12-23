# WIP


You might find it useful to initialize the logger in tests temporarily to get
useful logs.

```rs
let _ = env_logger::builder()
    .filter_level(log::LevelFilter::Trace)
    .is_test(true)
    .try_init();
```
