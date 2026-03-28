#![allow(unused_imports)]
#![allow(clippy::needless_borrow)]

use chrono::{TimeZone, Utc};
use url::Url;

pub type Id = String;
pub type GitObjectId = String;
pub type NodeId = String;
pub type Uri = Url;
pub type DateTime = chrono::DateTime<Utc>;

include!(concat!(env!("OUT_DIR"), "/generated.rs"));
