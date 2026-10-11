//! Typed wrapper around the host's handle-based filter builder.

use crate::abi;
use alloc::string::String;
use alloc::vec::Vec;

/// An opaque handle to a filter constructed on the host.
///
/// Handles are only valid within the evaluation that produced them; they are
/// `Copy` because the host keeps them alive for the whole evaluation. All
/// builder methods delegate 1:1 to `josh_filter::Filter` on the host, so
/// chaining/composition semantics (including canonicalization) match native
/// filter specs exactly.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Filter(u32);

/// The nop filter (`:/`) — the usual starting point for builder chains.
pub fn nop() -> Filter {
    Filter(unsafe { abi::nop() })
}

/// The empty filter (projects everything away).
pub fn empty() -> Filter {
    Filter(unsafe { abi::empty() })
}

/// Compose filters (the `:[a,b,...]` overlay).
pub fn compose<I>(filters: I) -> Filter
where
    I: IntoIterator<Item = Filter>,
{
    let handles: Vec<u32> = filters.into_iter().map(|f| f.0).collect();
    Filter(unsafe { abi::compose(handles.as_ptr(), handles.len() as u32) })
}

impl Filter {
    /// The raw ABI handle; what `josh_run` must return.
    pub fn handle(self) -> u32 {
        self.0
    }

    /// Wrap a raw ABI handle obtained through [`crate::abi`].
    pub fn from_handle(handle: u32) -> Filter {
        Filter(handle)
    }

    pub fn chain(self, other: Filter) -> Filter {
        Filter(unsafe { abi::chain(self.0, other.0) })
    }

    /// Select the subtree at `path` (`:/path`).
    pub fn subdir(self, path: &str) -> Filter {
        Filter(unsafe { abi::subdir(self.0, path.as_ptr(), path.len() as u32) })
    }

    /// Mount the result at `path` (`:prefix=path`).
    pub fn prefix(self, path: &str) -> Filter {
        Filter(unsafe { abi::prefix(self.0, path.as_ptr(), path.len() as u32) })
    }

    /// Select a single file (`::path`).
    pub fn file(self, path: &str) -> Filter {
        Filter(unsafe { abi::file(self.0, path.as_ptr(), path.len() as u32) })
    }

    /// Rename `src` to `dst`. NOTE the argument order: the DESTINATION comes
    /// first — `rename("new.txt", "old.txt")` maps `old.txt` to `new.txt`,
    /// matching `josh_filter::Filter::rename`.
    pub fn rename(self, dst: &str, src: &str) -> Filter {
        Filter(unsafe {
            abi::rename(
                self.0,
                dst.as_ptr(),
                dst.len() as u32,
                src.as_ptr(),
                src.len() as u32,
            )
        })
    }

    /// Select files matching a glob (`::*.rs`). An invalid glob traps and
    /// fails the whole evaluation.
    pub fn pattern(self, glob: &str) -> Filter {
        Filter(unsafe { abi::pattern(self.0, glob.as_ptr(), glob.len() as u32) })
    }

    pub fn linear(self) -> Filter {
        Filter(unsafe { abi::linear(self.0) })
    }

    pub fn workspace(self, path: &str) -> Filter {
        Filter(unsafe { abi::workspace(self.0, path.as_ptr(), path.len() as u32) })
    }

    pub fn stored(self, path: &str) -> Filter {
        Filter(unsafe { abi::stored(self.0, path.as_ptr(), path.len() as u32) })
    }

    pub fn author(self, name: &str, email: &str) -> Filter {
        Filter(unsafe {
            abi::author(
                self.0,
                name.as_ptr(),
                name.len() as u32,
                email.as_ptr(),
                email.len() as u32,
            )
        })
    }

    pub fn committer(self, name: &str, email: &str) -> Filter {
        Filter(unsafe {
            abi::committer(
                self.0,
                name.as_ptr(),
                name.len() as u32,
                email.as_ptr(),
                email.len() as u32,
            )
        })
    }

    pub fn message(self, message: &str) -> Filter {
        Filter(unsafe { abi::message(self.0, message.as_ptr(), message.len() as u32) })
    }

    pub fn unsign(self) -> Filter {
        Filter(unsafe { abi::unsign(self.0) })
    }

    pub fn prune_trivial_merge(self) -> Filter {
        Filter(unsafe { abi::prune_trivial_merge(self.0) })
    }

    pub fn hook(self, hook: &str) -> Filter {
        Filter(unsafe { abi::hook(self.0, hook.as_ptr(), hook.len() as u32) })
    }

    pub fn with_meta(self, key: &str, value: &str) -> Filter {
        Filter(unsafe {
            abi::with_meta(
                self.0,
                key.as_ptr(),
                key.len() as u32,
                value.as_ptr(),
                value.len() as u32,
            )
        })
    }

    /// Insert a synthetic file. Traps (fails the evaluation) on an invalid
    /// path or content, like the host builder.
    pub fn insert(self, path: &str, content: &str) -> Filter {
        Filter(unsafe {
            abi::insert(
                self.0,
                path.as_ptr(),
                path.len() as u32,
                content.as_ptr(),
                content.len() as u32,
            )
        })
    }

    /// Experimental-gated on the host; traps when the gate is disabled.
    pub fn treeid(self, path: &str, sub: Filter) -> Filter {
        Filter(unsafe { abi::treeid(self.0, path.as_ptr(), path.len() as u32, sub.0) })
    }

    /// A nested wasm filter op (`:!path=arg,...[ctx]`). Args must not
    /// contain newlines (the ABI joins them with `\n`). Experimental-gated
    /// on the host; note that evaluation depth is capped.
    pub fn wasm(self, path: &str, args: &[&str], ctx: Filter) -> Filter {
        let args: String = args.join("\n");
        Filter(unsafe {
            abi::wasm(
                self.0,
                path.as_ptr(),
                path.len() as u32,
                args.as_ptr(),
                args.len() as u32,
                ctx.0,
            )
        })
    }

    pub fn peel(self) -> Filter {
        Filter(unsafe { abi::peel(self.0) })
    }

    pub fn is_nop(self) -> bool {
        (unsafe { abi::is_nop(self.0) }) != 0
    }
}
