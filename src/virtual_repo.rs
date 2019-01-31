use super::*;
use git2::Oid;
use std::env;
use std::env::current_exe;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;

pub fn setup_tmp_repo(scratch_dir: &Path) {
    let shell = Shell {
        cwd: scratch_dir.to_path_buf(),
    };

    let ce = current_exe().expect("can't find path to exe");
    shell.command("rm -Rf hooks");
    shell.command("mkdir hooks");
    symlink(ce, scratch_dir.join("hooks").join("update")).expect("can't symlink update hook");

    shell.command("git config http.receivepack true");
    shell.command("rm -Rf refs/for");
    shell.command("rm -Rf refs/drafts");
}

pub fn update_hook(refname: &str, _old: &str, new: &str) -> i32 {
    let scratch = scratch::new(&Path::new(&env::var("GIT_DIR").expect("GIT_DIR not set")));

    let username = env::var("GRIB_USERNAME").expect("GRIB_USERNAME not set");
    let password = env::var("GRIB_PASSWORD").expect("GRIB_PASSWORD not set");
    let remote_url = env::var("GRIB_REMOTE").expect("GRIB_REMOTE not set");

    let new_oid = if let Ok(viewstr) = env::var("GRIB_VIEWSTR") {
        let viewobj = build_view(&viewstr);
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

        let namespaced_repo = git2::Repository::open_from_env().unwrap();
        let old = namespaced_repo.refname_to_id(&without_refs_for).unwrap();

        debug!("=== processed_old {}", old);

        match scratch::unapply_view(
            &scratch,
            central_head,
            &*viewobj,
            old,
            Oid::from_str(new).expect("can't parse new OID"),
        ) {
            UnapplyView::Done(rewritten) => rewritten,
            _ => return 1,
        }
    } else {
        debug!("=== return direct");
        git2::Oid::from_str(new).expect("can't parse new Oid")
    };

    scratch.set_head_detached(new_oid).expect("can't set head");

    debug!("=== pushing {}:{}", "HEAD", refname);
    base_repo::push_head_url(scratch.path(), &refname, &remote_url, &username, &password);

    return 0;
}
