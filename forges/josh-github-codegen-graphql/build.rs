use anyhow::Context;
use graphql_client_codegen::normalization::Normalization;
use graphql_client_codegen::{
    generate_module_token_stream_from_string, CodegenMode, GraphQLClientCodegenOptions,
};
use serde::Deserialize;

use std::collections::BTreeMap;
use std::path::Path;
use std::{env, fs};

#[derive(Debug, Deserialize)]
struct Config {
    queries: BTreeMap<String, QueryConfig>,
}

#[derive(Debug, Deserialize, Default)]
struct QueryConfig {
    #[serde(default)]
    fragments: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    let manifest = read_manifest()?;

    let mut all_generated = String::new();

    for (name, query_config) in &manifest.queries {
        let mut graphql_doc = String::new();

        for fragment in &query_config.fragments {
            let fragment_path = Path::new("src/fragments").join(format!("{}.graphql", fragment));
            let content = fs::read_to_string(&fragment_path)
                .with_context(|| format!("Failed to read fragment: {}", fragment))?;
            graphql_doc.push_str(&content);
            graphql_doc.push('\n');
        }

        let query_path = Path::new("src").join(format!("{}.graphql", name));
        let content = fs::read_to_string(&query_path)
            .with_context(|| format!("Failed to read query: {}", name))?;
        graphql_doc.push_str(&content);
        graphql_doc.push('\n');

        let generated = generate_code(&graphql_doc)?;
        all_generated.push_str(&generated);
        all_generated.push('\n');
    }

    write_output(&all_generated, "generated.rs")?;

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/manifest.toml");
    println!("cargo:rerun-if-changed=src/github.graphql");
    println!("cargo:rerun-if-changed=src/fragments/");

    for name in manifest.queries.keys() {
        println!("cargo:rerun-if-changed=src/{}.graphql", name);
    }

    Ok(())
}

fn read_manifest() -> anyhow::Result<Config> {
    let manifest_path = Path::new("src").join("manifest.toml");
    let manifest_content =
        fs::read_to_string(manifest_path).context("Failed to read manifest.toml")?;
    let config: Config =
        toml::from_str(&manifest_content).context("Failed to parse manifest.toml")?;

    Ok(config)
}

fn make_public_visibility() -> syn::Visibility {
    syn::Visibility::Public(syn::Token![pub](proc_macro2::Span::call_site()))
}

fn generate_code(query: &str) -> anyhow::Result<String> {
    let mut options = GraphQLClientCodegenOptions::new(CodegenMode::Cli);
    options.set_module_visibility(make_public_visibility());
    options.set_response_derives("Debug".to_string());
    options.set_normalization(Normalization::Rust);

    let schema_path = Path::new("src").join("github.graphql");

    let token_stream = generate_module_token_stream_from_string(query, &schema_path, options)
        .map_err(|e| anyhow::anyhow!(e))?;

    let file = syn::parse2::<syn::File>(token_stream)?;
    let formatted = prettyplease::unparse(&file);

    Ok(formatted)
}

fn write_output(generated_code: &str, filename: &str) -> anyhow::Result<()> {
    let out_dir = env::var_os("OUT_DIR").context("OUT_DIR not set")?;
    let dest_path = Path::new(&out_dir).join(filename);

    fs::write(dest_path, generated_code).context("Failed to write generated code to file")
}
