
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
use centralgithook::migrate::RepoHost;

fn _oid_to_sha1(oid: &[u8]) -> String {
    oid.iter().fold(Vec::new(), |mut acc, x| {
        acc.push(format!("{0:>02x}", x)); acc
    }).concat()
}


struct TestHost {
    td: TempDir
}

impl TestHost {
    fn new() -> Self {
        TestHost {
            td: TempDir::new("test_host").expect("folder test_host should be created")
        }
    }
}

fn empty_commit(repo: &git2::Repository) {
    let sig = git2::Signature::now("foo", "bar").expect("created signature");
    let commit = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        "initial",
        &repo.find_tree(repo.treebuilder(None).expect("cannot create empty tree")
             .write().expect("cannot write empty tree")).expect("cannot find empty tree"),
        &[]

    ).expect("cannot commit empty");
    repo.branch("master",&repo.find_commit(commit).expect("cannot find empty commit"),true);
}

impl migrate::RepoHost for TestHost {
    fn create_project(&self, module: &str) -> Result<(), git2::Error> {
        let repo_dir = self.td.path().join(&Path::new(module));
        println!("TestHost: create_project {} in {:?}", module, repo_dir);
        let repo = git2::Repository::init_bare(&repo_dir).expect("TestHost: init_bare failed");
        empty_commit(&repo);
        Ok(())
    }

    fn remote_url(&self, module_path: &str) -> String {
        self.td.path().join(&module_path).to_string_lossy().to_string()
    }
}

#[test]
fn test_commit_to_central() {
    let host = TestHost::new();
    let td_play = TempDir::new("play").expect("folder play should be created");
    let central_repo_path = td_play.path().join("central");
    println!("    ");
    println!("    ########### SETUP: create central repository ########### ");
    host.create_project("central").expect("error: create_project");
    let central_repo = create_repository(&central_repo_path);
    migrate::call_command("git", &["status"], Some(&central_repo_path));
    commit_files(&central_repo,
            &central_repo_path,
            &vec!["modules/moduleA/added_in_central.txt",
            "modules/moduleB/added_in_central.txt",
            "modules/moduleC/added_in_central.txt"]);

    println!("    ########### SETUP: create module repositories ########### ");

    let module_names = vec!["moduleA", "moduleB", "moduleC"];
    // let _module_repos: Vec<git2::Repository> = module_names
    //     .iter()
    //     .map(|m| {
    //             let x = td.path().join(&m);
    //             let repo = create_repository(&x);
    //             // point remote to central
    //             let ref_name = "modules/".to_string() + &m;
    //             central_repo.remote(&ref_name, &m).expect("remote as to be added");
    //             commit_files(&repo, &Path::new(&x), &vec!["test/a.txt", "b.txt", "c.txt"]);
    //             migrate::call_command("git", &["checkout","-b", "not_needed"], Some(&x));
    //             repo
    //             })
    // .collect();

    let fp = central_repo_path.clone().join(".git").join("refs").join("heads").join("master");
    let mut f = File::open(&fp).expect("open file master");
    let mut s = String::new();
    f.read_to_string(&mut s).expect("read file shoud work");
    println!("master file in {:?}: {}", central_repo_path, s);

    let h = central_repo.head().expect("get head of central repo");
    let t = h.target().expect("get oid of head reference");
    let central_repo_head_oid = t.as_bytes();


    println!("    ########### START: calling central_submit ########### ");
    let td_scratch = TempDir::new("scratch").expect("folder scratch should be created");

    migrate::central_submit(
        &_oid_to_sha1(&central_repo_head_oid),
        &host,
        "central",
        &central_repo_path,
        &td_scratch.path()
        // &Path::new("/tmp/testscratch")
    ).expect("call central_submit");
    // for m in module_names {
    //     let x = td_scratch.path().join(&m);
    //     migrate::call_command("git", &["checkout", "master"], Some(&x));
    //     assert!(td_scratch.path().join(m).join("added_in_central.txt").exists());
    // }
    // std::thread::sleep_ms(1111111);
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
    migrate::call_command("git", &["status"], Some(&path));
    repo
}

fn create_dummy_file(f: &PathBuf) {
    let parent_dir = f.as_path().parent().expect("need to get parent");
    fs::create_dir_all(parent_dir).expect("create directories");

    let mut file = File::create(&f.as_path()).expect("create file");
    file.write_all("test content".as_bytes()).expect("write to file");
}
