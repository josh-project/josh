//! A revision walker reproducing libgit2's `git_revwalk` algorithm exactly, for the
//! TOPOLOGICAL (+ optional first-parent, hide, hide-callback) configurations josh uses.
//!
//! Ported from the vendored libgit2 1.9.2 sources (`RW` = src/libgit2/revwalk.c,
//! `CL` = src/libgit2/commit_list.c, `PQ` = src/util/pqueue.c), pinned by differential
//! tests against `git2::Revwalk` below. The goal is identical yield sets AND identical
//! order, so call sites keep producing byte-identical results when they switch over.
//!
//! Algorithm shape (all josh walks are "limited", so this is exactly what libgit2 runs
//! inside the first `next()` call):
//!
//! 1. prepare (RW:617-658): user inputs are parsed; hidden inputs get their (directly
//!    known) parents marked uninteresting; the initial frontier is the user inputs in
//!    reverse push/hide call order (`user_input` is head-prepended at RW:96 and prepare
//!    iterates head-first).
//! 2. limit_list (RW:471-505): pop from a commit-date-descending frontier
//!    (`git_commit_list_insert_by_date`, CL:62-75 -- equal-time commits insert AFTER
//!    existing equal-time entries, so ties keep insertion order). Uninteresting pops
//!    propagate marks to all parents (ignoring first-parent and the hide callback) and
//!    are subject to the SLOP=5 date cutoff (RW:443-469). Interesting pops enqueue their
//!    parents (only the first under `simplify_first_parent`; a parent pruned by the hide
//!    callback is skipped with `continue`, which notably also skips the first-parent
//!    `break`, RW:428-437) and are appended to the output unless pruned themselves
//!    (RW:495-496).
//! 3. sort_in_topological_order (RW:531-615): Kahn's algorithm over the limit_list
//!    output only, with a plain-vector LIFO ready queue (the pqueue has no comparator
//!    without GIT_SORT_TIME: insert appends, pop takes the last element, PQ:98-110) and
//!    the initial tip set reversed once (RW:585-586). Entries marked uninteresting after
//!    they were appended still participate in the sort and are filtered only at yield
//!    (RW:333), which this port does when draining the sorted list into the result.
//! 4. REVERSE is buffer-and-reverse of the forward order (RW:684-694); callers of
//!    [`RevWalk::into_topo_vec`] simply iterate the result back to front.
//!
//! Commit reads go through raw odb bytes and a single `gix_object::CommitRefIter` pass
//! per commit (caching parent ids and committer time), never libgit2's lock-guarded
//! commit parse cache and never an owned `gix_object::Commit`.
//!
//! Known divergences from libgit2:
//!
//! - No annotated-tag peeling: `git_revwalk__push_commit` peels tags to commits (RW:55);
//!   [`RevWalk::push`]/[`RevWalk::hide`] error on any non-commit instead. Josh only ever
//!   pushes commit oids.
//! - Malformed-commit strictness differs (only reachable with fsck-invalid objects).
//!   libgit2 fully parses push/hide inputs (RW:52) but quick-parses during the walk
//!   (CL:132-135), skipping tree validation and tolerating a missing author line;
//!   `CommitRefIter` validates both, so such a commit errors here during the walk
//!   where libgit2 errors at push/hide time (if it is the input) or not at all (if it
//!   is an ancestor). In particular a tolerated-hide site like unapply_filter turns a
//!   libgit2 push-time parse failure (walk proceeds unhidden) into a walk error.
//!   Committer-line parity is restored explicitly in [`RevWalk::parse`]: no timestamp
//!   at all means time 0 on both sides (signature.c leaves the zeroed time), and an
//!   object truncated before the committer line errors on both sides -- but other
//!   exotic malformed committer lines may still classify differently (e.g. libgit2
//!   treats junk between the email and the newline as "no time" where gix's decoder
//!   errors).
//! - No commit-graph fast path (CL:180-204); it only changes where libgit2 reads parent
//!   ids and times from, not the walk semantics.

use std::collections::{HashMap, VecDeque};
use std::ops::ControlFlow;

use crate::git2_oid;

/// How many uninteresting commits to look at after running out of interesting ones
/// (RW:443).
const SLOP: i32 = 5;

type Visit<'v> = &'v mut dyn FnMut(git2::Oid) -> anyhow::Result<ControlFlow<()>>;

/// Per-commit state, the port of `git_commit_list_node` (flags at commit_list.h).
struct Node {
    oid: git2::Oid,
    /// All parents as node indices, populated by [`RevWalk::parse`]. Like libgit2's
    /// quick-parse this always records the full parent list even under first-parent
    /// simplification -- the topo sort counts edges over all parents (RW:561-567).
    parents: Vec<usize>,
    /// Committer timestamp in seconds; the date the frontier is ordered by.
    time: i64,
    in_degree: usize,
    parsed: bool,
    seen: bool,
    added: bool,
    uninteresting: bool,
}

impl Node {
    fn new(oid: git2::Oid) -> Self {
        Node {
            oid,
            parents: Vec::new(),
            time: 0,
            in_degree: 0,
            parsed: false,
            seen: false,
            added: false,
            uninteresting: false,
        }
    }
}

/// See the module docs. One-shot: configure via [`push`]/[`hide`]/
/// [`simplify_first_parent`], then consume with [`into_topo_vec`] or [`discover`].
///
/// [`push`]: RevWalk::push
/// [`hide`]: RevWalk::hide
/// [`simplify_first_parent`]: RevWalk::simplify_first_parent
/// [`into_topo_vec`]: RevWalk::into_topo_vec
/// [`discover`]: RevWalk::discover
pub struct RevWalk<'a> {
    odb: &'a git2::Odb<'a>,
    nodes: Vec<Node>,
    by_oid: HashMap<git2::Oid, usize>,
    /// Push/hide inputs in call order. libgit2 keeps them head-prepended and prepare
    /// iterates head-first (RW:96, RW:635), so prepare walks this in reverse.
    user_input: Vec<usize>,
    /// Reusable stack for [`mark_parents_uninteresting`](RevWalk::mark_parents_uninteresting),
    /// which runs once per uninteresting pop; always drained empty between calls.
    scratch: Vec<usize>,
    did_push: bool,
    first_parent: bool,
}

impl<'a> RevWalk<'a> {
    pub fn new(odb: &'a git2::Odb<'a>) -> Self {
        RevWalk {
            odb,
            nodes: Vec::new(),
            by_oid: HashMap::new(),
            user_input: Vec::new(),
            scratch: Vec::new(),
            did_push: false,
            first_parent: false,
        }
    }

    /// Mark a commit to start traversal from. Parity with `git_revwalk_push`: a missing
    /// oid or a non-commit object errors immediately (RW:52-64; tags are NOT peeled,
    /// see module docs).
    pub fn push(&mut self, tip: git2::Oid) -> anyhow::Result<()> {
        self.push_commit(tip, false)
    }

    /// Mark a commit (and, during the walk, its ancestry) as uninteresting. Same
    /// immediate error behavior as [`push`](RevWalk::push); a hidden commit with
    /// missing ancestors errors during the walk, not here (RW:403-404).
    pub fn hide(&mut self, oid: git2::Oid) -> anyhow::Result<()> {
        self.push_commit(oid, true)
    }

