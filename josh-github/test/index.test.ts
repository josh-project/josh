// You can import your modules
// import index from '../src/index'

import nock from "nock";
// Requiring our app implementation
import myProbotApp from "../src";
import { Probot, ProbotOctokit } from "probot";
// Requiring our fixtures
import payload from "./fixtures/push.json";
const pullBody = {
    title: "WIP github CR app",
    head: "refs/heads/@changes/test3/louis-marie.givel@esrlabs.com/github-app",
    base: "test3"
}
const fs = require("fs");
const path = require("path");

const privateKey = fs.readFileSync(
  path.join(__dirname, "fixtures/mock-cert.pem"),
  "utf-8"
);

describe("josh-github", () => {
  let probot: any;

  beforeEach(() => {
    nock.disableNetConnect();
    probot = new Probot({
      appId: 123,
      privateKey,
      // disable request throttling and retries for testing
      Octokit: ProbotOctokit.defaults({
        retry: { enabled: false },
        throttle: { enabled: false },
      }),
    });
    // Load our app into probot
    probot.load(myProbotApp);
  });

  test("Open PR on push", async () => {
    const mock = nock("https://api.github.com")
      // Test that we correctly return a test token
      .post("/app/installations/27237071/access_tokens")
      .reply(200, {
        token: "test",
        permissions: {
          issues: "write",
        },
      })

      // Test that a PR is opened
      .post("/repos/josh-project/josh/pulls", (body: any) => {
        expect(body).toMatchObject(pullBody);
        return true;
      })
      .reply(200);

    // Receive a webhook event
    await probot.receive({ name: "push", payload });

    expect(mock.pendingMocks()).toStrictEqual([]);
  });

  afterEach(() => {
    nock.cleanAll();
    nock.enableNetConnect();
  });
});

// For more information about testing with Jest see:
// https://facebook.github.io/jest/

// For more information about using TypeScript in your tests, Jest recommends:
// https://github.com/kulshekhar/ts-jest

// For more information about testing with Nock see:
// https://github.com/nock/nock
