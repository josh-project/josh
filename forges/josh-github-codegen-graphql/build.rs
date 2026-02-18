use anyhow::{Context, Result};
use graphql_client_codegen::normalization::Normalization;
use graphql_client_codegen::{
    generate_module_token_stream_from_string, CodegenMode, GraphQLClientCodegenOptions,
};
use serde::Deserialize;

use std::path::Path;
use std::{env, fs};

#[derive(Debug, Deserialize)]
struct Config {
    graphql: GraphQLConfig,
}

#[derive(Debug, Deserialize)]
struct GraphQLConfig {
    queries: Vec<String>,
    fragments: String,
    #[serde(default)]
    api: Option<GraphQLApiConfig>,
}

#[derive(Debug, Deserialize, Default)]
struct GraphQLApiConfig {
    queries: Vec<String>,
}

fn main() -> Result<()> {
    let manifest = read_manifest()?;

    let files = [
        vec![manifest.graphql.fragments.clone()],
        manifest.graphql.queries.clone(),
    ]
    .concat();
    let concatenated_files = concatenate_graphql_files(&files)?;
    let generated_code = generate_code(&concatenated_files)?;
    write_output(&generated_code, "generated.rs")?;

    if let Some(api) = &manifest.graphql.api {
        if !api.queries.is_empty() {
            let api_concatenated = concatenate_graphql_files(&api.queries)?;
            let api_generated = generate_code(&api_concatenated)?;
            write_output(&api_generated, "generated_api.rs")?;
            for query in &api.queries {
                println!("cargo:rerun-if-changed=src/{}", query);
            }
        }
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/manifest.toml");
    println!("cargo:rerun-if-changed=src/github.graphql");

    for query in &files {
        println!("cargo:rerun-if-changed=src/{}", query);
    }

    Ok(())
}

fn read_manifest() -> Result<Config> {
    let manifest_path = Path::new("src").join("manifest.toml");
    let manifest_content =
        fs::read_to_string(manifest_path).context("Failed to read manifest.toml")?;
    let config: Config =
        toml::from_str(&manifest_content).context("Failed to parse manifest.toml")?;

    Ok(config)
}

fn concatenate_graphql_files(queries: &[String]) -> Result<String> {
    let mut concatenated = String::new();

    for query in queries {
        let query_path = Path::new("src").join(query);
        let query_content = fs::read_to_string(&query_path)
            .with_context(|| format!("Failed to read GraphQL file: {}", query))?;

        concatenated.push_str(&query_content);
        concatenated.push('\n');
    }

    Ok(concatenated)
}

fn make_public_visibility() -> syn::Visibility {
    syn::Visibility::Public(syn::Token![pub](proc_macro2::Span::call_site()))
}

fn generate_code(query: &str) -> Result<String> {
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

fn write_output(generated_code: &str, filename: &str) -> Result<()> {
    let out_dir = env::var_os("OUT_DIR").context("OUT_DIR not set")?;
    let dest_path = Path::new(&out_dir).join(filename);

    fs::write(dest_path, generated_code).context("Failed to write generated code to file")
}
