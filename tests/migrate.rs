
extern crate tempdir;
extern crate git2;
extern crate centralgithook;
use centralgithook::migrate;
use self::tempdir::TempDir;
use std::path::{Path, PathBuf};
use std::fs::File;
use std::fs;
use std::str;
use std::io::Write;
use std::io::Read;

fn _oid_to_sha1(oid: &[u8]) -> String {
    oid.iter().fold(Vec::new(), |mut acc, x| {
        acc.push(format!("{0:>02x}", x)); acc
    }).concat()
}


#[test]
fn test_commit_to_central() {
    let td = TempDir::new("play").expect("folder play should be created");
    let temp_path = td.path();
    let central_repo_dir = temp_path.join("central");
    let central_repo_path = &Path::new(&central_repo_dir);
    println!("    ");
    println!("    ########### SETUP: create central repository ########### ");
    let central_repo = create_repository(&central_repo_dir);
    commit_files(&central_repo,
            &central_repo_path,
            &vec!["modules/moduleA/added_in_central.txt",
            "modules/moduleB/added_in_central.txt",
            "modules/moduleC/added_in_central.txt"]);

    println!("    ########### SETUP: create module repositories ########### ");

    let module_names = vec!["moduleA", "moduleB", "moduleC"];
    let _module_repos: Vec<git2::Repository> = module_names
        .iter()
        .map(|m| {
                let x = temp_path.join(&m);
                let repo = create_repository(&x);
                // point remote to central
                let ref_name = "modules/".to_string() + &m;
                central_repo.remote(&ref_name, &m).expect("remote as to be added");
                commit_files(&repo, &Path::new(&x), &vec!["test/a.txt", "b.txt", "c.txt"]);
                migrate::call_command("git", &["checkout","-b", "not_needed"], Some(&x), None);
                repo
                })
    .collect();

    let remote_url = move |module_path: &str| -> String {
        temp_path.join(&module_path).to_string_lossy().to_string()
    };
    let make_sure_module_exists = move |module_name: &str| -> Result<(), git2::Error> {
        let repo_dir = temp_path.join(&module_name);
        if git2::Repository::open(&repo_dir).is_err() {
            //TODO only create repository if this is intended
            println!("+++++++++ {:?} did not exist => creating it", repo_dir);
            let _ = create_repository(&repo_dir);
        }
        Ok(())
    };
    let fp = central_repo_dir.clone().join(".git").join("refs").join("heads").join("master");
    let mut f = File::open(&fp).expect("open file master");
    let mut s = String::new();
    f.read_to_string(&mut s).expect("read file shoud work");
    println!("master file in {:?}: {}", central_repo_dir, s);

    let h = central_repo.head().expect("get head of central repo");
    let t = h.target().expect("get oid of head reference");
    let central_repo_head_oid = t.as_bytes();

    let central_head_sha1 = _oid_to_sha1(&central_repo_head_oid);

    println!("    ########### START: calling central_submit ########### ");
    let td = TempDir::new("scratch").expect("folder scratch should be created");
    let scratch_dir = td.path();

    let _t = temp_path.join("moduleA").join("added_in_central.txt");
    println!("file in question:{:?}", _t);
    assert!(!temp_path.join("moduleA").join("added_in_central.txt").exists());
    migrate::central_submit("central",
            &central_head_sha1[..],
            &remote_url,
            &make_sure_module_exists,
            "central",
            &central_repo_path,
            &scratch_dir).expect("call central_submit");
    for m in module_names {
        let x = temp_path.join(&m);
        migrate::call_command("git", &["checkout", "master"], Some(&x), None);
        assert!(temp_path.join(m).join("added_in_central.txt").exists());
    }
}

fn commit_file(repo: &git2::Repository, file: &Path, parents: &[&git2::Commit]) -> git2::Oid {
    let mut index = repo.index().expect("get index of repo");
    index.add_path(file).expect("file should be added");
    index.write().expect("write index");
    let tree_id = index.write_tree().expect("got tree_id");
    let tree = repo.find_tree(tree_id).expect("got tree");
    let sig = git2::Signature::now("foo", "bar").expect("created signature");
    repo.commit(Some("HEAD"),
            &sig,
            &sig,
            &format!("commit for {:?}", &file.as_os_str()),
            &tree,
            &parents)
        .expect("commit to repo")
}

fn commit_files(repo: &git2::Repository, pb: &Path, content: &Vec<&str>) {
    let mut parent_commit = None;
    for file_name in content {
        let foo_file = pb.join(file_name);
        create_dummy_file(&foo_file);
        let oid = match parent_commit {
            Some(parent) => commit_file(&repo, &Path::new(file_name), &[&parent]),
                None => commit_file(&repo, &Path::new(file_name), &[]),
        };
        parent_commit = repo.find_commit(oid).ok();
    }
}

fn create_repository(temp: &Path) -> git2::Repository {
    let repo = git2::Repository::init(temp).expect("init should succeed");

    let path = migrate::get_repo_path(&repo).to_path_buf();
    println!("Initialized empty Git repository in {}", path.display());
    repo
}

fn create_dummy_file(f: &PathBuf) {
    let parent_dir = f.as_path().parent().expect("need to get parent");
    fs::create_dir_all(parent_dir).expect("create directories");

    let mut file = File::create(&f.as_path()).expect("create file");
    file.write_all("test content".as_bytes()).expect("write to file");
}
