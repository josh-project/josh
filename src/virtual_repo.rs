use super::*;
use git2::Oid;
use std::env::current_exe;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;

pub fn setup_tmp_repo(
    scratch_dir: &Path,
    view: &str,
    user: &str,
    password: &str,
    remote_url: &str,
) -> PathBuf
{
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

    shell.command(&format!("printf {} > view", view));

    shell.command(&format!("printf {} > orig", scratch_dir.to_string_lossy()));
    shell.command(&format!("printf {} > username", user));
    shell.command(&format!("printf {} > password", password));
    shell.command(&format!("printf {} > remote_url", remote_url));
    shell.command("git config http.receivepack true");
    shell.command("rm -Rf refs/for");
    return path;
}

fn read_repo_info_file(name: &str) -> String
{
    let mut s = String::new();
    File::open(&Path::new(&name))
        .expect(&format!("could not open {} name file", name))
        .read_to_string(&mut s)
        .expect(&format!("could not read {} name", name));
    return s;
}

pub fn update_hook(refname: &str, _old: &str, new: &str) -> i32
{
    let scratch = Scratch::new(&Path::new(&read_repo_info_file("orig")));


    let username = read_repo_info_file("username");
    let password = read_repo_info_file("password");
    let remote_url = read_repo_info_file("remote_url");
    let viewname = read_repo_info_file("view");


    let r = git2::Repository::open_from_env().unwrap();
    let mut remote = r.remote_anonymous(&remote_url).unwrap();

    if viewname.starts_with(".") {
        debug!("=== pushing {}:{}", "HEAD", refname);
        debug!("=== return direct");
        r.set_head_detached(git2::Oid::from_str(new).expect("can't parse new Oid"))
            .expect("can't set head");
    } else {
        let view = SubdirView::new(&Path::new(&viewname));

        debug!("=== MORE");

        let without_refs_for = refname.to_owned();
        let central_head = scratch
            .repo
            .refname_to_id(&without_refs_for)
            .expect(&format!("no ref: {}", &refname));

        let old = r.refname_to_id(&without_refs_for).unwrap();

        debug!("=== processed_old {}", old);

        match scratch.unapply_view(
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

    base_repo::push_head(&refname, remote, &username, &password);

    return 0;
}
