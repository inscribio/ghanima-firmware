# Show this help
default:
    @just --list

### Build ###

config-test-env := 'CARGO_TARGET_DIR=/tmp/cargo-target-ghanima-config DEFMT_LOG=off'
cargo-args := '--release --features thumbv6'

# Build firmware and generate .bin file
build *ARGS:
    cargo build {{cargo-args}} {{ARGS}}
    cargo objcopy {{cargo-args}} --bin ghanima {{ARGS}} -- -O binary target/ghanima.bin

# Run cargo check on any file change
watch-check *ARGS:
    cargo watch -c -- cargo check {{cargo-args}} {{ARGS}}

# Generate Rust type sizes tree as HTML
type-sizes *ARGS:
    type-sizes --bin ghanima {{cargo-args}} --output-dir ./tmp/type-sizes --exclude-std {{ARGS}}

# Generate and open docs
doc *ARGS:
    cargo doc {{cargo-args}} --document-private-items --features smlang/graphviz --open {{ARGS}}

# Generate and open docs for the binary crate (replaces lib docs)
doc-bin: (doc "--bin" "ghanima")

# Calculate binary size
size *ARGS:
    cargo size {{cargo-args}} --bin ghanima {{ARGS}}

# Output binary disassembly to stdout (redirect to desired file)
disassemble *ARGS:
    arm-none-eabi-objdump -D target/thumbv6m-none-eabi/release/ghanima | arm-none-eabi-c++filt

### Remote ###

# Flash firmware using helper script (with auto detach)
flash *ARGS:
    utils/flash {{ARGS}}

# Flash given .bin file over debug probe using openocd. Build it first!
openocd-flash FILE:
    openocd -f remote/openocd.cfg -c "program {{FILE}} verify reset exit 0x08000000"

# Run firmware over debug probe using probe-rs
run *ARGS:
    cargo run {{cargo-args}} {{ARGS}}

# Start debugging with gdb
gdb *ARGS:
    cargo build {{cargo-args}} {{ARGS}}
    cargo objcopy {{cargo-args}} --bin ghanima {{ARGS}} -- -O binary target/ghanima.bin
    cd remote && arm-none-eabi-gdb ../target/thumbv6m-none-eabi/release/ghanima -x ./gdbinit

# Start post-mortem debugging with gdb
gdb-postmortem *ARGS:
    cd remote && arm-none-eabi-gdb ../target/thumbv6m-none-eabi/release/ghanima -x ./gdbinit-postmortem

### Tests ###

# Run firmware tests
test *ARGS:
    DEFMT_LOG=off cargo test --target x86_64-unknown-linux-gnu {{ARGS}}

# Run firmware-config tests
test-config *ARGS:
    {{config-test-env}} cargo test -p ghanima-config --target x86_64-unknown-linux-gnu {{ARGS}}

# Continuously run firmware tests
watch-test *ARGS:
    DEFMT_LOG=off cargo watch -c -x 'test --target x86_64-unknown-linux-gnu {{ARGS}}'

# Continuously run firmware-config tests
watch-test-config *ARGS:
    {{config-test-env}} cargo watch -p ghanima-config -c -x 'test --target x86_64-unknown-linux-gnu {{ARGS}}'

# Run tests in GDB to debug panics, use `just test` to find TEST_BIN path (target/...)
test-gdb TEST_BIN TEST_NAME:
    DEFMT_LOG=off gdb -ex "break rust_panic" -ex "run" --args {{TEST_BIN}} {{TEST_NAME}} --nocapture
