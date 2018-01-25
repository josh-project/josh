use git2::Oid;
use std::env::current_exe;
use std::fs::File;
use std::os::unix::fs::symlink;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use super::*;

pub fn setup_tmp_repo(scratch_dir: &Path, view: Option<&str>) -> PathBuf
{
    let path = thread_local_temp_dir();

    let root = match view {
        Some(view) => view_ref_root(&view),
        None => "refs".to_string(),
    };

    debug!("setup_tmp_repo, root: {:?}", &root);
    let shell = Shell { cwd: path.to_path_buf() };

    let ce = current_exe().expect("can't find path to exe");
    shell.command("mkdir hooks");
    symlink(ce, path.join("hooks").join("update")).expect("can't symlink update hook");

    shell.command(&format!("cp {:?} {:?}", scratch_dir.join("HEAD"), path));
    shell.command(&format!("cp {:?} {:?}", scratch_dir.join("config"), path));
    symlink(scratch_dir.join(root), path.join("refs")).expect("can't symlink refs");
    symlink(scratch_dir.join("objects"), path.join("objects")).expect("can't symlink objects");

    shell.command(&format!("printf {} > view",
                           match view {
                               Some(view) => view,
                               None => ".",
                           }));

    shell.command(&format!("printf {} > orig", scratch_dir.to_string_lossy()));
    return path;
}

pub fn update_hook(refname: &str, old: &str, new: &str) -> i32
{
    let scratch = {
        let mut s = String::new();
        File::open(&Path::new("orig"))
            .expect("could not open orig name file")
            .read_to_string(&mut s)
            .expect("could not read orig name");


        let scratch_dir = Path::new(&s);
        let scratch = Scratch::new(&scratch_dir);
        scratch
    };


    let view = {
        let mut s = String::new();
        File::open(&Path::new("view"))
            .expect("could not open view name file")
            .read_to_string(&mut s)
            .expect("could not read view name");

        if s.starts_with(".") {
            return 0;
        }
        let view = SubdirView::new(&Path::new(&s));
        view
    };

    let central_head = scratch.repo.refname_to_id(&refname).expect("no ref: master");


    match scratch.unapply_view(central_head,
                               &view,
                               Oid::from_str(old).expect("can't parse old OID"),
                               Oid::from_str(new).expect("can't parse new OID")) {

        UnapplyView::Done(rewritten) => {
            scratch.repo
                .reference(&refname, rewritten, true, "unapply_view")
                .expect("can't create new reference");
        }
        _ => return 1,
    };

    return 0;
}
