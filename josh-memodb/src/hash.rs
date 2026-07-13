use std::hash::Hasher;
use std::mem::size_of;

/// A hasher that uses an already-uniform key directly as the hash value, avoiding double-hashing.
///
/// Two key shapes are supported: the first 8 bytes of a 20-byte SHA1 (e.g. a git `Oid` key) via
/// [`write`](Hasher::write), or a single pointer-sized integer (e.g. an interned handle) via
/// [`write_usize`](Hasher::write_usize). Anything else panics: the hasher assumes its input is
/// already a cryptographic digest or a unique integer, so mixing it further would be wasted work.
#[derive(Default)]
pub struct PassthroughHasher {
    buffer: [u8; 8],
    done: bool,
}

impl Hasher for PassthroughHasher {
    fn finish(&self) -> u64 {
        if !self.done {
            panic!("not enough data supplied to hasher")
        }

        u64::from_le_bytes(self.buffer)
    }

    // Used for keys whose hash is the value itself, e.g. an interned pointer.
    fn write_usize(&mut self, i: usize) {
        if self.done {
            panic!("hasher data already written");
        }
        self.buffer[..size_of::<usize>()].copy_from_slice(&i.to_le_bytes());
        self.done = true;
    }

    // hyper-specialized: reject everything that's not sha1 length
    fn write(&mut self, bytes: &[u8]) {
        if self.done {
            panic!("hasher data already written");
        }

        // skip length prefix
        // FIXME remove this when https://github.com/rust-lang/rust/issues/96762 is stabilized
        if bytes.len() == size_of::<usize>() {
            return;
        }

        if bytes.len() != 20 {
            panic!("unexpected data length {} in hasher", bytes.len())
        }

        self.buffer.as_mut().copy_from_slice(&bytes[..8]);
        self.done = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_passthrough_hasher() {
        let mut hasher = PassthroughHasher::default();
        let sha1: [u8; 20] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14,
        ];

        hasher.write(&sha1);

        // First 8 bytes as little-endian u64
        let expected = u64::from_le_bytes([0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
        assert_eq!(hasher.finish(), expected);
    }
}
