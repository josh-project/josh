import { Probot } from "probot";

export = (app: Probot) => {

  app.on("push", async (context) => {
    const match = new RegExp("^refs/heads/@changes/\(.*\)/\(.*\)/\(.*\)$")
    let res
    if((res = match.exec(context.payload.ref))!== null) {
      const owner = context.payload.repository.owner.name ?? ""
      const repo = context.payload.repository.name
      const title = context.payload.head_commit?.message.split('\n')[0] ?? ""
      const head = context.payload.ref
      const base = res[1]
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
    }
  });
};