    /// Port of `git_revwalk__push_commit` (RW:44-104).
    fn push_commit(&mut self, oid: git2::Oid, uninteresting: bool) -> anyhow::Result<()> {
        let (_, kind) = self.odb.read_header(oid)?;
        if kind != git2::ObjectType::Commit {
            anyhow::bail!("object {} is not a committish", oid);
        }
        let commit = self.lookup(oid);
        // A previous hide already told us we don't want this commit (RW:76-78). Note
        // this also skips setting did_push for a push-after-hide.
        if self.nodes[commit].uninteresting {
            return Ok(());
        }
        if uninteresting {
            self.nodes[commit].uninteresting = true;
        } else {
            self.did_push = true;
        }
        self.user_input.push(commit);
        Ok(())
    }

    /// Only the first parent of each interesting commit is loaded and enqueued
    /// (RW:436-437); the uninteresting branch still expands all parents (RW:390-415).
    pub fn simplify_first_parent(&mut self) {
        self.first_parent = true;
    }

    /// The complete walk in forward TOPOLOGICAL order, exactly libgit2's. `prune` has
    /// `git_revwalk_add_hide_cb` semantics: pure traversal pruning, consulted once per
    /// parent edge before insertion (RW:428-429) and once per popped commit before
    /// emission (RW:495-496); no uninteresting marking, no propagation, and it may be
    /// called multiple times for the same oid. REVERSE order is the result iterated
    /// back to front (RW:684-694 is a plain buffer-reverse).
    pub fn into_topo_vec(
        mut self,
        mut prune: impl FnMut(git2::Oid) -> bool,
    ) -> anyhow::Result<Vec<git2::Oid>> {
        // A walk without pushes is already over (RW:623-627).
        let Some(frontier) = self.prepare()? else {
            return Ok(Vec::new());
        };
        let list = self.limit_list(frontier, &mut prune, None)?;
        let sorted = self.sort_in_topological_order(&list);
        Ok(sorted
            .into_iter()
            .filter(|&i| !self.nodes[i].uninteresting)
            .map(|i| self.nodes[i].oid)
            .collect())
    }

    /// Lazy discovery iteration: visits oids in frontier insertion order -- the pushed
    /// tip(s) first (in initial frontier order, hidden inputs skipped), then the
    /// newly-seen parents of each date-desc-popped commit in parent-index order,
    /// expanding as it goes. `visit` returns [`ControlFlow::Break`] to abort the walk.
    ///
    /// Commits already marked uninteresting at insertion time are not visited; with
    /// hides, marks arriving after a commit was visited cannot retract the visit (the
    /// planned caller, find_unapply_base, uses no hides).
    pub fn discover(
        mut self,
        mut visit: impl FnMut(git2::Oid) -> anyhow::Result<ControlFlow<()>>,
    ) -> anyhow::Result<()> {
        let Some(frontier) = self.prepare()? else {
            return Ok(());
        };
        for &commit in &frontier {
            if self.nodes[commit].uninteresting {
                continue;
            }
            if visit(self.nodes[commit].oid)?.is_break() {
                return Ok(());
            }
        }
        let mut no_prune = |_: git2::Oid| false;
        self.limit_list(frontier, &mut no_prune, Some(&mut visit))?;
        Ok(())
    }

    /// Port of `git_revwalk__commit_lookup` (RW:23-42).
    fn lookup(&mut self, oid: git2::Oid) -> usize {
        if let Some(&i) = self.by_oid.get(&oid) {
            return i;
        }
        let i = self.nodes.len();
        self.nodes.push(Node::new(oid));
        self.by_oid.insert(oid, i);
        i
    }

    /// Port of `git_commit_list_parse` + `commit_quick_parse` (CL:126-232): one raw odb
    /// read, one `CommitRefIter` pass caching parent ids and committer time. Strict
    /// about missing objects, like libgit2 ("git does it gently here, but we don't like
    /// missing objects", RW:403).
    fn parse(&mut self, commit: usize) -> anyhow::Result<()> {
        if self.nodes[commit].parsed {
            return Ok(());
        }
        let oid = self.nodes[commit].oid;
        let odb = self.odb;
        let obj = odb.read(oid)?;
        if obj.kind() != git2::ObjectType::Commit {
            anyhow::bail!("object {} is no commit object", oid);
        }
        let mut parents = Vec::new();
        let mut time = None;
        for token in gix_object::CommitRefIter::from_bytes(obj.data(), gix_hash::Kind::Sha1) {
            use gix_object::commit::ref_iter::Token;
            match token? {
                Token::Tree { .. } | Token::Author { .. } => {}
                Token::Parent { id } => {
                    let p = git2_oid(&id);
                    parents.push(self.lookup(p));
                }
                Token::Committer { signature } => {
                    // A committer line with no timestamp at all is valid in libgit2
                    // (signature.c leaves the zeroed time when nothing follows the
                    // email), so only decode a non-empty time field.
                    time = Some(if signature.time.trim().is_empty() {
                        0
                    } else {
                        signature.time()?.seconds
                    });
                    break;
                }
                _ => break,
            }
        }
        // An object truncated before the committer line just ends the iterator;
        // libgit2 errors on it ("no newline given").
        let Some(time) = time else {
            anyhow::bail!("object {} has no committer line", oid);
        };
        let node = &mut self.nodes[commit];
        node.parents = parents;
        node.time = time;
        node.parsed = true;
        Ok(())
    }

    /// Port of `prepare_walk`'s input processing (RW:617-658). Returns `None` when
    /// nothing was pushed (empty walk).
    fn prepare(&mut self) -> anyhow::Result<Option<VecDeque<usize>>> {
        if !self.did_push {
            return Ok(None);
        }
        let mut frontier = VecDeque::new();
        // user_input is iterated head-first in C, i.e. latest push/hide first.
        for i in (0..self.user_input.len()).rev() {
            let commit = self.user_input[i];
            self.parse(commit)?;
            if self.nodes[commit].uninteresting {
                self.mark_parents_uninteresting(commit);
            }
            if !self.nodes[commit].seen {
                self.nodes[commit].seen = true;
                frontier.push_back(commit);
            }
        }
        Ok(Some(frontier))
    }

    /// Port of `mark_parents_uninteresting` (RW:348-378): marks all ancestors reachable
    /// through already-parsed nodes, stopping at unparsed ones (RW:370-371 -- for a
    /// parsed root, `parents[0]` is the pool-zeroed NULL, ending the inner loop).
    fn mark_parents_uninteresting(&mut self, commit: usize) {
        // C prepends each parent and pops from the head (last parent first); a Vec
        // pushed in parent order and popped from the end is the same LIFO.
        let mut stack = std::mem::take(&mut self.scratch);
        stack.extend_from_slice(&self.nodes[commit].parents);
        while let Some(popped) = stack.pop() {
            let mut c = popped;
            loop {
                if self.nodes[c].uninteresting {
                    break;
                }
                self.nodes[c].uninteresting = true;
                if !self.nodes[c].parsed {
                    break;
                }
                for i in 0..self.nodes[c].parents.len() {
                    stack.push(self.nodes[c].parents[i]);
                }
                match self.nodes[c].parents.first() {
                    Some(&p) => c = p,
                    None => break,
                }
            }
        }
        self.scratch = stack;
    }

