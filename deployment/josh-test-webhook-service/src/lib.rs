use bincode::{Decode, Encode};

use std::collections::HashMap;

#[derive(Encode, Decode, Clone)]
pub struct WebhookPayload {
    pub body: Vec<u8>,

    // Secrets are removed, so there's no secrets passed in this value
    pub headers: HashMap<String, String>,
}
