// extern crate centralgithook;
extern crate git2;
extern crate clap;
extern crate env_logger;

use std::path::Path;
use super::RepoHost;
use super::Scratch;
use super::ReviewUploadResult;
use super::Hooks;


pub fn dispatch(pargs: Vec<String>, hooks: &Hooks, host: &RepoHost, scratch: &Scratch) -> i32
{
    println!(".\n");
    let hook = &pargs[0];
    let args = clap::App::new("centralgithook")
        .arg(clap::Arg::with_name("branch").long("branch").takes_value(true))
        .arg(clap::Arg::with_name("change").long("change").takes_value(true))
        .arg(clap::Arg::with_name("change-owner").long("change-owner").takes_value(true))
        .arg(clap::Arg::with_name("change-url").long("change-url").takes_value(true))
        .arg(clap::Arg::with_name("commit").long("commit").takes_value(true))
        .arg(clap::Arg::with_name("head").long("head").takes_value(true))
        .arg(clap::Arg::with_name("newrev").long("newrev").takes_value(true))
        .arg(clap::Arg::with_name("oldrev").long("oldrev").takes_value(true))
        .arg(clap::Arg::with_name("project").long("project").takes_value(true))
        .arg(clap::Arg::with_name("refname").long("refname").takes_value(true))
        .arg(clap::Arg::with_name("submitter").long("submitter").takes_value(true))
        .arg(clap::Arg::with_name("topic").long("topic").takes_value(true))
        .arg(clap::Arg::with_name("uploader").long("uploader").takes_value(true))
        .get_matches_from(&pargs);

    let oldrev = args.value_of("oldrev").unwrap_or("");
    let project = args.value_of("project").unwrap_or("");
    let refname = args.value_of("refname").unwrap_or("");

    let is_module = project != format!("{}{}", host.prefix(), host.central());
    let (_, project) = project.split_at(host.prefix().len());

    let this_project = Path::new(&host.local_path(project)).to_path_buf();

    // ref-update: fired after push
    // change-merged: fired after gerrit-submit
    let is_update = hook.ends_with("ref-update");
    let is_submit = hook.ends_with("change-merged");
    let is_project_created = hook.ends_with("project-created");

    let is_review = is_update && refname == format!("refs/for/{}", hooks.branch());;
    let is_initial = !is_module && oldrev == "0000000000000000000000000000000000000000";

    let uploader = args.value_of("uploader").unwrap_or("");
    if is_update && !is_review && !uploader.contains("Automation") {
        // FIXME: hardcoded
        debug!("==== uploader: {}", uploader);
        debug!("{} {} {}",
               is_update,
               is_review,
               uploader.contains("Automation"));
        println!("===================================================================");
        println!("================= Do not push directly to {}! =================",
                 hooks.branch());
        println!("===================================================================");
        return 1;
    }

    if is_submit {
        // submit to central
        let commit = args.value_of("commit").unwrap_or("");
        hooks.central_submit(&scratch, host, scratch.transfer(commit, &this_project));
    }
    else if is_project_created {
        hooks.project_created(&scratch, host, &project);
        println!("==== project_created");
    }
    else if is_review {
        // module was pushed, get changes to central
        let newrev = args.value_of("newrev").unwrap_or("");
        match hooks.review_upload(&scratch,
                                  host,
                                  scratch.transfer(newrev, &this_project),
                                  project) {
            ReviewUploadResult::RejectNoFF => {
                println!("===================================================================");
                println!("=========== Commit not based on {}, rebase first! =============",
                         hooks.branch());
                println!("===================================================================");
            }
            ReviewUploadResult::NoChanges => {}
            ReviewUploadResult::RejectMerge => {
                println!("===================================================================");
                println!("=================== Do not submit merge commits! ==================");
                println!("===================================================================");
            }
            ReviewUploadResult::Uploaded(oid, initial) => {
                println!("================ Doing actual upload in central git ===============");
                if initial {
                    println!("======================= This is a NEW module ======================");
                }

                println!("{}",
                         scratch.push(host,
                                      oid,
                                      host.central(),
                                      &format!("refs/for/{}", hooks.branch())));
                println!("==== The review upload may have worked, even if it says error below. \
                          Look UP! ====")
            }
            ReviewUploadResult::Central => return 0,
        }

        // stop host from allowing push to module directly
        return 1;
    }
    else if !is_module && is_update && !is_review {
        if is_initial {
            println!(".\n\n##### INITIAL IMPORT ######");
            let newrev = args.value_of("newrev").unwrap_or("");
            // hooks.central_submit(&scratch, scratch.transfer(newrev, &this_project));
            hooks.central_submit(&scratch, host, scratch.transfer(newrev, &this_project));
            return 0;
        }
        else {
            println!(".\n\n##### INITIAL IMPORT ALREADY HAPPEND ######");
            return 1;
        }
    }
    return 0;
}
