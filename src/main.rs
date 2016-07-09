extern crate centralgithook;
extern crate git2;
extern crate clap;
extern crate fern;
#[macro_use]
extern crate log;

use std::env;
use std::process::exit;
use std::path::Path;
use centralgithook::CentralGit;
use centralgithook::Scratch;
use centralgithook::Gerrit;
use centralgithook::RepoHost;

const GERRIT_PORT: &'static str = "29418";
const AUTOMATION_USER: &'static str = "centralgit";
const GERRIT_HOST: &'static str = "localhost";
const CENTRAL_NAME: &'static str = "central";
const BRANCH: &'static str = "master";

fn main()
{
    let git_dir = env::var("GIT_DIR").expect("GIT_DIR not set");
    if let Some((gerrit_path, gerrit)) = Gerrit::new(&Path::new(&git_dir),
                                                     CENTRAL_NAME,
                                                     AUTOMATION_USER,
                                                     GERRIT_HOST,
                                                     GERRIT_PORT) {
        let logfilename = &gerrit.root()
            .join("logs")
            .join("centralgit.log");

        let logger_config = fern::DispatchConfig {
            format: Box::new(|msg: &str, level: &log::LogLevel, _location: &log::LogLocation| {
                format!("[{}] {}", level, msg)
            }),
            output: vec![fern::OutputConfig::file(logfilename)],
            level: log::LogLevelFilter::Trace,
        };

        fern::init_global_logger(logger_config, log::LogLevelFilter::Trace)
            .expect("can't init logger");

        debug!("Gerrit prefix: {:?}", gerrit.prefix());


        let scratch_dir = gerrit_path.join(format!("centralgithook_scratch_{}", gerrit.prefix()));
        let scratch = Scratch::new(&scratch_dir);

        let mut args = vec![];
        for arg in env::args() {
            args.push(arg);
        }
        let hooks = CentralGit::new(BRANCH);
        exit(centralgithook::dispatch(args, &hooks, &gerrit, &gerrit, &scratch));
    }
}
