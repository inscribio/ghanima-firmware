build *ARGS:
    cargo build --release {{ARGS}}
    cargo objcopy --release --bin ghanima -- -O binary target/ghanima.bin

flash *ARGS:
    utils/flash {{ARGS}}

test *ARGS:
    DEFMT_LOG=off cargo test --target x86_64-unknown-linux-gnu {{ARGS}}

watch-test *ARGS:
    DEFMT_LOG=off cargo watch -x 'test --target x86_64-unknown-linux-gnu {{ARGS}}'
