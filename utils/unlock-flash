#!/usr/bin/env bash

# Sometimes a fresh mcu has flash write protection enabled and flashing will fail.
# This command should unlock it (can also try `st-flash erase`):
openocd \
    -f interface/stlink.cfg \
    -f target/stm32f0x.cfg \
    -c "init; reset halt; stm32f0x unlock 0; reset halt; exit"
