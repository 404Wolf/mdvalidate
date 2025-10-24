#!/usr/bin/env bash

export CARGO_PROFILE_RELEASE_DEBUG=true

cargo build --profile flamegraph
cargo flamegraph --profile flamegraph --bin mdv -- example/schema.md example/input.md