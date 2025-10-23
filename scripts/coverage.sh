#!/usr/bin/env bash

cargo llvm-cov clean --workspace
cargo llvm-cov --no-report --ignore-filename-regex --open '/rustc/|/nix/store/' test