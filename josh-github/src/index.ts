import { Probot } from "probot";

async function get_base(app: any, context: any, owner: any, repo: any, base: any) {
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
         const base_ref = "refs/heads/@base-for/" + context.payload.ref

         //try to create base branch
         try {
          const res_create_ref = await context.octokit.git.createRef({
               owner,
               repo,
               ref: base_ref,
               sha: ref.object.sha,
           })

           if(res_create_ref.status === 200) {
               return base_ref
           }
         }
         catch (e: unknown) {
             app.log("couldn't create ref " + e)
         }

         //update if it failed
         try {
           const res_update_ref = await context.octokit.git.updateRef({
               owner,
               repo,
               ref: "heads/@base-for/" + context.payload.ref,
               sha: ref.object.sha,
               force: true
           })

           if(res_update_ref.status === 200) {
             return base_ref
           }
         }
         catch (e: unknown) {
             app.log("couldn't update ref " + e)
         }

         return base
       }
     }
   }
   return base
}

async function get_pr(context: any, owner: any, repo: any, pr_head: any) {
    const pr = await context.octokit.pulls.list({
      owner,
      repo,
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

    const base = await get_base(app, context, owner, repo, parsed_ref[1])

    if(current_pr.length === 1) {
        current_pr = current_pr[0]
        app.log("PR existing for ref " + context.payload.ref)
       
        // force refresh of base branch by updating to master, then to base.
        try {
          await context.octokit.pulls.update({
              owner,
              repo,
              pull_number: current_pr.number,
              base: "master",
          })
          app.log("PR base updated to master")
        }
        catch (e: unknown) {
            app.log("Couldn't update PR " + e)
        }
        try {
          await context.octokit.pulls.update({
              owner,
              repo,
              pull_number: current_pr.number,
              base,
          })
          app.log("PR base updated to base")
        }
        catch (e: unknown) {
            app.log("Couldn't update PR " + e)
        }
        
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
    await context.octokit.pulls.create(req);
    
  });
};
