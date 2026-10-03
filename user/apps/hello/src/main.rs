#![no_std]
#![no_main]

extern crate alloc;

use libluke::{exit, getpid, println};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let pid = getpid();
    println!("Hello from Ring 3 user process! PID = {}", pid);
    exit(0);
}
