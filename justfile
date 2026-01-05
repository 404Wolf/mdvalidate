build:
    cargo build

test:
    cargo test

format:
    nix fmt

[working-directory: './docs']
docs-dev:
    bun run dev

[working-directory: './docs']
docs-build:
    bun run build

[working-directory: './docs']
docs-start:
    bun run start
