### Build ###

config-test-env := 'CARGO_TARGET_DIR=/tmp/cargo-target-ghanima-config DEFMT_LOG=off'

# Build firmware and generate .bin file
build *ARGS:
    cargo build --release {{ARGS}}
    cargo objcopy --release --bin ghanima {{ARGS}} -- -O binary target/ghanima.bin

# Run cargo check on any file change
watch-check *ARGS:
    cargo watch -c -- cargo check --release {{ARGS}}

type-sizes *ARGS:
    type-sizes --bin ghanima --release --exclude-std {{ARGS}}

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
    cargo objcopy --release --bin ghanima {{ARGS}} -- -O binary target/ghanima.bin
    cd remote && arm-none-eabi-gdb ../target/thumbv6m-none-eabi/release/ghanima -x ./gdbinit

gdb-postmortem *ARGS:
    cd remote && arm-none-eabi-gdb ../target/thumbv6m-none-eabi/release/ghanima -x ./gdbinit-postmortem

### Tests ###

# Run firmware tests
test *ARGS:
    DEFMT_LOG=off cargo test --target x86_64-unknown-linux-gnu {{ARGS}}

# Run firmware-config tests
test-config *ARGS:
    cd config && {{config-test-env}} cargo test --target x86_64-unknown-linux-gnu {{ARGS}}

# Continuously run firmware tests
watch-test *ARGS:
    DEFMT_LOG=off cargo watch -c -x 'test --target x86_64-unknown-linux-gnu {{ARGS}}'

# Continuously run firmware-config tests
watch-test-config *ARGS:
    cd config && {{config-test-env}} cargo watch -c -x 'test --target x86_64-unknown-linux-gnu {{ARGS}}'

# Run tests in GDB to debug panics, use `just test` to find TEST_BIN path (target/...)
test-gdb TEST_BIN TEST_NAME:
    DEFMT_LOG=off gdb -ex "break rust_panic" -ex "run" --args {{TEST_BIN}} {{TEST_NAME}} --nocapture
