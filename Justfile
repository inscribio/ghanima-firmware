### Build ###

# Build firmware and generate .bin file
build *ARGS:
    cargo build --release {{ARGS}}
    cargo objcopy --release --bin ghanima -- -O binary target/ghanima.bin

# Run cargo check on any file change
watch-check *ARGS:
    cargo watch -c -- cargo check --release {{ARGS}}

### Remote ###

# Flash firmware using helper script (with auto detach)
flash *ARGS:
    utils/flash {{ARGS}}

# Flash given .bin file over debug probe using openocd. Build it first!
openocd-flash FILE:
    openocd -f remote/openocd.cfg -c "program {{FILE}} verify reset exit 0x08000000"

# Run firmware over debug probe using probe-rs
run *ARGS:
    cargo run --release {{ARGS}}

# Start debugging with gdb
gdb *ARGS:
    cargo build --release {{ARGS}}
    cargo objcopy --release --bin ghanima -- -O binary target/ghanima.bin
    cd remote && arm-none-eabi-gdb "../target/thumbv6m-none-eabi/debug/ghanima" -x ./gdbinit

### Tests ###

# Run firmware tests
test *ARGS:
    DEFMT_LOG=off cargo test --target x86_64-unknown-linux-gnu {{ARGS}}

# Run firmware-config tests
test-config *ARGS:
    cd config && cargo test --target x86_64-unknown-linux-gnu {{ARGS}}

# Continuously run firmware tests
watch-test *ARGS:
    DEFMT_LOG=off cargo watch -x 'test --target x86_64-unknown-linux-gnu {{ARGS}}'

# Continuously run firmware-config tests
watch-test-config *ARGS:
    cd config && DEFMT_LOG=off cargo watch -x 'test --target x86_64-unknown-linux-gnu {{ARGS}}'
