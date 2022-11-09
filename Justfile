build *ARGS:
    cargo build --release {{ARGS}}
    cargo objcopy --release --bin ghanima -- -O binary target/ghanima.bin

flash *ARGS:
    utils/flash {{ARGS}}

gdb *ARGS:
    cargo build --release {{ARGS}}
    cargo objcopy --release --bin ghanima -- -O binary target/ghanima.bin
    cd remote && arm-none-eabi-gdb "../target/thumbv6m-none-eabi/debug/ghanima" -x ./gdbinit

test *ARGS:
    DEFMT_LOG=off cargo test --target x86_64-unknown-linux-gnu {{ARGS}}

test-config *ARGS:
    cd config && cargo test --target x86_64-unknown-linux-gnu {{ARGS}}

watch-test *ARGS:
    DEFMT_LOG=off cargo watch -x 'test --target x86_64-unknown-linux-gnu {{ARGS}}'
