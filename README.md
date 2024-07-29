# Ghanima keyboard firmware

This repository contains source code for firmware of the Ghanima keyboard. You can buy the keyboard
on [inscrib.io](https://inscrib.io/) and use [the online configurator](https://inscrib.io/inscribe/)
to easily generate correct firmware for your keyboard, as well as manage different firmware configurations.

## Building from sources

The code is written in Rust. You will need [Cargo](https://github.com/rust-lang/cargo) and Rust
toolchain for target `thumbv6m-none-eabi` (`rustup target add thumbv6m-none-eabi`).

Many useful commands are available in the `justfile`, e.g.

* `just build` - build with default configuration
* `just flash` - build with default configuration and flash
* `GHANIMA_JSON_CONFIG=your/config.json just flash -- --features json-config` - use custom configuration JSON
* `just test && just test-config` - run all tests
