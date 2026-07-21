use git2::{Oid, Repository};

pub fn load_blob_content(repo: &Repository, oid: Oid) -> String {
    match repo.find_blob(oid) {
        Ok(blob) => {
            let content = blob.content();
            String::from_utf8(content.to_vec()).unwrap_or_else(|_| "<Binary file>".to_string())
        }
        Err(e) => format!("Error loading file: {}", e),
    }
}
