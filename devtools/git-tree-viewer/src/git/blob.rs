pub fn load_blob_content(repo: &gix::Repository, oid: gix::ObjectId) -> String {
    match repo.find_blob(oid) {
        Ok(blob) => std::str::from_utf8(&blob.data)
            .map(str::to_owned)
            .unwrap_or_else(|_| "<Binary file>".to_string()),
        Err(e) => format!("Error loading file: {e}"),
    }
}
