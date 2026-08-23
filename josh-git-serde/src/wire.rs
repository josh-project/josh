use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};

use crate::error::SerdeGitError;

pub(crate) const FILENAME_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'!')
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'%')
    .add(b'&')
    .add(b'\'')
    .add(b'(')
    .add(b')')
    .add(b'*')
    .add(b'+')
    .add(b',')
    .add(b'.')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}')
    .add(b'~');

/// Map an arbitrary key to a valid git tree entry name. The escape set is
/// deliberately wider than git's forbidden bytes so encoded names are safe in
/// josh filters too.
pub fn encode_key(key: &str) -> String {
    utf8_percent_encode(key, FILENAME_SET).to_string()
}

/// Inverse of [`encode_key`], and strictly so: the decoded key must re-encode
/// to exactly its input. Plain percent-decoding accepts any `%XX`, so distinct
/// filenames could collapse into one key. Re-encoding instead of hand-checking
/// escapes keeps the encoder's set as the single source of truth.
pub(crate) fn decode_key(encoded_key: &str) -> Result<String, SerdeGitError> {
    let err = || SerdeGitError("invalid key encoding".to_string());
    let decoded = percent_decode_str(encoded_key)
        .decode_utf8()
        .map_err(|_| err())?
        .into_owned();

    if utf8_percent_encode(&decoded, FILENAME_SET).to_string() != encoded_key {
        return Err(err());
    }

    Ok(decoded)
}
