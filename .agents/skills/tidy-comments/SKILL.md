---
name: tidy-comments
description: Clean up code comments in the context of current PR/change. Use when user asks to clean up or tidy code comments.
---

The goal of this skill is to clean up code comments, rewriting them in a way that minimizes noise,
and maximizes useful information for code readers.

* Comments should be concise, clear and focused.
* Comments should focus on why the code exists; not what it does.
* Comments should not plainly restate code in plain language;
  instead of repeating the logic, comments should signal intent.
* Comments that explain code that is already obvious should be avoided altogether.
  * Example of a comment that should be removed:
  ```
  // start the server
  server.start()?;
  ```
* Comments should be focused on the region of code they're placed in; irrelevant,
  far-reaching comments that don't help with readability of the code should be removed.
* Comments should not include decorative elements: horizontal bars, ASCII boxes, etc.
* Comments should not include references to in-progress plans and documents not included
  in the commit scope: no references future readers can't access.
* Comment length should not hurt readability of the code; use long comments sparingly.
