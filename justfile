# Same gate as homestead: format, lint as errors, test.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

# Exact counters, including the CLI and every library; no production exclusions.
coverage:
    python3 scripts/test_coverage.py
    cargo llvm-cov --workspace --all-targets --ignore-filename-regex '/tests/' --json --output-path target/coverage.json
    python3 scripts/coverage.py target/coverage.json --check

# Mutation-fuzz the page and transcript readers: `just fuzz [secs per target] [target]`.
fuzz *args:
    cargo run -q --profile fuzz -p admit-fuzz -- {{args}}

plan *args:
    cargo run -q -p admit-cli --bin admit -- plan {{args}}

apply *args:
    cargo run -q -p admit-cli --bin admit -- apply {{args}}

audit:
    cargo run -q -p admit-cli --bin admit -- audit

pull *args:
    cargo run -q -p admit-cli --bin admit -- pull {{args}}

validate:
    cargo run -q -p admit-cli --bin admit -- validate
