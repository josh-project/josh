pub struct ParsedSubmoduleEntry {
    pub path: std::path::PathBuf,
    pub url: String,
    pub branch: String,
}

pub fn parse_gitmodules(gitmodules_content: &str) -> anyhow::Result<Vec<ParsedSubmoduleEntry>> {
    use anyhow::Context;
    use gix_submodule::File;

    let submodules = File::from_bytes(gitmodules_content.as_bytes(), None, &Default::default())
        .context("Failed to parse .gitmodules")?;

    let mut entries: Vec<ParsedSubmoduleEntry> = Vec::new();

    for name in submodules.names() {
        // path is required to consider an entry
        if let Ok(path) = submodules.path(name) {
            let path = std::path::PathBuf::from(path.to_string());

            let url = submodules
                .url(name)
                .ok()
                .map(|u| u.to_string())
                .unwrap_or_default();

            // Default branch to "HEAD" if not configured
            let branch = submodules
                .branch(name)
                .ok()
                .and_then(|opt| {
                    opt.map(|b| match b {
                        gix_submodule::config::Branch::CurrentInSuperproject => ".".to_string(),
                        gix_submodule::config::Branch::Name(n) => n.to_string(),
                    })
                })
                .unwrap_or_else(|| "HEAD".to_string());

            entries.push(ParsedSubmoduleEntry { path, url, branch });
        }
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_gitmodules_basic() {
        let content = r#"[submodule "libs/foo"]
	path = libs/foo
	url = https://github.com/example/foo.git
	branch = main

[submodule "libs/bar"]
	path = libs/bar
	url = https://github.com/example/bar.git"#;

        let result = parse_gitmodules(content).unwrap();
        assert_eq!(result.len(), 2);

        assert_eq!(result[0].path, PathBuf::from("libs/foo"));
        assert_eq!(result[0].url, "https://github.com/example/foo.git");
        assert_eq!(result[0].branch, "main");

        assert_eq!(result[1].path, PathBuf::from("libs/bar"));
        assert_eq!(result[1].url, "https://github.com/example/bar.git");
        assert_eq!(result[1].branch, "HEAD"); // default
    }

    #[test]
    fn test_parse_gitmodules_empty() {
        let content = "";
        let result = parse_gitmodules(content).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_parse_gitmodules_invalid() {
        // gix-config parses content without a section header leniently,
        // so this yields no submodule entries rather than an error.
        let content = "invalid gitmodules content";
        let result = parse_gitmodules(content).unwrap();
        assert_eq!(result.len(), 0);
    }
}
