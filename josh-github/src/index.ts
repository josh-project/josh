import { Probot } from "probot";

async function get_base(context: any, owner: any, repo: any, base: any) {
   // list references
   const matching_refs = await context.octokit.git.listMatchingRefs({
       owner,
       repo,
       ref: "heads/@changes",
   })
   if(matching_refs.data.length === 0) {
       return base
   }

   // get commit parents
   const commit = await context.octokit.git.getCommit({
     owner,
     repo,
     commit_sha: context.payload.after
   })
   if(commit.data.length === 0) {
       return base
   }
   for(let parent of commit.data.parents){
     for(let ref of matching_refs.data) {
       if(parent.sha == ref.object.sha) {
         return ref.ref
       }
     }
   }
   return base
}

async function get_pr(context: any, owner: any, repo: any, pr_head: any) {
    const pr = await context.octokit.pulls.list({
      owner,
      repo,
      state: "open",
      head: "josh-project:" + pr_head,
    })

    return pr.data
}

export = (app: Probot) => {

  app.on("push", async (context) => {
    const match = new RegExp("^refs/heads/@changes/\(.*\)/\(.*\)/\(.*\)$")
    let parsed_ref = match.exec(context.payload.ref)
    if(parsed_ref === null) {
        app.log("Push to non-matching ref " + context.payload.ref)
        return
    }

    const owner = context.payload.repository.owner.name ?? ""
    const repo = context.payload.repository.name
    const title = context.payload.head_commit?.message.split('\n')[0] ?? ""
    const head = context.payload.ref

    // check if the PR exists already
    let current_pr = await get_pr(context, owner, repo, head)
    if(current_pr.length > 1) {
        app.log("Multiple PRs existing for ref " + context.payload.ref + ". Aborting.")
        return
    }

    const base = await get_base(context, owner, repo, parsed_ref[1])

    if(current_pr.length === 1) {
        current_pr = current_pr[0]
        app.log("PR existing for ref " + context.payload.ref)

        if("refs/heads/" + current_pr.base.ref === base) {
            app.log("PR already up-to-date")
            return
        }

        if(current_pr.state !== "open") {
          let update_res = await context.octokit.pulls.update({
              owner,
              repo,
              pull_number: current_pr.number,
              state: "open",
          })
          app.log("Reopen closed PR")
          app.log(update_res)
        }

        let update_res = await context.octokit.pulls.update({
            owner,
            repo,
            pull_number: current_pr.number,
            base,
        })
        app.log("PR base updated")
        app.log(update_res)
        
        return
    }

    const req = {
      owner,
      title, 
      repo,
      head,
      base
    }

    app.log("opening PR")
    app.log(req)
    await context.octokit.pulls.create(req);
    
  });
};
