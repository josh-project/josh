//! serde serialization of Rust values into git tree objects.
//!
//! A `Serialize` value becomes a git tree: struct fields and map keys become
//! child entries at the top level of their tree, scalars become blobs, and an
//! enum becomes a tree with a single entry keyed by the variant name. No type
//! information is carried in-band: the tree's shape is data only, and the
//! decoding schema alone decides what it means. A `None` in entry position
//! (struct field, map value) is omitted entirely; `None` at the root has no
//! representation and is an error, as are sequences, tuples, and tuple
//! variants. The conversion is split:
//!
//! - pure in-memory: [`GitValue`] model + serde ser/de (`to_value`/`from_value`)
//! - object store: `GitValue` <-> tree OIDs over `gix_object::Find`/`Write`
//!   (`to_tree_oid`/`from_tree_oid`)
//!
//! ```
//! use josh_git_serde::{from_value, to_value};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize, PartialEq, Debug)]
//! struct Config {
//!     name: String,
//!     retries: u32,
//! }
//!
//! let config = Config { name: "josh".into(), retries: 3 };
//! let value = to_value(&config).unwrap();
//! assert_eq!(from_value::<Config>(&value).unwrap(), config);
//! ```

mod de;
mod error;
mod ser;
mod store;
mod value;
pub(crate) mod wire;

pub use de::from_value;
pub use error::SerdeGitError;
pub use ser::to_value;
pub use store::{from_tree_oid, to_tree_oid};
pub use value::GitValue;
pub use wire::encode_key;
