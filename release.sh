#!/usr/bin/env bash

git cliff -o CHANGELOG.md --tag {{version}}
git add CHANGELOG.md
git commit -m "chore: update CHANGELOG.md"
cargo-release release $@ --execute
