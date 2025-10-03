#!/usr/bin/env bash

set -euo pipefail

for file in docs/images/*.mmd; do
    [ -e "$file" ] || continue
    basename=$(basename "$file" .mmd)
    out="docs/images/${basename}.png"
    mmdc -i "$file" -o "$out"
done
