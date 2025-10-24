#!/usr/bin/env bash

cargo llvm-cov clean --workspace
cargo llvm-cov --open --ignore-filename-regex '/rustc/|/nix/store/' test