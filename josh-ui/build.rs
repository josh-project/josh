use dircpy::*;
use npm_rs::*;

fn main() {
    let exit_status = NpmEnv::default()
        .with_env("PUBLIC_URL", "/~/ui")
        .init_env()
        .install(None)
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
