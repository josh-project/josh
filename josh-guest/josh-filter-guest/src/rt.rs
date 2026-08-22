//! Minimal runtime pieces for pure-`no_std` guests (feature `rt`, on by
//! default): a leak-only bump allocator, installed together with an aborting
//! panic handler by invoking [`josh_guest_rt!`](crate::josh_guest_rt) in the
//! guest crate. The lang items are deliberately NOT defined here: this rlib
//! may be shared (via cargo feature unification) with guests that link `std`,
//! which brings its own allocator and panic handler.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;

const PAGE: usize = 65536;

/// Bump allocator over pages appended to the end of linear memory via
/// `memory.grow`. `dealloc` is a no-op: the instance is discarded right
/// after `josh_run`, so everything leaks by design.
pub struct BumpAlloc {
    /// `(next free byte, end of owned region)`; `(0, 0)` = uninitialized.
    state: UnsafeCell<(usize, usize)>,
}

impl BumpAlloc {
    pub const fn new() -> BumpAlloc {
        BumpAlloc {
            state: UnsafeCell::new((0, 0)),
        }
    }
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

/// Install the `no_std` guest runtime: the [`BumpAlloc`] global allocator
/// and an aborting panic handler (a panic traps, which the host reports as
/// an evaluation error — no message plumbing). Invoke once at the top level
/// of a pure-`no_std` guest crate; guests that link `std` (e.g. an
/// interpreter build) must NOT invoke this and should depend on the SDK with
/// `default-features = false`.
#[macro_export]
macro_rules! josh_guest_rt {
    () => {
        #[global_allocator]
        static __JOSH_GUEST_ALLOC: $crate::rt::BumpAlloc = $crate::rt::BumpAlloc::new();

        #[panic_handler]
        fn __josh_guest_panic(_info: &::core::panic::PanicInfo) -> ! {
            ::core::arch::wasm32::unreachable()
        }
    };
}
