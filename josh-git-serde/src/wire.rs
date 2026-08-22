use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};

use crate::error::SerdeGitError;

pub(crate) const DATA_FIELD: &str = "data";
pub(crate) const EXTRA_FIELD_VARIANT_BASE: &str = "variant_base";
pub(crate) const MARKER_FIELD_STRUCT: &str = "struct";
pub(crate) const MARKER_FIELD_NEWTYPE_STRUCT: &str = "struct";
pub(crate) const MARKER_FIELD_STRUCT_VARIANT: &str = "struct_variant";
pub(crate) const MARKER_FIELD_UNIT: &str = "unit";
pub(crate) const MARKER_FIELD_UNIT_STRUCT: &str = "unit_struct";
pub(crate) const MARKER_FIELD_UNIT_VARIANT: &str = "unit_variant";
pub(crate) const MARKER_FIELD_NEWTYPE_VARIANT: &str = "newtype_variant";
pub(crate) const MARKER_FIELD_SEQ: &str = "seq";
pub(crate) const MARKER_FIELD_TUPLE: &str = "tuple";
pub(crate) const MARKER_FIELD_TUPLE_STRUCT: &str = "tuple_struct";
pub(crate) const MARKER_FIELD_TUPLE_VARIANT: &str = "tuple_variant";
pub(crate) const MARKER_FIELD_MAP: &str = "map";
pub(crate) const MARKER_FIELD_SOME: &str = "some";
pub(crate) const MARKER_FIELD_NONE: &str = "none";

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

pub(crate) fn encode_key(key: &str) -> String {
    utf8_percent_encode(key, FILENAME_SET).to_string()
}

/// Inverse of [`encode_key`], and strictly so: the decoded key must re-encode
/// to exactly its input. `percent_decode_str` alone is unusable here -- it
/// decodes any `%XX`, so two distinct filenames (`a` and `%61`) would collapse
/// into one key. Re-encoding instead of hand-checking escapes keeps the
/// encoder's set as the single source of truth.
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
