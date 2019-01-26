use super::*;
use git2::Oid;
use std::env;
use std::env::current_exe;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;

pub fn setup_tmp_repo(scratch_dir: &Path, view: &str) -> PathBuf {
    let path = thread_local_temp_dir();

    let root = match view {
        "." => "refs".to_string(),
        view => view_ref_root(&view),
    };

    debug!("setup_tmp_repo, root: {:?}", &root);
    let shell = Shell {
        cwd: path.to_path_buf(),
    };

    let ce = current_exe().expect("can't find path to exe");
    shell.command("mkdir hooks");
    symlink(ce, path.join("hooks").join("update")).expect("can't symlink update hook");

    shell.command(&format!("cp {:?} {:?}", scratch_dir.join("HEAD"), path));
    shell.command(&format!("cp {:?} {:?}", scratch_dir.join("config"), path));
    symlink(scratch_dir.join(root), path.join("refs")).expect("can't symlink refs");
    shell.command(&format!(
        "cp {:?} {:?}",
        path.join("refs").join("heads").join("master"),
        path.join("HEAD")
    ));
    symlink(scratch_dir.join("objects"), path.join("objects")).expect("can't symlink objects");

    shell.command("git config http.receivepack true");
    shell.command("rm -Rf refs/for");
    shell.command("rm -Rf refs/drafts");
    return path;
}

pub fn update_hook(refname: &str, _old: &str, new: &str) -> i32 {
    let scratch = scratch::new(&Path::new(
        &env::var("GRIB_BR_PATH").expect("GRIB_BR_PATH not set"),
    ));

    let username = env::var("GRIB_USERNAME").expect("GRIB_USERNAME not set");
    let password = env::var("GRIB_PASSWORD").expect("GRIB_PASSWORD not set");
    let remote_url = env::var("GRIB_REMOTE").expect("GRIB_REMOTE not set");
    let viewname = env::var("GRIB_VIEW").expect("GRIB_VIEW not set");

    let r = git2::Repository::open_from_env().unwrap();

    if viewname.starts_with(".") {
        debug!("=== pushing {}:{}", "HEAD", refname);
        debug!("=== return direct");
        r.set_head_detached(git2::Oid::from_str(new).expect("can't parse new Oid"))
            .expect("can't set head");
    } else {
        let view = SubdirView::new(&Path::new(&viewname));

        debug!("=== MORE");

        let without_refs_for = refname.to_owned();
        let without_refs_for = without_refs_for.trim_left_matches("refs/for/");
        let without_refs_for = without_refs_for.trim_left_matches("refs/drafts/");
        let without_refs_for = without_refs_for.trim_left_matches("refs/heads/");

        let without_refs_for = format!("refs/heads/{}", &without_refs_for);

        let central_head = scratch.refname_to_id(&without_refs_for).expect(&format!(
            "no ref: {} ({}) in {:?}",
            &refname,
            &without_refs_for,
            scratch.path()
        ));

        let old = r.refname_to_id(&without_refs_for).unwrap();

        debug!("=== processed_old {}", old);

        match scratch::unapply_view(
            &scratch,
            central_head,
            &view,
            old,
            Oid::from_str(new).expect("can't parse new OID"),
        ) {
            UnapplyView::Done(rewritten) => {
                r.set_head_detached(rewritten)
                    .expect("rewrite: can't detach head");
            }
            _ => return 1,
        };
    }

    base_repo::push_head_url(scratch.path(), &refname, &remote_url, &username, &password);

    return 0;
}
