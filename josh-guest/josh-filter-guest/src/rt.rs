//! Minimal runtime for pure-`no_std` guests (feature `rt`, on by default):
//! a leak-only bump allocator plus an aborting panic handler. Guests built
//! with `std` must disable this feature — `std` provides both already.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;

const PAGE: usize = 65536;

/// Bump allocator over pages appended to the end of linear memory via
/// `memory.grow`. `dealloc` is a no-op: the instance is discarded right
/// after `josh_run`, so everything leaks by design.
struct BumpAlloc {
    /// `(next free byte, end of owned region)`; `(0, 0)` = uninitialized.
    state: UnsafeCell<(usize, usize)>,
}

// SAFETY: wasm32-unknown-unknown guests are single-threaded.
unsafe impl Sync for BumpAlloc {}

unsafe impl GlobalAlloc for BumpAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let (next, end) = unsafe { &mut *self.state.get() };
        if *end == 0 {
            // Start the arena at the current end of linear memory
            // (memory.grow with a delta of 0 returns the current size).
            let base = core::arch::wasm32::memory_grow::<0>(0) * PAGE;
            *next = base;
            *end = base;
        }
        let Some(ptr) = next
            .checked_add(layout.align() - 1)
            .map(|p| p & !(layout.align() - 1))
        else {
            return core::ptr::null_mut();
        };
        let Some(new_next) = ptr.checked_add(layout.size()) else {
            return core::ptr::null_mut();
        };
        if new_next > *end {
            let pages = (new_next - *end).div_ceil(PAGE);
            if core::arch::wasm32::memory_grow::<0>(pages) == usize::MAX {
                return core::ptr::null_mut();
            }
            *end += pages * PAGE;
        }
        *next = new_next;
        ptr as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOC: BumpAlloc = BumpAlloc {
    state: UnsafeCell::new((0, 0)),
};

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // Trap: the host reports an evaluation error. No message plumbing — a
    // guest that wants diagnostics should avoid panicking.
    core::arch::wasm32::unreachable()
}
