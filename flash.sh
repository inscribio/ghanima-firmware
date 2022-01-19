#!/usr/bin/env bash

# exit on any error
set -euo pipefail

# only output colors if our output is to terminal
use_ansi() { test -t 1; }
if use_ansi ; then
    RED="\033[0;31m"
    BOLD="\033[1m"
    CLEAR="\033[0m"
else
    RED=""
    BOLD=""
    CLEAR=""
fi

info() {
    echo
    echo -e "${BOLD}""$*""${CLEAR}"
    echo -e "${BOLD}""==================================================""${CLEAR}"
}

die() {
    echo
    echo -e "${RED}""$*""${CLEAR}" 1>&2; exit 1
    echo -e "${RED}""==================================================""${CLEAR}"
}

###

while (( $# > 0 )); do
    case "$1" in
        -h)
            echo "Usage: $(basename $0) [ --first-flash ]"
            exit 0 ;;
        --first-flash)
            first=yes ;;
        --prebuilt)
            shift
            prebuilt="$1" ;;
        *)
            echo "Unknown option: $1"
            exit 1 ;;
    esac
    shift
done

opt='--release'
target_name='ghanima'

stm32dfu_id='0483:df11'
keyboard_id='16c0:27db'

target_bin="target/$target_name.bin"
cargo_flags="$opt"
dfu_modifiers="leave"

# on first flashing we may ned to unprotect the flash
[[ ${first:-no} = 'yes' ]] && dfu_modifiers="force:unprotect"

###

check_firmware() {
    if [[ ${prebuilt:-0} = 0 ]]; then
        info "Checking firmware ..."
        cargo check "$cargo_flags"
    else
        info "Checking firmware (prebuilt) ..."
        test -f "$prebuilt"
    fi
}

# returns path on stdout, all messages on stderr
get_firmware() {
    if [[ ${prebuilt:-0} = 0 ]]; then
        info "Building firmware ..." 1>&2
        # build firmware
        cargo build "$cargo_flags" 1>&2 || die "Failed to build firmware"
        # convert to binary
        cargo objcopy "$cargo_flags" --bin "$target_name" -- -O binary "$target_bin" 1>&2
        echo "$target_bin"
    else
        info "Using prebuilt firmware: $prebuilt ..." 1>&2
        echo "$prebuilt"
    fi
}

# Get value of USB device string: lsusb_string VIDPID ISTRING
lsusb_string() {
    # lines look like:
    #  iManufacturer           1 inscrib.io
    # squeeze multi-spaces into single space then cut
    lsusb -d "$1" -v | grep "$2" | tr -s ' ' | cut -d ' ' -f 4-
}

find_keyboard() {
    lsusb -d "$keyboard_id" > /dev/null
}

find_bootloader() {
    lsusb -d "$stm32dfu_id" > /dev/null
}

detach_keyboard() {
    find_keyboard || die "Keyboard not found"

    imanufacturer=$(lsusb_string $keyboard_id 'iManufacturer')
    iproduct=$(lsusb_string $keyboard_id 'iProduct')
    iserial=$(lsusb_string $keyboard_id 'iSerial')
    info "Found keyboard: $imanufacturer | $iproduct | $iserial"

    # check before detaching to avoid loosing keyboard unnecesarily
    check_firmware

    # detach to bootloader
    info "Detaching keyboard to bootloader ..."
    dfu-util --detach --device $keyboard_id
}

flash_firmware() {
    local firmware
    firmware="$(get_firmware)"

    # wait for stm32 bootloader
    info "Waiting for DFU bootloader ..."
    for _ in {1..15}; do
        find_bootloader && break
        sleep 0.2
        echo -n .
    done
    echo
    find_bootloader || die "Could not find DFU bootloader device"

    info "Flashing device ..."
    dfu-util --device $stm32dfu_id \
        --alt 0 \
        --dfuse-address "0x08000000:$dfu_modifiers" \
        --download "$firmware" \
        --reset
}

if find_keyboard; then
    detach_keyboard
    flash_firmware
elif find_bootloader; then
    info "No keyboard found but found an STM32 DFU Bootloader ..."
    flash_firmware
else
    die "No keyboard, nor bootloader found"
fi
