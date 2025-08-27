use dircpy::*;
use npm_rs::*;

fn main() {
    let exit_status = NpmEnv::default()
        .with_env("PUBLIC_URL", "/~/ui")
        .with_env("TSC_COMPILE_ON_ERROR", "true")
        .init_env()
        .install(Some(&["--legacy-peer-deps"]))
        .run("build")
        .exec();

    assert!(exit_status.is_ok());
    assert!(exit_status.unwrap().success());

    let this_dir = env!("CARGO_MANIFEST_DIR");

    CopyBuilder::new(
        format!("{}/build", this_dir),
        format!("{}/../static", this_dir),
    )
    .overwrite(true)
    .run()
    .unwrap();
}