    /// Port of `add_parents_to_list` (RW:380-440). `visit` is the [`discover`]
    /// insertion callback, fired for parents newly inserted into the frontier on the
    /// interesting path.
    ///
    /// [`discover`]: RevWalk::discover
    fn add_parents_to_list(
        &mut self,
        commit: usize,
        list: &mut VecDeque<usize>,
        prune: &mut dyn FnMut(git2::Oid) -> bool,
        visit: &mut Option<Visit<'_>>,
    ) -> anyhow::Result<ControlFlow<()>> {
        if self.nodes[commit].added {
            return Ok(ControlFlow::Continue(()));
        }
        self.nodes[commit].added = true;

        // "Go full on in the uninteresting case": all parents, no first-parent, no
        // prune callback, insertion not guarded by `seen` (RW:390-415).
        if self.nodes[commit].uninteresting {
            for i in 0..self.nodes[commit].parents.len() {
                let p = self.nodes[commit].parents[i];
                self.nodes[p].uninteresting = true;
                self.parse(p)?;
                self.mark_parents_uninteresting(p);
                self.nodes[p].seen = true;
                insert_by_date(&self.nodes, list, p);
            }
            return Ok(ControlFlow::Continue(()));
        }

        for i in 0..self.nodes[commit].parents.len() {
            let p = self.nodes[commit].parents[i];
            self.parse(p)?;
            // A pruned parent `continue`s past the first-parent break below (RW:429),
            // so under first-parent simplification a pruned first parent lets the
            // second parent be considered. Faithful to the C.
            if prune(self.nodes[p].oid) {
                continue;
            }
            if !self.nodes[p].seen {
                self.nodes[p].seen = true;
                insert_by_date(&self.nodes, list, p);
                if let Some(v) = visit.as_mut() {
                    if v(self.nodes[p].oid)?.is_break() {
                        return Ok(ControlFlow::Break(()));
                    }
                }
            }
            if self.first_parent {
                break;
            }
        }
        Ok(ControlFlow::Continue(()))
    }

    /// Port of `still_interesting` (RW:445-469).
    fn still_interesting(&self, list: &VecDeque<usize>, time: i64, slop: i32) -> i32 {
        let Some(&head) = list.front() else {
            return 0;
        };
        if time <= self.nodes[head].time {
            return SLOP;
        }
        for &i in list {
            if !self.nodes[i].uninteresting || self.nodes[i].time > time {
                return SLOP;
            }
        }
        slop - 1
    }

    /// Port of `limit_list` (RW:471-505). Returns the emitted commits in frontier pop
    /// order; entries may still be marked uninteresting afterwards by later pops.
    fn limit_list(
        &mut self,
        mut list: VecDeque<usize>,
        prune: &mut dyn FnMut(git2::Oid) -> bool,
        mut visit: Option<Visit<'_>>,
    ) -> anyhow::Result<Vec<usize>> {
        let mut slop = SLOP;
        let mut time = i64::MAX;
        let mut newlist = Vec::new();
        while let Some(commit) = list.pop_front() {
            if self
                .add_parents_to_list(commit, &mut list, prune, &mut visit)?
                .is_break()
            {
                return Ok(newlist);
            }
            if self.nodes[commit].uninteresting {
                self.mark_parents_uninteresting(commit);
                slop = self.still_interesting(&list, time, slop);
                if slop != 0 {
                    continue;
                }
                break;
            }
            if prune(self.nodes[commit].oid) {
                continue;
            }
            time = self.nodes[commit].time;
            newlist.push(commit);
        }
        Ok(newlist)
    }

    /// Port of `sort_in_topological_order` (RW:531-615) for the no-GIT_SORT_TIME case:
    /// the ready queue is a pqueue with NULL comparator, i.e. a plain LIFO stack
    /// (PQ:98-110), seeded with the tips reversed once (RW:585-586).
    fn sort_in_topological_order(&mut self, list: &[usize]) -> Vec<usize> {
        for &i in list {
            self.nodes[i].in_degree = 1;
        }
        // Child counts over ALL parent edges, restricted to in-list nodes (parents
        // outside the list keep in_degree 0 and are skipped, RW:561-567).
        for &i in list {
            for j in 0..self.nodes[i].parents.len() {
                let p = self.nodes[i].parents[j];
                if self.nodes[p].in_degree > 0 {
                    self.nodes[p].in_degree += 1;
                }
            }
        }
        let mut stack: Vec<usize> = list
            .iter()
            .copied()
            .filter(|&i| self.nodes[i].in_degree == 1)
            .collect();
        stack.reverse();
        let mut out = Vec::with_capacity(list.len());
        while let Some(next) = stack.pop() {
            for j in 0..self.nodes[next].parents.len() {
                let p = self.nodes[next].parents[j];
                if self.nodes[p].in_degree == 0 {
                    continue;
                }
                self.nodes[p].in_degree -= 1;
                if self.nodes[p].in_degree == 1 {
                    stack.push(p);
                }
            }
            self.nodes[next].in_degree = 0;
            out.push(next);
        }
        out
    }
}

