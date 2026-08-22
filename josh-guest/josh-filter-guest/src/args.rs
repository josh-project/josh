//! Invocation arguments.

use crate::abi;
use alloc::string::String;
use alloc::vec::Vec;

/// The `=arg,...` list from the invoking filter spec; empty if none were
/// given. Args containing a newline are dropped by the host (the ABI encodes
/// the list newline-joined).
pub fn args() -> Vec<String> {
    crate::split_list(crate::decode_string(unsafe { abi::invocation_args() }))
}
