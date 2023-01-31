pub const RAM_START: u32 = 0x2000_0000;
pub const RAM_SIZE: u32 = 0x4000; // 16 kB
pub const RAM_END: u32 = RAM_START + RAM_SIZE;

/// Calculate free stack space when using flip-link
///
/// With flip-link stack grows from `RAM_END - sizeof(data+bss+uninit)` up to `RAM_START`.
#[inline(always)]
pub fn stack_free() -> u32 {
    let msp = cortex_m::register::msp::read();
    msp - RAM_START
}

/// Calculate used stack space when using flip-link
#[inline(always)]
pub fn stack_used() -> u32 {
    stack_size() - stack_free()
}

/// Stack size assuming .data is first (__sdata) and __eheap points to last section end
#[inline(always)]
pub fn stack_size() -> u32 {
    let start = symbols::data_start() as *const u8;
    let end = symbols::heap_start() as *const u8;
    let nonstack_size = unsafe { end.offset_from(start) as u32 };
    RAM_SIZE - nonstack_size
}

/// Get whole stack memory as slice (when using flip-link)
#[inline(always)]
pub fn stack_as_slice() -> &'static [u8] {
    let start = RAM_START as *const u32 as *const u8;
    let end = symbols::data_start() as *const u8;
    unsafe {
        core::slice::from_raw_parts(start, end.offset_from(start) as usize)
    }
}

/// Get free stack memory as mutable slice (when using flip-link)
///
/// # Safety
///
/// Any code after calling this inline-function may allocate more data on the stack
/// so writing to the returned slice may overwrite the stack which is totally unsafe.
#[inline(always)]
pub unsafe fn free_stack_as_slice() -> &'static mut [u8] {
    let start = RAM_START as *mut u32 as *mut u8;
    let end = cortex_m::register::msp::read() as *mut u32 as *mut u8;
    core::slice::from_raw_parts_mut(start, end.offset_from(start) as usize)
}

const SENTINEL: u8 = 0xcd;

/// Fill the free stack space with a known value
///
/// # Safety
///
/// In theory it fills unoccupied stack space, but if memory layout is different than
/// assumed one (e.g. not using flip-link) then this might just overwrite arbitrary memory.
#[inline(always)]
pub unsafe fn free_stack_fill(margin: u32) {
    let free_stack = free_stack_as_slice();
    let size = free_stack.len();
    free_stack[..size - margin as usize].fill(SENTINEL)
}

/// Calculate minimum free stack since last `free_stack_fill` by examining memory content
#[inline(always)]
pub fn free_stack_check_min() -> u32 {
    let whole_stack = stack_as_slice();
    let size = stack_size();
    let unmodified = whole_stack.iter().position(|b| *b != SENTINEL)
        .unwrap_or(size as usize);
    size - unmodified as u32
}

pub fn print_stack_info() {
    let stack_size = stack_size();
    let curr_used = stack_used();
    let min_free = free_stack_check_min();
    let max_used = stack_size - min_free;
    defmt::info!("Stack usage: current {=u32} B ({=u32}%), max {=u32} B ({=u32}%) / {=u32} B",
        curr_used, 100 * curr_used / stack_size,
        max_used, 100 * max_used / stack_size,
        stack_size);
}

/// Values of linker symbols
pub mod symbols {
    macro_rules! symbol_getters {
        ($($sym:ident: $getter:ident),+ $(,)?) => {
            $(
                #[inline(always)]
                pub fn $getter() -> *mut u32 {
                    extern "C" { static mut $sym: u32; }
                    unsafe { &mut $sym }
                }
            )+
        };
    }

    // Example values with flip-link:
    //  data_start   = 0x20002b28
    //  data_end     = 0x20002c40
    //  bss_start    = 0x20002c40
    //  bss_end      = 0x20002f5c
    //  uninit_start = 0x20002f5c
    //  uninit_end   = 0x20003ffc
    //  heap_start   = 0x20003ffc
    //  text_start   = 0x080000c0
    //  text_end     = 0x0800a420
    //  rodata_start = 0x0800a420
    //  rodata_end   = 0x0800b91c
    // Example values without filp link
    //  data_start   = 0x20000000
    //  data_end     = 0x20000118
    //  bss_start    = 0x20000118
    //  bss_end      = 0x20000434
    //  uninit_start = 0x20000434
    //  uninit_end   = 0x200014d4
    //  heap_start   = 0x200014d4
    //  text_start   = 0x080000c0
    //  text_end     = 0x0800a420
    //  rodata_start = 0x0800a420
    //  rodata_end   = 0x0800b91c
    symbol_getters! {
        __sheap: heap_start,
        __sdata: data_start,
        __edata: data_end,
        __sbss: bss_start,
        __ebss: bss_end,
        __suninit: uninit_start,
        __euninit: uninit_end,
        __stext: text_start,
        __etext: text_end,
        __srodata: rodata_start,
        __erodata: rodata_end,
    }
}
