#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let a: &str = core::hint::black_box("name");
    let b: &str = core::hint::black_box("name");
    if a == b {
        exit(0)
    } else {
        exit(1)
    }
}

fn exit(code: u32) -> ! {
    unsafe {
        core::arch::asm!(
            "li $v0, 5058",
            "move $a0, {0}",
            "syscall",
            in(reg) code,
            options(noreturn),
        );
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { exit(2) }