/// Port of `git_commit_list_insert_by_date` (CL:62-75): scan from the head, insert
/// before the first entry with a strictly older time -- equal-time entries are passed
/// over, so ties keep insertion order.
fn insert_by_date(nodes: &[Node], list: &mut VecDeque<usize>, commit: usize) {
    let time = nodes[commit].time;
    let pos = list
        .iter()
        .position(|&i| nodes[i].time < time)
        .unwrap_or(list.len());
    list.insert(pos, commit);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A temp repo with commits at fully controlled timestamps (duplicate timestamps
    /// are the norm in josh's test repos, so most shapes below share one timestamp).
    struct TestRepo {
        _dir: tempfile::TempDir,
        repo: git2::Repository,
    }

    impl TestRepo {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = git2::Repository::init_bare(dir.path()).unwrap();
            TestRepo { _dir: dir, repo }
        }

        fn commit(&self, msg: &str, time: i64, parents: &[git2::Oid]) -> git2::Oid {
            let sig = git2::Signature::new("Test", "test@example.com", &git2::Time::new(time, 0))
                .unwrap();
            let tree_id = self.repo.treebuilder(None).unwrap().write().unwrap();
            let tree = self.repo.find_tree(tree_id).unwrap();
            let parents: Vec<_> = parents
                .iter()
                .map(|&p| self.repo.find_commit(p).unwrap())
                .collect();
            let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
            self.repo
                .commit(None, &sig, &sig, msg, &tree, &parent_refs)
                .unwrap()
        }

        /// Write raw commit bytes so parents don't have to exist in the odb (and may
        /// repeat -- git2's commit builder can produce neither).
        fn raw_commit(&self, msg: &str, time: i64, parents: &[git2::Oid]) -> git2::Oid {
            self.raw_commit_tail(
                parents,
                &format!(
                    "author Test <test@example.com> {time} +0000\n\
                     committer Test <test@example.com> {time} +0000\n\n{msg}\n"
                ),
            )
        }

        /// Write a commit with an arbitrary (possibly malformed) tail after the
        /// tree/parent headers.
        fn raw_commit_tail(&self, parents: &[git2::Oid], tail: &str) -> git2::Oid {
            let tree_id = self.repo.treebuilder(None).unwrap().write().unwrap();
            let mut buf = format!("tree {}\n", tree_id);
            for p in parents {
                buf.push_str(&format!("parent {}\n", p));
            }
            buf.push_str(tail);
            self.repo
                .odb()
                .unwrap()
                .write(git2::ObjectType::Commit, buf.as_bytes())
                .unwrap()
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum Input {
        Push(git2::Oid),
        Hide(git2::Oid),
    }

    /// The git2 reference walk, configured exactly like [`our_topo`]. Returns the
    /// yielded sequence and the hide-callback invocation sequence.
    fn git2_topo(
        repo: &git2::Repository,
        inputs: &[Input],
        reverse: bool,
        first_parent: bool,
        prune: Option<&HashSet<git2::Oid>>,
    ) -> (Result<Vec<git2::Oid>, git2::Error>, Vec<git2::Oid>) {
        let mut walk = repo.revwalk().unwrap();
        let mut sort = git2::Sort::TOPOLOGICAL;
        if reverse {
            sort |= git2::Sort::REVERSE;
        }
        walk.set_sorting(sort).unwrap();
        if first_parent {
            walk.simplify_first_parent().unwrap();
        }
        let mut calls = Vec::new();
        for input in inputs {
            let r = match *input {
                Input::Push(oid) => walk.push(oid),
                Input::Hide(oid) => walk.hide(oid),
            };
            if let Err(e) = r {
                return (Err(e), calls);
            }
        }
        let result = match prune {
            Some(pruned) => {
                let mut cb = |oid: git2::Oid| {
                    calls.push(oid);
                    pruned.contains(&oid)
                };
                let mut w = walk.with_hide_callback(&mut cb).unwrap();
                let r = (&mut w).collect::<Result<Vec<_>, _>>();
                drop(w);
                drop(cb);
                r
            }
            None => walk.collect::<Result<Vec<_>, _>>(),
        };
        (result, calls)
    }

    fn our_topo(
        repo: &git2::Repository,
        inputs: &[Input],
        reverse: bool,
        first_parent: bool,
        prune: Option<&HashSet<git2::Oid>>,
    ) -> (anyhow::Result<Vec<git2::Oid>>, Vec<git2::Oid>) {
        let odb = repo.odb().unwrap();
        let mut walk = RevWalk::new(&odb);
        if first_parent {
            walk.simplify_first_parent();
        }
        let mut calls = Vec::new();
        for input in inputs {
            let r = match *input {
                Input::Push(oid) => walk.push(oid),
                Input::Hide(oid) => walk.hide(oid),
            };
            if let Err(e) = r {
                return (Err(e), calls);
            }
        }
        let result = match prune {
            Some(pruned) => walk.into_topo_vec(|oid| {
                calls.push(oid);
                pruned.contains(&oid)
            }),
            None => walk.into_topo_vec(|_| false),
        };
        let result = result.map(|mut v| {
            if reverse {
                v.reverse();
            }
            v
        });
        (result, calls)
    }

    /// Exact-sequence differential assertion over all four sort/first-parent modes,
    /// including the prune-callback invocation sequence (the callback fires per parent
    /// edge, so identical sequences prove identical traversal, not just identical
    /// yields).
    fn assert_parity(
        repo: &git2::Repository,
        inputs: &[Input],
        prune: Option<&HashSet<git2::Oid>>,
        label: &str,
    ) {
        for reverse in [false, true] {
            for first_parent in [false, true] {
                let (theirs, their_calls) = git2_topo(repo, inputs, reverse, first_parent, prune);
                let (ours, our_calls) = our_topo(repo, inputs, reverse, first_parent, prune);
                let mode = format!("{label} reverse={reverse} first_parent={first_parent}");
                match (theirs, ours) {
                    (Ok(t), Ok(o)) => {
                        assert_eq!(o, t, "sequence mismatch: {mode}");
                        assert_eq!(our_calls, their_calls, "prune-call mismatch: {mode}");
                    }
                    (Err(_), Err(_)) => {}
                    (t, o) => panic!(
                        "ok/err mismatch: {mode}: git2 ok={} ours ok={}",
                        t.is_ok(),
                        o.is_ok()
                    ),
                }
            }
        }
    }

    /// All hide()/prune variants the design doc calls for, on one graph.
    fn assert_parity_matrix(
        repo: &git2::Repository,
        tips: &[git2::Oid],
        all: &[git2::Oid],
        label: &str,
    ) {
        let pushes: Vec<Input> = tips.iter().map(|&t| Input::Push(t)).collect();
        assert_parity(repo, &pushes, None, &format!("{label} plain"));
        for (i, &hide) in all.iter().enumerate().step_by(1 + all.len() / 4) {
            let mut inputs = pushes.clone();
            inputs.push(Input::Hide(hide));
            assert_parity(repo, &inputs, None, &format!("{label} hide#{i}"));
            // Hide before push: exercises user-input order (reverse call order).
            let mut inputs = vec![Input::Hide(hide)];
            inputs.extend(pushes.iter().copied());
            assert_parity(repo, &inputs, None, &format!("{label} hide-first#{i}"));
        }
        for (i, &pruned) in all.iter().enumerate().step_by(1 + all.len() / 4) {
            let set = HashSet::from([pruned]);
            assert_parity(repo, &pushes, Some(&set), &format!("{label} prune#{i}"));
        }
    }

    #[test]
    fn linear_chain_matches_git2() {
        // All-equal timestamps: ties are the norm.
        let t = TestRepo::new();
        let mut tip = t.commit("c0", 1000, &[]);
        let mut all = vec![tip];
        for i in 1..12 {
            tip = t.commit(&format!("c{i}"), 1000, &[tip]);
            all.push(tip);
        }
        assert_parity_matrix(&t.repo, &[tip], &all, "linear-equal-times");

        // Ascending timestamps.
        let t = TestRepo::new();
        let mut tip = t.commit("c0", 1000, &[]);
        let mut all = vec![tip];
        for i in 1..12 {
            tip = t.commit(&format!("c{i}"), 1000 + i, &[tip]);
            all.push(tip);
        }
        assert_parity_matrix(&t.repo, &[tip], &all, "linear-ascending");
    }

    #[test]
    fn diamond_matches_git2() {
        let t = TestRepo::new();
        let a = t.commit("a", 1000, &[]);
        let b = t.commit("b", 1000, &[a]);
        let c = t.commit("c", 1000, &[a]);
        let m = t.commit("m", 1000, &[b, c]);
        assert_parity_matrix(&t.repo, &[m], &[a, b, c, m], "diamond");
        // Same shape with distinct times in both branch orders.
        let t = TestRepo::new();
        let a = t.commit("a", 1000, &[]);
        let b = t.commit("b", 1002, &[a]);
        let c = t.commit("c", 1001, &[a]);
        let m = t.commit("m", 1003, &[b, c]);
        assert_parity_matrix(&t.repo, &[m], &[a, b, c, m], "diamond-newer-first");
        let m2 = t.commit("m2", 1003, &[c, b]);
        assert_parity_matrix(&t.repo, &[m2], &[a, b, c, m2], "diamond-older-first");
    }

    #[test]
    fn octopus_merge_matches_git2() {
        let t = TestRepo::new();
        let root = t.commit("root", 1000, &[]);
        let b1 = t.commit("b1", 1000, &[root]);
        let b2 = t.commit("b2", 1000, &[root]);
        let b3 = t.commit("b3", 1000, &[root]);
        let b4 = t.commit("b4", 1001, &[root]);
        let m = t.commit("m", 1000, &[b1, b2, b3, b4]);
        assert_parity_matrix(&t.repo, &[m], &[root, b1, b2, b3, b4, m], "octopus");
    }

    #[test]
    fn criss_cross_matches_git2() {
        let t = TestRepo::new();
        let root = t.commit("root", 1000, &[]);
        let a = t.commit("a", 1000, &[root]);
        let b = t.commit("b", 1000, &[root]);
        let x = t.commit("x", 1000, &[a, b]);
        let y = t.commit("y", 1000, &[b, a]);
        let m = t.commit("m", 1000, &[x, y]);
        assert_parity_matrix(&t.repo, &[m], &[root, a, b, x, y, m], "criss-cross");
        // Both criss-cross heads pushed as separate tips, both push orders.
        assert_parity(
            &t.repo,
            &[Input::Push(x), Input::Push(y)],
            None,
            "criss-cross two tips x,y",
        );
        assert_parity(
            &t.repo,
            &[Input::Push(y), Input::Push(x)],
            None,
            "criss-cross two tips y,x",
        );
    }

    #[test]
    fn disjoint_roots_merged_late_matches_git2() {
        let t = TestRepo::new();
        let r1 = t.commit("r1", 1000, &[]);
        let a1 = t.commit("a1", 1000, &[r1]);
        let a2 = t.commit("a2", 1000, &[a1]);
        let r2 = t.commit("r2", 900, &[]);
        let b1 = t.commit("b1", 1000, &[r2]);
        let m = t.commit("m", 1000, &[a2, b1]);
        let tip = t.commit("tip", 1000, &[m]);
        assert_parity_matrix(
            &t.repo,
            &[tip],
            &[r1, a1, a2, r2, b1, m, tip],
            "disjoint-roots",
        );
    }

    #[test]
    fn date_skew_exercises_slop_cutoff() {
        // Children OLDER than parents: the SLOP heuristic can cut the uninteresting
        // scan short and even misclassify -- parity means reproducing that too.
        let t = TestRepo::new();
        let mut times = vec![
            2000i64, 1900, 1800, 1700, 1600, 1500, 1400, 1300, 1200, 1100,
        ];
        let mut tip = t.commit("c0", times.remove(0), &[]);
        let mut all = vec![tip];
        for (i, time) in times.into_iter().enumerate() {
            tip = t.commit(&format!("c{}", i + 1), time, &[tip]);
            all.push(tip);
        }
        assert_parity_matrix(&t.repo, &[tip], &all, "date-skew-linear");

        // Skewed side branch hidden while the main branch is walked.
        let t = TestRepo::new();
        let root = t.commit("root", 1000, &[]);
        let mut main = root;
        let mut all = vec![root];
        for i in 0..8 {
            main = t.commit(&format!("m{i}"), 1000 + i, &[main]);
            all.push(main);
        }
        let mut side = root;
        for i in 0..8 {
            // Side branch times far in the future: hidden frontier entries stay
            // "newer than last emitted", resetting the slop.
            side = t.commit(&format!("s{i}"), 5000 - i, &[side]);
            all.push(side);
        }
        for inputs in [
            vec![Input::Push(main), Input::Hide(side)],
            vec![Input::Hide(side), Input::Push(main)],
            vec![Input::Push(side), Input::Hide(main)],
        ] {
            assert_parity(&t.repo, &inputs, None, "date-skew-hidden-branch");
        }
        assert_parity_matrix(&t.repo, &[main, side], &all, "date-skew-two-tips");
    }

    #[test]
    fn slop_budget_boundary_matches_git2() {
        // A hidden all-uninteresting chain older than everything emitted decrements the
        // slop once per pop; whether the marks reach the junction back into the emitted
        // main chain depends exactly on the SLOP=5 budget. Sweeping the chain length
        // over the boundary discriminates any other budget (libgit2 misclassifies the
        // junction as still-interesting for long chains -- parity means matching that).
        for len in 3..10usize {
            let t = TestRepo::new();
            let mut main = t.commit("r", 1000, &[]);
            let mut chain = vec![main];
            for i in 0..8 {
                main = t.commit(&format!("m{i}"), 1000 + i, &[main]);
                chain.push(main);
            }
            let mut hidden = chain[4];
            for i in 0..len {
                hidden = t.commit(&format!("u{i}"), 20 - i as i64, &[hidden]);
            }
            assert_parity(
                &t.repo,
                &[Input::Push(main), Input::Hide(hidden)],
                None,
                &format!("slop-boundary len={len}"),
            );
        }
    }

    #[test]
    fn reinserted_seen_parent_matches_git2() {
        // The uninteresting branch inserts parents WITHOUT a seen-guard (RW:398-415),
        // so an already-emitted commit P re-enters the frontier when a hidden chain
        // reaches it. While it sits there, still_interesting's head check (`time <=
        // head.time`, RW:456) keeps the slop alive, extending how far the marks travel
        // down a second hidden chain toward its junction with emitted history. Swept
        // over the junction distance, with P's time both far newer than (5000) and
        // exactly equal to (100) the last emitted time -- the equal case pins the <=.
        // q_time=Some(44) gives the re-inserted P an already-parsed parent whose time
        // falls inside the v-chain's range: P's re-pop must NOT re-expand it (the
        // `added` guard, RW:385-388) -- a re-inserted q would burn one of the slop
        // decrements and shift the cutoff.
        for (p_time, q_time) in [
            (5000i64, None),
            (100, None),
            (5000, Some(44i64)),
            (100, Some(44)),
        ] {
            for len in 2..8usize {
                let t = TestRepo::new();
                let a = t.commit("a", 100, &[]);
                let tip = t.commit("t", 100, &[a]);
                let p = match q_time {
                    None => t.commit("p", p_time, &[]),
                    Some(q_time) => {
                        let q = t.commit("q", q_time, &[]);
                        t.commit("p", p_time, &[q])
                    }
                };
                let mut v = a;
                for i in 0..len {
                    v = t.commit(&format!("v{i}"), 46 - i as i64, &[v]);
                }
                let x = t.commit("x", 47, &[p, v]);
                let mut h = x;
                for i in 0..3 {
                    h = t.commit(&format!("u{i}"), 48 + i as i64, &[h]);
                }
                assert_parity(
                    &t.repo,
                    &[Input::Push(tip), Input::Push(p), Input::Hide(h)],
                    None,
                    &format!("reinserted-seen-parent p_time={p_time} q_time={q_time:?} len={len}"),
                );
            }
        }
    }

    #[test]
    fn pruned_commit_does_not_advance_emit_time() {
        // limit_list updates `time` only for EMITTED commits (RW:495-499): a pruned pop
        // must not move the still_interesting reference. With a set-based callback the
        // emission check is only ever reached by a pruned PUSHED TIP (any other pruned
        // commit is already blocked at the parent-edge check, RW:428). Shape: the
        // pruned root tip t1 (t=800) pops after the emitted t=2000 chain; the hidden
        // chain ties t1's time exactly, so `last_emitted(2000) <= 800` keeps
        // decrementing the slop while a leaked `800 <= 800` would keep resetting it,
        // changing whether the marks reach the junction j2 as the length sweeps the
        // slop boundary.
        for len in 1..12usize {
            let t = TestRepo::new();
            let j2 = t.commit("j2", 2000, &[]);
            let j1 = t.commit("j1", 2000, &[j2]);
            let t2 = t.commit("t2", 2000, &[j1]);
            let t1 = t.commit("t1", 800, &[]);
            let mut h = j2;
            for i in 0..len {
                h = t.commit(&format!("u{i}"), 800, &[h]);
            }
            let pruned = HashSet::from([t1]);
            assert_parity(
                &t.repo,
                &[Input::Push(t1), Input::Push(t2), Input::Hide(h)],
                Some(&pruned),
                &format!("pruned-emit-time len={len}"),
            );
        }
    }

    #[test]
    fn deep_chain_matches_git2() {
        let t = TestRepo::new();
        let mut tip = t.commit("c0", 1000, &[]);
        let mut all = vec![tip];
        for i in 1..520 {
            // Duplicate-heavy times with occasional jumps in both directions.
            let time = 1000 + (i % 7) as i64 * ((i % 3) as i64 - 1);
            tip = t.commit(&format!("c{i}"), time, &[tip]);
            all.push(tip);
        }
        let mid = all[200];
        let pruned: HashSet<git2::Oid> = all.iter().copied().step_by(7).collect();
        assert_parity(&t.repo, &[Input::Push(tip)], None, "deep-chain plain");
        assert_parity(
            &t.repo,
            &[Input::Push(tip), Input::Hide(mid)],
            None,
            "deep-chain hide-mid",
        );
        assert_parity(
            &t.repo,
            &[Input::Push(tip)],
            Some(&pruned),
            "deep-chain prune-every-7th",
        );
    }

    #[test]
    fn multiple_tips_and_duplicate_pushes_match_git2() {
        let t = TestRepo::new();
        let root = t.commit("root", 1000, &[]);
        let a = t.commit("a", 1000, &[root]);
        let b = t.commit("b", 1000, &[root]);
        let c = t.commit("c", 1000, &[b]);
        for inputs in [
            vec![Input::Push(a), Input::Push(c)],
            vec![Input::Push(c), Input::Push(a)],
            vec![Input::Push(a), Input::Push(a), Input::Push(c)],
            // A pushed tip that is also an ancestor of another pushed tip.
            vec![Input::Push(b), Input::Push(c)],
            vec![Input::Push(c), Input::Push(b)],
        ] {
            assert_parity(&t.repo, &inputs, None, &format!("multi-tip {inputs:?}"));
        }
    }

    #[test]
    fn hide_of_sibling_branch_matches_git2() {
        let t = TestRepo::new();
        let root = t.commit("root", 1000, &[]);
        let mut main = root;
        for i in 0..5 {
            main = t.commit(&format!("m{i}"), 1000, &[main]);
        }
        let mut side = root;
        for i in 0..5 {
            side = t.commit(&format!("s{i}"), 1000, &[side]);
        }
        for inputs in [
            vec![Input::Push(main), Input::Hide(side)],
            vec![Input::Hide(side), Input::Push(main)],
        ] {
            assert_parity(&t.repo, &inputs, None, "hide-sibling-branch");
        }
    }

    #[test]
    fn prune_callback_semantics() {
        let t = TestRepo::new();
        let a = t.commit("a", 1000, &[]);
        let b = t.commit("b", 1000, &[a]);
        let c = t.commit("c", 1000, &[a]);
        let m = t.commit("m", 1000, &[b, c]);

        // A commit reachable via both a pruned and an unpruned path IS yielded: prune
        // does not propagate (RW:428-429 is pure insertion pruning).
        let odb = t.repo.odb().unwrap();
        let mut w = RevWalk::new(&odb);
        w.push(m).unwrap();
        let out = w.into_topo_vec(|oid| oid == b).unwrap();
        assert!(
            out.contains(&a),
            "a reachable via unpruned c must be yielded"
        );
        assert!(!out.contains(&b));

        // A pruned pushed tip still explores its parents (RW:482 runs before RW:495).
        let mut w = RevWalk::new(&odb);
        w.push(m).unwrap();
        let out = w.into_topo_vec(|oid| oid == m).unwrap();
        assert!(!out.contains(&m));
        assert!(out.contains(&b) && out.contains(&c) && out.contains(&a));

        // A commit reachable ONLY through pruned commits is not yielded.
        let mut w = RevWalk::new(&odb);
        w.push(m).unwrap();
        let out = w.into_topo_vec(|oid| oid == b || oid == c).unwrap();
        assert_eq!(out, vec![m]);

        // Differential versions of the same three cases (incl. callback sequences,
        // which prove the per-child-edge multiple invocations match).
        assert_parity(
            &t.repo,
            &[Input::Push(m)],
            Some(&HashSet::from([b])),
            "prune-one-path",
        );
        assert_parity(
            &t.repo,
            &[Input::Push(m)],
            Some(&HashSet::from([m])),
            "prune-tip",
        );
        assert_parity(
            &t.repo,
            &[Input::Push(m)],
            Some(&HashSet::from([b, c])),
            "prune-both-paths",
        );
        // Prune interacting with hide.
        assert_parity(
            &t.repo,
            &[Input::Push(m), Input::Hide(c)],
            Some(&HashSet::from([b])),
            "prune-plus-hide",
        );
    }

    #[test]
    fn push_hide_error_parity() {
        let t = TestRepo::new();
        let a = t.commit("a", 1000, &[]);
        let missing = git2::Oid::from_str("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();
        let blob = t
            .repo
            .odb()
            .unwrap()
            .write(git2::ObjectType::Blob, b"x")
            .unwrap();
        let tree = t.repo.treebuilder(None).unwrap().write().unwrap();

        // Missing oid errors at push/hide time; non-commits error too. assert_parity
        // covers the both-err case; also pin that OUR side errors immediately.
        let odb = t.repo.odb().unwrap();
        assert!(RevWalk::new(&odb).push(missing).is_err());
        assert!(RevWalk::new(&odb).hide(missing).is_err());
        assert!(RevWalk::new(&odb).push(blob).is_err());
        assert!(RevWalk::new(&odb).hide(tree).is_err());
        assert_parity(&t.repo, &[Input::Push(missing)], None, "push-missing");
        assert_parity(
            &t.repo,
            &[Input::Push(a), Input::Hide(missing)],
            None,
            "hide-missing",
        );
        assert_parity(&t.repo, &[Input::Push(blob)], None, "push-blob");
        assert_parity(
            &t.repo,
            &[Input::Push(a), Input::Hide(tree)],
            None,
            "hide-tree",
        );
    }

    #[test]
    fn annotated_tag_divergence_is_pinned() {
        // Documented divergence: git2 peels annotated tags to their commit at
        // push/hide (RW:55) while we reject any non-commit input. Pin both sides so
        // a refactor cannot silently start accepting the tag (and yielding its oid).
        let t = TestRepo::new();
        let a = t.commit("a", 1000, &[]);
        let b = t.commit("b", 1000, &[a]);
        let sig =
            git2::Signature::new("Test", "test@example.com", &git2::Time::new(1000, 0)).unwrap();
        let obj = t.repo.find_object(b, None).unwrap();
        let tag = t.repo.tag("v1", &obj, &sig, "annotated", false).unwrap();

        let mut walk = t.repo.revwalk().unwrap();
        walk.set_sorting(git2::Sort::TOPOLOGICAL).unwrap();
        walk.push(tag).unwrap();
        let theirs: Vec<git2::Oid> = walk.map(|r| r.unwrap()).collect();
        assert_eq!(
            theirs,
            vec![b, a],
            "git2 yields the peeled commit's history"
        );

        let odb = t.repo.odb().unwrap();
        assert!(RevWalk::new(&odb).push(tag).is_err());
        assert!(RevWalk::new(&odb).hide(tag).is_err());
    }

    #[test]
    fn committer_without_timestamp_matches_git2() {
        // A committer line with no timestamp at all is valid in libgit2 (time stays
        // 0, signature.c); the commit must walk fine and order as time 0.
        let t = TestRepo::new();
        let base = t.commit("base", 1000, &[]);
        let weird = t.raw_commit_tail(
            &[base],
            "author Test <test@example.com> 1000 +0000\n\
             committer Test <test@example.com>\n\nweird\n",
        );
        let tip = t.raw_commit("tip", 1000, &[weird]);
        assert_parity(&t.repo, &[Input::Push(tip)], None, "committer-no-time");
        assert_parity(
            &t.repo,
            &[Input::Push(tip), Input::Hide(base)],
            None,
            "committer-no-time hide",
        );

        let odb = t.repo.odb().unwrap();
        let mut w = RevWalk::new(&odb);
        w.push(tip).unwrap();
        assert_eq!(w.into_topo_vec(|_| false).unwrap(), vec![tip, weird, base]);
    }

    #[test]
    fn truncated_commit_errors_like_git2() {
        // An object truncated before the committer line: libgit2's quick-parse
        // errors ("no newline given"), while the token iterator just ends -- parse
        // must turn that silent end into an error.
        let t = TestRepo::new();
        let trunc = t.raw_commit_tail(&[], "");
        let tip = t.raw_commit("tip", 1000, &[trunc]);
        let odb = t.repo.odb().unwrap();
        let mut w = RevWalk::new(&odb);
        w.push(tip).unwrap();
        assert!(w.into_topo_vec(|_| false).is_err());
        assert_parity(&t.repo, &[Input::Push(tip)], None, "truncated-parent");
        // Pushing the truncated commit itself: git2 errors at push (full parse),
        // we error during the walk -- both funnel to Err.
        assert_parity(&t.repo, &[Input::Push(trunc)], None, "truncated-tip");
    }

    #[test]
    fn missing_author_line_divergence_is_pinned() {
        // Documented divergence: quick-parse skips the mandatory author parse
        // (commit.c under GIT_COMMIT_PARSE_QUICK) so git2 walks an ancestor with no
        // author line; `CommitRefIter` requires it and the walk errors.
        let t = TestRepo::new();
        let weird = t.raw_commit_tail(&[], "committer Test <test@example.com> 1000 +0000\n\nx\n");
        let tip = t.raw_commit("tip", 1000, &[weird]);
        let mut walk = t.repo.revwalk().unwrap();
        walk.set_sorting(git2::Sort::TOPOLOGICAL).unwrap();
        walk.push(tip).unwrap();
        let theirs: Vec<git2::Oid> = walk.map(|r| r.unwrap()).collect();
        assert_eq!(theirs, vec![tip, weird]);
        let odb = t.repo.odb().unwrap();
        let mut w = RevWalk::new(&odb);
        w.push(tip).unwrap();
        assert!(w.into_topo_vec(|_| false).is_err());
    }

    #[test]
    fn duplicate_parent_edges_match_git2() {
        // "parent X" listed twice: each edge counts in the topo in-degree pass
        // (RW:561-567) and gets its own prune-callback consult (RW:428-429).
        let t = TestRepo::new();
        let a = t.commit("a", 1000, &[]);
        let b = t.commit("b", 1000, &[a]);
        let m = t.raw_commit("m", 1000, &[b, b, a]);
        assert_parity(&t.repo, &[Input::Push(m)], None, "dup-edges plain");
        assert_parity(
            &t.repo,
            &[Input::Push(m), Input::Hide(b)],
            None,
            "dup-edges hide",
        );
        for pruned in [
            HashSet::from([b]),
            HashSet::from([a]),
            HashSet::from([b, a]),
        ] {
            assert_parity(&t.repo, &[Input::Push(m)], Some(&pruned), "dup-edges prune");
        }
    }

    #[test]
    fn pruned_pushed_ancestor_tip_matches_git2() {
        // A pushed tip that is an ancestor of another pushed tip meets the prune
        // callback at three distinct points: prepare seeds it into the frontier
        // unpruned (RW:646-655), the parent edge consults the callback before the
        // seen check (RW:425-433), and emission checks it again (RW:495-496). The
        // call-sequence assert pins that interaction.
        let t = TestRepo::new();
        let a = t.commit("a", 1000, &[]);
        let b = t.commit("b", 1000, &[a]);
        let c = t.commit("c", 1000, &[b]);
        for inputs in [
            vec![Input::Push(b), Input::Push(c)],
            vec![Input::Push(c), Input::Push(b)],
        ] {
            for pruned in [
                HashSet::from([b]),
                HashSet::from([c]),
                HashSet::from([b, c]),
            ] {
                assert_parity(
                    &t.repo,
                    &inputs,
                    Some(&pruned),
                    &format!("pruned-ancestor-tip {inputs:?} {pruned:?}"),
                );
            }
        }
    }

    #[test]
    fn missing_ancestors_error_during_walk_not_at_hide() {
        let t = TestRepo::new();
        let tip = t.commit("tip", 1000, &[]);
        let missing = git2::Oid::from_str("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();
        let dangling = t.raw_commit("dangling", 1000, &[missing]);

        // hide() of a valid commit with missing ancestors succeeds; the walk errors.
        let odb = t.repo.odb().unwrap();
        let mut w = RevWalk::new(&odb);
        w.push(tip).unwrap();
        w.hide(dangling).unwrap();
        assert!(w.into_topo_vec(|_| false).is_err());
        assert_parity(
            &t.repo,
            &[Input::Push(tip), Input::Hide(dangling)],
            None,
            "hide-dangling",
        );

        // Same for a pushed commit with a missing parent (interesting-path parse).
        let mut w = RevWalk::new(&odb);
        w.push(dangling).unwrap();
        assert!(w.into_topo_vec(|_| false).is_err());
        assert_parity(&t.repo, &[Input::Push(dangling)], None, "push-dangling");
    }

    #[test]
    fn hides_without_pushes_and_push_hide_overlap() {
        let t = TestRepo::new();
        let a = t.commit("a", 1000, &[]);
        let b = t.commit("b", 1000, &[a]);

        // Hides but no pushes: empty walk (RW:623-627).
        assert_parity(&t.repo, &[Input::Hide(b)], None, "hide-only");
        // Push after hide of the same oid is a no-op (RW:76-78): still empty.
        assert_parity(
            &t.repo,
            &[Input::Hide(b), Input::Push(b)],
            None,
            "push-after-hide",
        );
        // Hide after push flips the commit to hidden.
        assert_parity(
            &t.repo,
            &[Input::Push(b), Input::Hide(b)],
            None,
            "hide-after-push",
        );
        // Hide the tip while pushing its parent's history via another path.
        assert_parity(
            &t.repo,
            &[Input::Push(b), Input::Hide(a)],
            None,
            "hide-parent",
        );
    }

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            // splitmix64: deterministic, no external deps, no wall-clock.
            self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            z ^ (z >> 31)
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    #[test]
    fn fuzz_random_dags_match_git2() {
        for seed in 0..32u64 {
            let mut rng = Rng(seed);
            let t = TestRepo::new();
            let n = 20 + rng.below(30);
            // Heavy duplicates plus mild and wild skew in both directions.
            let times = [
                1_000_000i64,
                1_000_000,
                1_000_000,
                1_000_060,
                999_940,
                1_500_000,
            ];
            let mut commits: Vec<git2::Oid> = Vec::new();
            for i in 0..n {
                let time = times[rng.below(times.len())];
                let parents: Vec<git2::Oid> = if commits.is_empty() || rng.below(12) == 0 {
                    vec![]
                } else {
                    let k = 1 + rng.below(3.min(commits.len()));
                    let mut ps = Vec::new();
                    for _ in 0..k {
                        let p = commits[rng.below(commits.len())];
                        if !ps.contains(&p) {
                            ps.push(p);
                        }
                    }
                    ps
                };
                commits.push(t.commit(&format!("c{i}"), time, &parents));
            }
            let mut inputs = Vec::new();
            for _ in 0..1 + rng.below(3) {
                inputs.push(Input::Push(commits[rng.below(commits.len())]));
            }
            for _ in 0..rng.below(3) {
                inputs.push(Input::Hide(commits[rng.below(commits.len())]));
            }
            for _ in 0..inputs.len() {
                let (a, b) = (rng.below(inputs.len()), rng.below(inputs.len()));
                inputs.swap(a, b);
            }
            let pruned: HashSet<git2::Oid> = commits
                .iter()
                .copied()
                .filter(|_| rng.below(6) == 0)
                .collect();
            assert_parity(&t.repo, &inputs, None, &format!("fuzz seed={seed}"));
            assert_parity(
                &t.repo,
                &inputs,
                Some(&pruned),
                &format!("fuzz seed={seed} pruned"),
            );
        }
    }

    #[test]
    fn discover_visits_in_frontier_insertion_order() {
        let t = TestRepo::new();
        let a = t.commit("a", 100, &[]);
        let b = t.commit("b", 300, &[a]);
        let c = t.commit("c", 200, &[a]);
        let m = t.commit("m", 400, &[b, c]);
        let odb = t.repo.odb().unwrap();

        // Tip first; then parents of each date-desc-popped commit in parent order:
        // pop m inserts b then c, pop b (newest) inserts a, pop c/a insert nothing.
        let mut w = RevWalk::new(&odb);
        w.push(m).unwrap();
        let mut visited = Vec::new();
        w.discover(|oid| {
            visited.push(oid);
            Ok(ControlFlow::Continue(()))
        })
        .unwrap();
        assert_eq!(visited, vec![m, b, c, a]);

        // Break aborts the walk.
        let mut w = RevWalk::new(&odb);
        w.push(m).unwrap();
        let mut visited = Vec::new();
        w.discover(|oid| {
            visited.push(oid);
            Ok(if oid == c {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            })
        })
        .unwrap();
        assert_eq!(visited, vec![m, b, c]);

        // first_parent confines discovery to the first-parent chain.
        let mut w = RevWalk::new(&odb);
        w.push(m).unwrap();
        w.simplify_first_parent();
        let mut visited = Vec::new();
        w.discover(|oid| {
            visited.push(oid);
            Ok(ControlFlow::Continue(()))
        })
        .unwrap();
        assert_eq!(visited, vec![m, b, a]);

        // Errors from the visit callback propagate.
        let mut w = RevWalk::new(&odb);
        w.push(m).unwrap();
        assert!(w.discover(|_| anyhow::bail!("boom")).is_err());

        // No pushes: no visits, clean return.
        let w = RevWalk::new(&odb);
        w.discover(|_| panic!("must not be visited")).unwrap();
    }

    #[test]
    fn discover_order_on_equal_time_ties() {
        // All-equal timestamps are the norm in josh's test repos, and discover's
        // visit order is the tie-break source for find_unapply_base's pick -- pin
        // it explicitly (there is no git2 reference walk to diff against): ties
        // keep frontier insertion order (insert_by_date passes over equal entries),
        // parents visit in parent-index order.
        let t = TestRepo::new();
        let a = t.commit("a", 1000, &[]);
        let b = t.commit("b", 1000, &[a]);
        let c = t.commit("c", 1000, &[a]);
        let m = t.commit("m", 1000, &[b, c]);
        let odb = t.repo.odb().unwrap();
        let mut w = RevWalk::new(&odb);
        w.push(m).unwrap();
        let mut visited = Vec::new();
        w.discover(|oid| {
            visited.push(oid);
            Ok(ControlFlow::Continue(()))
        })
        .unwrap();
        assert_eq!(visited, vec![m, b, c, a]);

        // Criss-cross, all one timestamp: pop m inserts x,y; pop x inserts a,b
        // (y precedes them in the frontier but was inserted first); pop y adds
        // nothing (both parents seen); pop a inserts root.
        let t = TestRepo::new();
        let root = t.commit("root", 1000, &[]);
        let a = t.commit("a", 1000, &[root]);
        let b = t.commit("b", 1000, &[root]);
        let x = t.commit("x", 1000, &[a, b]);
        let y = t.commit("y", 1000, &[b, a]);
        let m = t.commit("m", 1000, &[x, y]);
        let odb = t.repo.odb().unwrap();
        let mut w = RevWalk::new(&odb);
        w.push(m).unwrap();
        let mut visited = Vec::new();
        w.discover(|oid| {
            visited.push(oid);
            Ok(ControlFlow::Continue(()))
        })
        .unwrap();
        assert_eq!(visited, vec![m, x, y, a, b, root]);
    }
}
