#[cfg(feature = "incubating")]
pub struct ParsedSubmoduleEntry {
    pub path: std::path::PathBuf,
    pub url: String,
    pub branch: String,
}

#[cfg(feature = "incubating")]
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

#[cfg(feature = "incubating")]
pub fn update_gitmodules(
    gitmodules_content: &str,
    entry: &ParsedSubmoduleEntry,
) -> anyhow::Result<String> {
    use anyhow::Context;
    use gix_config::File as ConfigFile;
    use gix_submodule::File as SubmoduleFile;

    // Parse the existing gitmodules content using gix_submodule
    let submodule_file = SubmoduleFile::from_bytes(
        gitmodules_content.as_bytes(),
        None,
        &ConfigFile::new(gix_config::file::Metadata::default()),
    )
    .context("Failed to parse .gitmodules")?;

    // Get the underlying config file to modify it
    let mut config = submodule_file.config().clone();

    // Find the existing submodule by matching the path
    let mut existing_submodule_name = None;
    for name in submodule_file.names() {
        if let Ok(path) = submodule_file.path(name) {
            if path.to_string() == entry.path.to_string_lossy() {
                existing_submodule_name = Some(name.to_string());
                break;
            }
        }
    }

    let submodule_name = if let Some(name) = existing_submodule_name {
        // Use the existing submodule name
        name
    } else {
        // Create a new submodule name from path (fallback)
        entry.path.to_string_lossy().replace('/', "_")
    };

    // Create or update the submodule section
    let mut section = config
        .section_mut_or_create_new("submodule", Some(submodule_name.as_str().into()))
        .context("Failed to create submodule section")?;

    // Remove existing keys if they exist to avoid duplicates
    use gix_config::parse::section::ValueName;

    // Remove all existing values for these keys
    while section.remove("path").is_some() {}
    while section.remove("url").is_some() {}
    while section.remove("branch").is_some() {}

    // Set the submodule properties using push method
    let path_key: ValueName = "path".try_into()?;
    let url_key: ValueName = "url".try_into()?;
    let branch_key: ValueName = "branch".try_into()?;

    section.push(path_key, Some(entry.path.to_string_lossy().as_ref().into()));
    section.push(url_key, Some(entry.url.as_str().into()));
    if entry.branch != "HEAD" {
        section.push(branch_key, Some(entry.branch.as_str().into()));
    }

    // Write the updated config back to string
    let mut output = Vec::new();
    config
        .write_to(&mut output)
        .context("Failed to write gitmodules")?;

    String::from_utf8(output).context("Invalid UTF-8 in gitmodules")
}

#[cfg(feature = "incubating")]
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
        let content = "invalid gitmodules content";
        let result = parse_gitmodules(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_gitmodules_add_new() {
        let content = "";
        let entry = ParsedSubmoduleEntry {
            path: PathBuf::from("libs/foo"),
            url: "https://github.com/example/foo.git".to_string(),
            branch: "main".to_string(),
        };

        let result = update_gitmodules(content, &entry).unwrap();

        let expected = r#"[submodule "libs_foo"]
	path = libs/foo
	url = https://github.com/example/foo.git
	branch = main
"#;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_update_gitmodules_add_new_with_default_branch() {
        let content = "";
        let entry = ParsedSubmoduleEntry {
            path: PathBuf::from("libs/bar"),
            url: "https://github.com/example/bar.git".to_string(),
            branch: "HEAD".to_string(),
        };

        let result = update_gitmodules(content, &entry).unwrap();

        let expected = r#"[submodule "libs_bar"]
	path = libs/bar
	url = https://github.com/example/bar.git
"#;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_update_gitmodules_update_existing() {
        let content = r#"[submodule "existing_foo"]
	path = libs/foo
	url = https://github.com/example/old-foo.git
	branch = old-branch"#;

        let entry = ParsedSubmoduleEntry {
            path: PathBuf::from("libs/foo"),
            url: "https://github.com/example/new-foo.git".to_string(),
            branch: "new-branch".to_string(),
        };

        let result = update_gitmodules(content, &entry).unwrap();

        // Should update existing values, not append duplicates
        let expected = r#"[submodule "existing_foo"]
	path = libs/foo
	url = https://github.com/example/new-foo.git
	branch = new-branch
"#;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_update_gitmodules_update_existing_with_default_branch() {
        let content = r#"[submodule "existing_bar"]
	path = libs/bar
	url = https://github.com/example/old-bar.git
	branch = old-branch"#;

        let entry = ParsedSubmoduleEntry {
            path: PathBuf::from("libs/bar"),
            url: "https://github.com/example/new-bar.git".to_string(),
            branch: "HEAD".to_string(),
        };

        let result = update_gitmodules(content, &entry).unwrap();

        // Should update existing values and remove branch when it's HEAD
        let expected = r#"[submodule "existing_bar"]
	path = libs/bar
	url = https://github.com/example/new-bar.git
"#;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_update_gitmodules_multiple_submodules() {
        let content = r#"[submodule "existing_foo"]
	path = libs/foo
	url = https://github.com/example/foo.git

[submodule "existing_bar"]
	path = libs/bar
	url = https://github.com/example/bar.git"#;

        let entry = ParsedSubmoduleEntry {
            path: PathBuf::from("libs/baz"),
            url: "https://github.com/example/baz.git".to_string(),
            branch: "develop".to_string(),
        };

        let result = update_gitmodules(content, &entry).unwrap();

        // The gix-config API appends new sections at the end
        let expected = r#"[submodule "existing_foo"]
	path = libs/foo
	url = https://github.com/example/foo.git

[submodule "existing_bar"]
	path = libs/bar
	url = https://github.com/example/bar.git
[submodule "libs_baz"]
	path = libs/baz
	url = https://github.com/example/baz.git
	branch = develop
"#;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_update_gitmodules_path_with_slashes() {
        let content = "";
        let entry = ParsedSubmoduleEntry {
            path: PathBuf::from("deep/nested/path/submodule"),
            url: "https://github.com/example/deep-submodule.git".to_string(),
            branch: "main".to_string(),
        };

        let result = update_gitmodules(content, &entry).unwrap();

        let expected = r#"[submodule "deep_nested_path_submodule"]
	path = deep/nested/path/submodule
	url = https://github.com/example/deep-submodule.git
	branch = main
"#;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_update_gitmodules_invalid_content() {
        let content = "invalid gitmodules content";
        let entry = ParsedSubmoduleEntry {
            path: PathBuf::from("libs/foo"),
            url: "https://github.com/example/foo.git".to_string(),
            branch: "main".to_string(),
        };

        let result = update_gitmodules(content, &entry);
        assert!(result.is_err());
    }
}
