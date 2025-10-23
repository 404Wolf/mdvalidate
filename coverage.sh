cargo install cargo-llvm-cov
cargo llvm-cov clean --workspace
cargo llvm-cov test --workspace --html