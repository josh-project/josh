//! josh's revision walkers, working over raw odb bytes: [`RevWalk`] for
//! pruned topological walks and [`RangeWalk`] for `base..tip` deltas.
//!
//! josh's walks need two things no gitoxide traversal provides:
//!
//! - Traversal pruning. walk2 and ensure_hint_cached make incremental walks
//!   cheap by skipping already-processed subgraphs (the prune callback).
//!   `gix_traverse::commit::topo::Builder` cannot do this: `with_predicate`
//!   only filters yields after parent expansion.
//! - Laziness without a commit-graph. josh repos have no commit-graph file, and
//!   without one the gix topo builder assigns every commit generation INFINITY
//!   and drains the entire reachable graph before the first yield.
//!   [`RevWalk::discover`] streams visits with early exit.
//!
//! Commits are parsed only up to their parent headers (one raw odb read, one
//! `CommitRefIter` pass; committer lines are never read), so malformed
//! author/committer lines (fsck-invalid objects) do not affect a walk. Tags
//! are NOT peeled: any non-commit input is an error (josh only ever walks
//! commit oids; the git2 walks this module replaces peeled).
//!
//! Differential tests pin the yield sets against an independent reference model, and pin
//! topological validity and order determinism, over fixed shapes and seeded random DAGs.

use std::collections::{BinaryHeap, HashMap};
use std::ops::ControlFlow;

type Visit<'v> = &'v mut dyn FnMut(gix_hash::ObjectId) -> anyhow::Result<ControlFlow<()>>;

/// Read a commit's parent ids: one raw object read, one `CommitRefIter` pass
/// stopped at the first non-tree/parent header. A missing or non-commit
/// object is a hard error; headers past the parents (author, committer,
/// message) are never read or validated.
fn read_parent_oids(
    src: &impl gix_object::Find,
    oid: gix_hash::ObjectId,
    buf: &mut Vec<u8>,
) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
    let data = src
        .try_find(&oid, buf)
        .map_err(|e| anyhow::anyhow!("read_parent_oids: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("object {} not found", oid))?;
    if data.kind != gix_object::Kind::Commit {
        anyhow::bail!("object {} is not a commit", oid);
    }
    let mut parents = Vec::new();
    for token in gix_object::CommitRefIter::from_bytes(data.data, gix_hash::Kind::Sha1) {
        use gix_object::commit::ref_iter::Token;
        match token? {
            Token::Tree { .. } => {}
            Token::Parent { id } => parents.push(id),
            _ => break,
        }
    }
    Ok(parents)
}

/// Verify `oid` names a commit without decompressing it.
fn ensure_commit(src: &impl gix_object::FindHeader, oid: gix_hash::ObjectId) -> anyhow::Result<()> {
    let header = src
        .try_header(&oid)
        .map_err(|e| anyhow::anyhow!("ensure_commit: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("object {} not found", oid))?;
    if header.kind != gix_object::Kind::Commit {
        anyhow::bail!("object {} is not a commit", oid);
    }
    Ok(())
}

/// Per-commit state of a [`RevWalk`].
struct Node {
    oid: gix_hash::ObjectId,
    /// Parents as node indices, filled in on first visit.
    parents: Vec<usize>,
    parsed: bool,
    visited: bool,
}

/// A topological walk of everything reachable from the pushed tips, with
/// per-commit pruning. One-shot: configure via [`push`]/
/// [`simplify_first_parent`], then consume with [`into_topo_vec`] or
/// [`discover`].
///
/// - Yield set: the commits reachable from the tips, minus pruned commits and
///   anything only reachable through them. `prune` is consulted once per
///   commit: a pruned commit is not yielded and its parents are not explored.
///   Pruning expresses context-free knowledge ("this subgraph is already
///   processed"), which josh's caches answer per commit and which is
///   ancestor-closed by construction, so cutting there is sound. For
///   excluding a specific history ("everything reachable from that tip") use
///   [`RangeWalk`] instead.
/// - Yield order: a topological order (children before parents), deterministic
///   and defined as reverse DFS postorder -- tips in push order, parents in
///   parent order. Filtered commit content does not depend on this order
///   (filtering a commit is a pure function of the commit and its filtered
///   parents, applied parents-first), but first-match callers
///   (find_unapply_base, find_original, find_oldest_similar_commit) pick among
///   equivalent candidates by it, so it is pinned by unit tests below.
///
/// [`push`]: RevWalk::push
/// [`simplify_first_parent`]: RevWalk::simplify_first_parent
/// [`into_topo_vec`]: RevWalk::into_topo_vec
/// [`discover`]: RevWalk::discover
pub struct RevWalk<'a, S> {
    odb: &'a S,
    nodes: Vec<Node>,
    by_oid: HashMap<gix_hash::ObjectId, usize>,
    /// Pushed tips in push order; the DFS explores them in this order.
    tips: Vec<usize>,
    first_parent: bool,
    /// Reused per-commit read buffer, so a walk does one allocation, not one per commit.
    scratch: Vec<u8>,
}

impl<'a, S: gix_object::Find + gix_object::FindHeader> RevWalk<'a, S> {
    pub fn new(odb: &'a S) -> Self {
        RevWalk {
            odb,
            nodes: Vec::new(),
            by_oid: HashMap::new(),
            tips: Vec::new(),
            first_parent: false,
            scratch: Vec::new(),
        }
    }

    /// Mark a commit to start traversal from. A missing oid or a non-commit
    /// object errors immediately.
    pub fn push(&mut self, tip: gix_hash::ObjectId) -> anyhow::Result<()> {
        ensure_commit(self.odb, tip)?;
        let tip = self.intern(tip);
        self.tips.push(tip);
        Ok(())
    }

    /// Only the first parent of each commit is explored.
    pub fn simplify_first_parent(&mut self) {
        self.first_parent = true;
    }

    /// The complete walk in forward topological order (children before
    /// parents); iterate the result back to front for parents-first order.
    pub fn into_topo_vec(
        mut self,
        mut prune: impl FnMut(gix_hash::ObjectId) -> bool,
    ) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
        let post = self.dfs(&mut prune, &mut None)?;
        Ok(post.into_iter().rev().map(|i| self.nodes[i].oid).collect())
    }

    /// Lazy discovery iteration: streams oids in DFS pre-order (each commit
    /// visited before its parents are explored). `visit` returns
    /// [`ControlFlow::Break`] to abort the walk.
    pub fn discover(
        mut self,
        mut visit: impl FnMut(gix_hash::ObjectId) -> anyhow::Result<ControlFlow<()>>,
    ) -> anyhow::Result<()> {
        let mut keep_all = |_: gix_hash::ObjectId| false;
        let visit: Visit<'_> = &mut visit;
        self.dfs(&mut keep_all, &mut Some(visit))?;
        Ok(())
    }

    /// The node index for `oid`, creating blank unparsed state on first sight.
    fn intern(&mut self, oid: gix_hash::ObjectId) -> usize {
        if let Some(&i) = self.by_oid.get(&oid) {
            return i;
        }
        let i = self.nodes.len();
        self.nodes.push(Node {
            oid,
            parents: Vec::new(),
            parsed: false,
            visited: false,
        });
        self.by_oid.insert(oid, i);
        i
    }

    fn parse(&mut self, commit: usize) -> anyhow::Result<()> {
        if self.nodes[commit].parsed {
            return Ok(());
        }
        let parents = read_parent_oids(self.odb, self.nodes[commit].oid, &mut self.scratch)?
            .into_iter()
            .map(|p| self.intern(p))
            .collect();
        let node = &mut self.nodes[commit];
        node.parents = parents;
        node.parsed = true;
        Ok(())
    }

    /// Iterative DFS from the tips (in push order, parents in parent order),
    /// skipping pruned commits, collecting postorder -- reversed by the
    /// caller into forward topological order. `visit` fires in pre-order; its
    /// `Break` aborts the walk with the partial result.
    fn dfs(
        &mut self,
        prune: &mut dyn FnMut(gix_hash::ObjectId) -> bool,
        visit: &mut Option<Visit<'_>>,
    ) -> anyhow::Result<Vec<usize>> {
        enum Frame {
            Explore(usize),
            Emit(usize),
        }
        let mut post = Vec::new();
        let mut stack: Vec<Frame> = self.tips.iter().rev().map(|&t| Frame::Explore(t)).collect();
        while let Some(frame) = stack.pop() {
            match frame {
                Frame::Explore(c) => {
                    if self.nodes[c].visited {
                        continue;
                    }
                    self.nodes[c].visited = true;
                    if prune(self.nodes[c].oid) {
                        continue;
                    }
                    if let Some(v) = visit.as_mut() {
                        if v(self.nodes[c].oid)?.is_break() {
                            return Ok(post);
                        }
                    }
                    self.parse(c)?;
                    stack.push(Frame::Emit(c));
                    let n = if self.first_parent {
                        self.nodes[c].parents.len().min(1)
                    } else {
                        self.nodes[c].parents.len()
                    };
                    for i in (0..n).rev() {
                        stack.push(Frame::Explore(self.nodes[c].parents[i]));
                    }
                }
                Frame::Emit(c) => post.push(c),
            }
        }
        Ok(post)
    }
}

/// A max-heap ordered by generation with stable tie-breaking.
pub struct GenerationFrontier<T>(BinaryHeap<(u64, std::cmp::Reverse<u64>, T)>, u64);
impl<T: Ord> GenerationFrontier<T> {
    pub fn new() -> Self {
        Self(BinaryHeap::new(), 0)
    }
    #[inline(always)]
    pub fn push(&mut self, generation: u64, value: T) {
        self.0.push((generation, std::cmp::Reverse(self.1), value));
        self.1 += 1;
    }
    #[inline(always)]
    pub fn pop(&mut self) -> Option<T> {
        self.0.pop().map(|(_, _, value)| value)
    }
}

/// Probe budget for the fast-forward path of a [`RangeWalk`], after which the
/// walk switches to the ranked strategy.
const FAST_FORWARD_PROBE_LIMIT: usize = 1000;

/// Per-commit state of a [`RangeWalk`].
struct RangeNode {
    oid: gix_hash::ObjectId,
    /// Parents as node indices, filled in on first visit.
    parents: Vec<usize>,
    parsed: bool,
    hidden: bool,
    /// Entered the ranked frontier (each commit is ranked once).
    enqueued: bool,
    /// Currently pending in the ranked frontier as a visible entry; cleared
    /// when a hidden mark arrives first or when the entry pops.
    queued_visible: bool,
}

/// The commits reachable from a tip but not from a base -- git's `base..tip`
/// -- in a topological order (children before parents). The base expresses a
/// contextual reachability boundary ("everything reachable from that tip"),
/// which no per-commit cache lookup can answer; that is what distinguishes
/// this from [`RevWalk`]'s pruning and why it is its own walker.
///
/// Construction requires josh's sequence numbers (a commit's number is
/// strictly greater than every parent's; the lookup lives in josh-core's
/// cache, above this crate, hence the function handle). The walk runs in one
/// of two ways, both exact:
///
/// - Fast-forward path: the base is an ancestor of the tip along a
///   single-parent chain -- the everyday shape of unapply_filter pushes. The
///   chain from the tip down to the base is exactly the answer: the base is
///   an ancestor of every chain commit, so no chain commit can also be an
///   ancestor of the base (they would be ancestors of each other), and
///   neither the base's ancestry nor the sequence numbers are touched at
///   all. Cost is O(delta).
/// - Ranked walk otherwise: process the frontier highest-sequence-number
///   first, spreading hidden marks parent-ward from the base. Numbers
///   strictly decrease along parent edges, so every commit pops after all
///   its children -- hidden marks provably arrive before the pop -- and the
///   walk stops the moment only hidden entries are pending: everything
///   undiscovered is then an ancestor of hidden commits only. The walk stops
///   at the merge-base level: O(delta + boundary). Emission order (highest
///   number first) is already forward topological order. Lookup failures
///   fail the walk.
///
/// The two strategies yield different topological orders, which is fine
/// because the callers consume the whole yield set parents-first.
pub struct RangeWalk<'a, S> {
    odb: &'a S,
    sequence_numbers: Box<dyn Fn(gix_hash::ObjectId) -> anyhow::Result<u64> + 'a>,
    nodes: Vec<RangeNode>,
    by_oid: HashMap<gix_hash::ObjectId, usize>,
    /// Reused per-commit read buffer, so a walk does one allocation, not one per commit.
    scratch: Vec<u8>,
}

impl<'a, S: gix_object::Find + gix_object::FindHeader> RangeWalk<'a, S> {
    pub fn new(
        odb: &'a S,
        sequence_numbers: impl Fn(gix_hash::ObjectId) -> anyhow::Result<u64> + 'a,
    ) -> Self {
        RangeWalk {
            odb,
            sequence_numbers: Box::new(sequence_numbers),
            nodes: Vec::new(),
            by_oid: HashMap::new(),
            scratch: Vec::new(),
        }
    }

    /// The commits of `base..tip` in forward topological order (children
    /// before parents); iterate the result back to front for parents-first
    /// order. A missing oid or a non-commit object errors immediately; a
    /// commit with missing ancestors errors if (and only if) the walk
    /// actually reaches them.
    pub fn into_topo_vec(
        mut self,
        tip: gix_hash::ObjectId,
        base: gix_hash::ObjectId,
    ) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
        ensure_commit(self.odb, tip)?;
        ensure_commit(self.odb, base)?;
        if tip == base {
            return Ok(Vec::new());
        }
        let tip = self.intern(tip);
        let base = self.intern(base);
        self.nodes[base].hidden = true;
        if let Some(chain) = self.fast_forward_walk(tip, base)? {
            return Ok(chain);
        }
        self.ranked_walk(tip, base)
    }

    /// The node index for `oid`, creating blank unparsed state on first sight.
    fn intern(&mut self, oid: gix_hash::ObjectId) -> usize {
        if let Some(&i) = self.by_oid.get(&oid) {
            return i;
        }
        let i = self.nodes.len();
        self.nodes.push(RangeNode {
            oid,
            parents: Vec::new(),
            parsed: false,
            hidden: false,
            enqueued: false,
            queued_visible: false,
        });
        self.by_oid.insert(oid, i);
        i
    }

    fn parse(&mut self, commit: usize) -> anyhow::Result<()> {
        if self.nodes[commit].parsed {
            return Ok(());
        }
        let parents = read_parent_oids(self.odb, self.nodes[commit].oid, &mut self.scratch)?
            .into_iter()
            .map(|p| self.intern(p))
            .collect();
        let node = &mut self.nodes[commit];
        node.parents = parents;
        node.parsed = true;
        Ok(())
    }

    /// Follow the parent chain from the tip; if it runs into the base within
    /// the probe budget, the chain is exactly the yield set, already in
    /// forward topological order. Returns `None` -- caller switches to the
    /// ranked walk -- on a merge commit, a root, or an exhausted budget.
    fn fast_forward_walk(
        &mut self,
        tip: usize,
        base: usize,
    ) -> anyhow::Result<Option<Vec<gix_hash::ObjectId>>> {
        let mut out = Vec::new();
        let mut c = tip;
        for _ in 0..FAST_FORWARD_PROBE_LIMIT {
            if c == base {
                return Ok(Some(out));
            }
            self.parse(c)?;
            let parents = &self.nodes[c].parents;
            let next = match parents.len() {
                1 => parents[0],
                _ => return Ok(None),
            };
            out.push(self.nodes[c].oid);
            c = next;
        }
        Ok(None)
    }

    fn ranked_walk(&mut self, tip: usize, base: usize) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
        let seq_of = std::mem::replace(&mut self.sequence_numbers, Box::new(|_| unreachable!()));
        let mut frontier = GenerationFrontier::new();
        let mut visible_pending = 0usize;
        let mut out = Vec::new();

        for c in [tip, base] {
            self.nodes[c].enqueued = true;
            if !self.nodes[c].hidden {
                self.nodes[c].queued_visible = true;
                visible_pending += 1;
            }
            frontier.push(seq_of(self.nodes[c].oid)?, c);
        }

        while visible_pending > 0 {
            let Some(c) = frontier.pop() else {
                break;
            };
            if self.nodes[c].hidden {
                // Hidden pop: spread hiding to all parents and keep them
                // ranked so the marks keep travelling frontier-first.
                self.parse(c)?;
                for i in 0..self.nodes[c].parents.len() {
                    let p = self.nodes[c].parents[i];
                    if !self.nodes[p].hidden {
                        self.nodes[p].hidden = true;
                        if self.nodes[p].queued_visible {
                            self.nodes[p].queued_visible = false;
                            visible_pending -= 1;
                        }
                    }
                    if !self.nodes[p].enqueued {
                        self.nodes[p].enqueued = true;
                        frontier.push(seq_of(self.nodes[p].oid)?, p);
                    }
                }
            } else {
                self.nodes[c].queued_visible = false;
                visible_pending -= 1;
                out.push(self.nodes[c].oid);
                self.parse(c)?;
                for i in 0..self.nodes[c].parents.len() {
                    let p = self.nodes[c].parents[i];
                    if !self.nodes[p].enqueued {
                        self.nodes[p].enqueued = true;
                        if !self.nodes[p].hidden {
                            self.nodes[p].queued_visible = true;
                            visible_pending += 1;
                        }
                        frontier.push(seq_of(self.nodes[p].oid)?, p);
                    }
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::str::FromStr;

    struct TestRepo {
        _dir: tempfile::TempDir,
        repo: gix::Repository,
        tree: gix_hash::ObjectId,
    }

    impl TestRepo {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = gix::init_bare(dir.path()).unwrap();
            let tree = gix_object::Write::write(
                &repo.objects,
                &gix_object::Tree {
                    entries: Vec::new(),
                },
            )
            .unwrap();
            TestRepo {
                _dir: dir,
                repo,
                tree,
            }
        }

        fn commit(&self, msg: &str, parents: &[gix_hash::ObjectId]) -> gix_hash::ObjectId {
            let signature = gix_actor::Signature {
                name: "Test".into(),
                email: "test@example.com".into(),
                time: gix_actor::date::Time {
                    seconds: 1000,
                    offset: 0,
                },
            };
            crate::write_commit(
                &self.repo.objects,
                self.tree,
                parents,
                &signature,
                &signature,
                msg,
            )
            .unwrap()
        }

        /// Write raw commit bytes so parents don't have to exist in the odb
        /// (and may repeat).
        fn raw_commit(&self, msg: &str, parents: &[gix_hash::ObjectId]) -> gix_hash::ObjectId {
            let mut buf = format!("tree {}\n", self.tree);
            for p in parents {
                buf.push_str(&format!("parent {}\n", p));
            }
            buf.push_str(&format!(
                "author Test <test@example.com> 1000 +0000\n\
                 committer Test <test@example.com> 1000 +0000\n\n{msg}\n"
            ));
            gix_object::Write::write_buf(
                &self.repo.objects,
                gix_object::Kind::Commit,
                buf.as_bytes(),
            )
            .unwrap()
        }
    }

    fn parents(repo: &gix::Repository, oid: gix_hash::ObjectId) -> Vec<gix_hash::ObjectId> {
        read_parent_oids(&repo.objects, oid, &mut Vec::new()).unwrap()
    }

    /// An exact sequence number over git objects: `1 + max(parent seqs)`,
    /// memoized -- the property josh's cached sequence numbers guarantee.
    /// Errors on missing ancestors, like the real lookup.
    fn exact_seq(
        repo: &gix::Repository,
        memo: &std::cell::RefCell<HashMap<gix_hash::ObjectId, u64>>,
        oid: gix_hash::ObjectId,
    ) -> anyhow::Result<u64> {
        if let Some(&s) = memo.borrow().get(&oid) {
            return Ok(s);
        }
        let mut max = 0;
        for p in read_parent_oids(&repo.objects, oid, &mut Vec::new())? {
            max = max.max(exact_seq(repo, memo, p)?);
        }
        memo.borrow_mut().insert(oid, max + 1);
        Ok(max + 1)
    }

    /// Independent reference model of both walkers' yield sets: the excluded
    /// closure is full ancestor reachability from `base`; the yield set is
    /// what a naive traversal from the tips reaches without stepping on
    /// excluded or pruned commits. It parses raw commit parents directly,
    /// sharing no traversal code with the walkers.
    fn reference_set(
        repo: &gix::Repository,
        tips: &[gix_hash::ObjectId],
        base: Option<gix_hash::ObjectId>,
        pruned: &HashSet<gix_hash::ObjectId>,
        first_parent: bool,
    ) -> HashSet<gix_hash::ObjectId> {
        let mut hidden: HashSet<gix_hash::ObjectId> = HashSet::new();
        let mut stack: Vec<gix_hash::ObjectId> = base.into_iter().collect();
        while let Some(h) = stack.pop() {
            if hidden.insert(h) {
                stack.extend(parents(repo, h));
            }
        }
        let mut out = HashSet::new();
        let mut stack: Vec<gix_hash::ObjectId> = tips.to_vec();
        while let Some(c) = stack.pop() {
            if out.contains(&c) || hidden.contains(&c) || pruned.contains(&c) {
                continue;
            }
            out.insert(c);
            let mut commit_parents = parents(repo, c);
            if first_parent {
                commit_parents.truncate(1);
            }
            stack.extend(commit_parents);
        }
        out
    }

    fn rev_walk(
        repo: &gix::Repository,
        tips: &[gix_hash::ObjectId],
        pruned: &HashSet<gix_hash::ObjectId>,
        first_parent: bool,
    ) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
        let mut walk = RevWalk::new(&repo.objects);
        if first_parent {
            walk.simplify_first_parent();
        }
        for &t in tips {
            walk.push(t)?;
        }
        walk.into_topo_vec(|oid| pruned.contains(&oid))
    }

    fn range_walk(
        repo: &gix::Repository,
        tip: gix_hash::ObjectId,
        base: gix_hash::ObjectId,
    ) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
        let memo = std::cell::RefCell::new(HashMap::new());
        let walk = RangeWalk::new(&repo.objects, |oid| exact_seq(repo, &memo, oid));
        walk.into_topo_vec(tip, base)
    }

    /// Check a yielded order for topological validity over the followed
    /// edges: every yielded commit precedes its yielded parents.
    fn assert_topo(
        repo: &gix::Repository,
        got: &[gix_hash::ObjectId],
        first_parent: bool,
        mode: &str,
    ) {
        let pos: HashMap<gix_hash::ObjectId, usize> =
            got.iter().enumerate().map(|(i, &o)| (o, i)).collect();
        assert_eq!(pos.len(), got.len(), "duplicate yields: {mode}");
        for &c in got {
            let mut commit_parents = parents(repo, c);
            if first_parent {
                commit_parents.truncate(1);
            }
            for p in commit_parents {
                if let Some(&pp) = pos.get(&p) {
                    assert!(pos[&c] < pp, "topology violation {c} -> {p}: {mode}");
                }
            }
        }
    }

    /// Assert [`RevWalk`] against the reference model: exact yield set, valid
    /// topological order, and run-to-run determinism.
    fn assert_rev_walk(
        repo: &gix::Repository,
        tips: &[gix_hash::ObjectId],
        pruned: &HashSet<gix_hash::ObjectId>,
        label: &str,
    ) {
        for first_parent in [false, true] {
            let mode = format!("{label} first_parent={first_parent}");
            let got = rev_walk(repo, tips, pruned, first_parent).unwrap();
            let want = reference_set(repo, tips, None, pruned, first_parent);
            let got_set: HashSet<gix_hash::ObjectId> = got.iter().copied().collect();
            assert_eq!(got_set, want, "yield set mismatch: {mode}");
            assert_topo(repo, &got, first_parent, &mode);
            let again = rev_walk(repo, tips, pruned, first_parent).unwrap();
            assert_eq!(got, again, "non-deterministic order: {mode}");
        }
    }

    /// Assert [`RangeWalk`] against the reference model: exact yield set,
    /// valid topological order, run-to-run determinism.
    fn assert_range_walk(
        repo: &gix::Repository,
        tip: gix_hash::ObjectId,
        base: gix_hash::ObjectId,
        label: &str,
    ) {
        let got = range_walk(repo, tip, base).unwrap();
        let want = reference_set(repo, &[tip], Some(base), &HashSet::new(), false);
        let got_set: HashSet<gix_hash::ObjectId> = got.iter().copied().collect();
        assert_eq!(got_set, want, "yield set mismatch: {label}");
        assert_topo(repo, &got, false, label);
        let again = range_walk(repo, tip, base).unwrap();
        assert_eq!(got, again, "non-deterministic order: {label}");
    }

    fn no_prune() -> HashSet<gix_hash::ObjectId> {
        HashSet::new()
    }

    #[test]
    fn linear_chain() {
        let t = TestRepo::new();
        let mut tip = t.commit("c0", &[]);
        let mut all = vec![tip];
        for i in 1..12 {
            tip = t.commit(&format!("c{i}"), &[tip]);
            all.push(tip);
        }
        assert_rev_walk(&t.repo, &[tip], &no_prune(), "linear");
        // Ranges at several depths: all take the fast-forward path.
        for &base in [all[0], all[5], all[10]].iter() {
            assert_range_walk(&t.repo, tip, base, "linear range");
        }
        // Forward order is tip-first down the chain.
        let got = range_walk(&t.repo, tip, all[5]).unwrap();
        let expect: Vec<gix_hash::ObjectId> = all[6..].iter().rev().copied().collect();
        assert_eq!(got, expect);
        // An empty range yields nothing.
        assert_eq!(
            range_walk(&t.repo, tip, tip).unwrap(),
            Vec::<gix_hash::ObjectId>::new()
        );
    }

    #[test]
    fn diamond_and_criss_cross() {
        let t = TestRepo::new();
        let a = t.commit("a", &[]);
        let b = t.commit("b", &[a]);
        let c = t.commit("c", &[a]);
        let m = t.commit("m", &[b, c]);
        assert_rev_walk(&t.repo, &[m], &no_prune(), "diamond");
        for base in [a, b, c] {
            assert_range_walk(&t.repo, m, base, "diamond range");
        }
        // Pinned order: DFS explores b's side to the bottom first, so the
        // forward (children-first) order emits m, then c, then b, then a.
        let got = rev_walk(&t.repo, &[m], &no_prune(), false).unwrap();
        assert_eq!(got, vec![m, c, b, a]);

        let t = TestRepo::new();
        let root = t.commit("root", &[]);
        let a = t.commit("a", &[root]);
        let b = t.commit("b", &[root]);
        let x = t.commit("x", &[a, b]);
        let y = t.commit("y", &[b, a]);
        let m = t.commit("m", &[x, y]);
        assert_rev_walk(&t.repo, &[m], &no_prune(), "criss-cross");
        assert_rev_walk(&t.repo, &[x, y], &no_prune(), "criss-cross two tips");
        assert_rev_walk(&t.repo, &[y, x], &no_prune(), "criss-cross two tips rev");
        for base in [a, b, x, y] {
            assert_range_walk(&t.repo, m, base, "criss-cross range");
        }
    }

    #[test]
    fn octopus_merge() {
        let t = TestRepo::new();
        let root = t.commit("root", &[]);
        let b1 = t.commit("b1", &[root]);
        let b2 = t.commit("b2", &[root]);
        let b3 = t.commit("b3", &[root]);
        let m = t.commit("m", &[b1, b2, b3]);
        assert_rev_walk(&t.repo, &[m], &no_prune(), "octopus");
        assert_range_walk(&t.repo, m, b2, "octopus range");
    }

    #[test]
    fn merge_of_old_side_branch_is_exact() {
        // A branch B forked below the base and merged into the tip: B's
        // commits are reachable from the fork point but NOT from the base, so
        // they must be yielded; the base's chain and everything below the
        // fork must not be.
        let t = TestRepo::new();
        let mut fork = t.commit("f0", &[]);
        for i in 0..30 {
            fork = t.commit(&format!("h{i}"), &[fork]);
        }
        let b = t.commit("b", &[fork]);
        let mut base = fork;
        for i in 0..10 {
            base = t.commit(&format!("o{i}"), &[base]);
        }
        let tip = t.commit("tip", &[base, b]);
        assert_range_walk(&t.repo, tip, base, "merged-side-branch");
        let got: HashSet<gix_hash::ObjectId> = range_walk(&t.repo, tip, base)
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(got, HashSet::from([tip, b]));
    }

    #[test]
    fn rebase_shape_is_exact() {
        // Sibling tips (a force-push shape): base and tip share a parent; the
        // fast-forward path bails at the root and the ranked walk yields
        // exactly the commits not reachable from the base.
        let t = TestRepo::new();
        let root = t.commit("root", &[]);
        let x = t.commit("x", &[root]);
        let base = t.commit("base", &[x]);
        let new1 = t.commit("new1", &[x]);
        let new2 = t.commit("new2", &[new1]);
        assert_range_walk(&t.repo, new2, base, "rebase");
        assert_eq!(range_walk(&t.repo, new2, base).unwrap(), vec![new2, new1]);
    }

    #[test]
    fn range_beyond_probe_limit() {
        // A delta longer than the probe budget: the fast-forward path gives
        // up and the ranked walk still produces the exact set.
        let t = TestRepo::new();
        let mut tip = t.commit("c0", &[]);
        let base_at = 3;
        let mut base = tip;
        for i in 1..(FAST_FORWARD_PROBE_LIMIT + 50) {
            tip = t.commit(&format!("c{i}"), &[tip]);
            if i == base_at {
                base = tip;
            }
        }
        let got = range_walk(&t.repo, tip, base).unwrap();
        assert_eq!(got.len(), FAST_FORWARD_PROBE_LIMIT + 50 - 1 - base_at);
        assert!(!got.contains(&base));
    }

    #[test]
    fn ranked_walk_is_bounded() {
        // Deep history below the fork, small delta above it, with a merge in
        // the delta so the fast-forward path bails. The ranked walk must stop
        // at the fork level: the lookup-call count stays proportional to the
        // delta, not the 600-commit history below.
        let t = TestRepo::new();
        let mut fork = t.commit("f0", &[]);
        for i in 0..600 {
            fork = t.commit(&format!("d{i}"), &[fork]);
        }
        let b = t.commit("b", &[fork]);
        let mut base = fork;
        for i in 0..5 {
            base = t.commit(&format!("o{i}"), &[base]);
        }
        let tip = t.commit("tip", &[base, b]);

        let memo = std::cell::RefCell::new(HashMap::new());
        let calls = std::cell::Cell::new(0usize);
        let walk = RangeWalk::new(&t.repo.objects, |oid| {
            calls.set(calls.get() + 1);
            exact_seq(&t.repo, &memo, oid)
        });
        let got = walk.into_topo_vec(tip, base).unwrap();
        assert_eq!(got, vec![tip, b]);
        assert!(
            calls.get() < 30,
            "ranked walk touched {} commits; expected O(delta), not O(history)",
            calls.get()
        );
    }

    #[test]
    fn missing_sequence_data_fails_the_walk() {
        // The lookup is fallible; failures surface instead of degrading. The
        // shape forces the ranked walk (merge in the delta).
        let t = TestRepo::new();
        let mut fork = t.commit("f0", &[]);
        for i in 0..20 {
            fork = t.commit(&format!("d{i}"), &[fork]);
        }
        let b = t.commit("b", &[fork]);
        let mut base = fork;
        for i in 0..4 {
            base = t.commit(&format!("o{i}"), &[base]);
        }
        let tip = t.commit("tip", &[base, b]);

        for blind in [tip, base, b] {
            let memo = std::cell::RefCell::new(HashMap::new());
            let walk = RangeWalk::new(&t.repo.objects, |oid| {
                if oid == blind {
                    anyhow::bail!("no sequence number for {}", oid);
                }
                exact_seq(&t.repo, &memo, oid)
            });
            assert!(
                walk.into_topo_vec(tip, base).is_err(),
                "blind spot {blind} must fail the walk"
            );
        }
    }

    #[test]
    fn prune_semantics() {
        let t = TestRepo::new();
        let a = t.commit("a", &[]);
        let b = t.commit("b", &[a]);
        let c = t.commit("c", &[a]);
        let m = t.commit("m", &[b, c]);

        // A commit reachable via both a pruned and an unpruned path IS
        // yielded; a commit reachable only through pruned commits is not; a
        // pruned tip yields nothing below it.
        assert_rev_walk(&t.repo, &[m], &HashSet::from([b]), "prune-one-path");
        assert_rev_walk(&t.repo, &[m], &HashSet::from([b, c]), "prune-both");
        assert_rev_walk(&t.repo, &[m], &HashSet::from([m]), "prune-tip");

        let got = rev_walk(&t.repo, &[m], &HashSet::from([b]), false).unwrap();
        assert!(
            got.contains(&a),
            "a reachable via unpruned c must be yielded"
        );
        assert!(!got.contains(&b));

        // The callback fires once per commit, even with duplicate edges.
        let dup = t.raw_commit("dup", &[b, b, a]);
        let mut calls = Vec::new();
        let mut w = RevWalk::new(&t.repo.objects);
        w.push(dup).unwrap();
        w.into_topo_vec(|oid| {
            calls.push(oid);
            false
        })
        .unwrap();
        let unique: HashSet<gix_hash::ObjectId> = calls.iter().copied().collect();
        assert_eq!(calls.len(), unique.len(), "prune consulted once per commit");
    }

    #[test]
    fn duplicate_parent_edges() {
        let t = TestRepo::new();
        let a = t.commit("a", &[]);
        let b = t.commit("b", &[a]);
        let m = t.raw_commit("m", &[b, b, a]);
        assert_rev_walk(&t.repo, &[m], &no_prune(), "dup-edges");
        assert_range_walk(&t.repo, m, b, "dup-edges range");
    }

    #[test]
    fn multiple_tips_and_overlapping_pushes() {
        let t = TestRepo::new();
        let root = t.commit("root", &[]);
        let a = t.commit("a", &[root]);
        let b = t.commit("b", &[root]);
        let c = t.commit("c", &[b]);
        for tips in [
            vec![a, c],
            vec![c, a],
            vec![a, a, c],
            vec![b, c],
            vec![c, b],
        ] {
            assert_rev_walk(&t.repo, &tips, &no_prune(), "multi-tip");
        }
        // A range between sibling branches: exact via the ranked walk.
        assert_range_walk(&t.repo, c, a, "sibling range");
        assert_range_walk(&t.repo, a, c, "sibling range rev");
    }

    #[test]
    fn input_error_cases() {
        let t = TestRepo::new();
        let a = t.commit("a", &[]);
        let missing =
            gix_hash::ObjectId::from_str("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();
        let blob =
            gix_object::Write::write_buf(&t.repo.objects, gix_object::Kind::Blob, b"x").unwrap();
        let tree = t.tree;
        let tag_bytes = format!(
            "object {a}\ntype commit\ntag v1\ntagger Test <test@example.com> 1000 +0000\n\nannotated"
        );
        let tag = gix_object::Write::write_buf(
            &t.repo.objects,
            gix_object::Kind::Tag,
            tag_bytes.as_bytes(),
        )
        .unwrap();

        let objects = &t.repo.objects;
        // Missing oids and non-commits (including annotated tags, which are
        // NOT peeled) error immediately.
        for bad in [missing, blob, tree, tag] {
            assert!(RevWalk::new(objects).push(bad).is_err());
            let w = RangeWalk::new(objects, |_| anyhow::bail!("unused"));
            assert!(w.into_topo_vec(bad, a).is_err());
            let w = RangeWalk::new(objects, |_| anyhow::bail!("unused"));
            assert!(w.into_topo_vec(a, bad).is_err());
        }
    }

    #[test]
    fn missing_ancestors_error_only_when_visited() {
        let t = TestRepo::new();
        let missing =
            gix_hash::ObjectId::from_str("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();
        let dangling = t.raw_commit("dangling", &[missing]);

        // A pushed commit with a missing parent errors during the walk.
        let objects = &t.repo.objects;
        let mut w = RevWalk::new(objects);
        w.push(dangling).unwrap();
        assert!(w.into_topo_vec(|_| false).is_err());

        // A range base with missing ancestors errors when the ranked walk
        // must rank it (the real lookup walks the ancestry)...
        let tip = t.commit("tip", &[]);
        let memo = std::cell::RefCell::new(HashMap::new());
        let w = RangeWalk::new(objects, |oid| exact_seq(&t.repo, &memo, oid));
        assert!(w.into_topo_vec(tip, dangling).is_err());

        // ...but not when the fast-forward path answers without touching the
        // base's ancestry: the walk stays lazy.
        let base = t.commit("base", &[dangling]);
        let top = t.commit("top", &[base]);
        let w = RangeWalk::new(objects, |_| anyhow::bail!("must not be consulted"));
        assert_eq!(w.into_topo_vec(top, base).unwrap(), vec![top]);
    }

    #[test]
    fn malformed_tails_are_tolerated() {
        // Parsing stops at the parent headers: truncated or author-less
        // commits walk fine (fsck-invalid objects; the old git2-backed walk
        // was partially lenient here too, in different places).
        let t = TestRepo::new();
        let trunc = gix_object::Write::write_buf(
            &t.repo.objects,
            gix_object::Kind::Commit,
            format!("tree {}\n", t.tree).as_bytes(),
        )
        .unwrap();
        let tip = t.raw_commit("tip", &[trunc]);
        let mut w = RevWalk::new(&t.repo.objects);
        w.push(tip).unwrap();
        assert_eq!(w.into_topo_vec(|_| false).unwrap(), vec![tip, trunc]);
    }

    #[test]
    fn discover_streams_dfs_preorder() {
        let t = TestRepo::new();
        let a = t.commit("a", &[]);
        let b = t.commit("b", &[a]);
        let c = t.commit("c", &[a]);
        let m = t.commit("m", &[b, c]);
        let objects = &t.repo.objects;

        // Pre-order: each commit before its parents, first parent's subgraph
        // before the second's.
        let mut w = RevWalk::new(objects);
        w.push(m).unwrap();
        let mut visited = Vec::new();
        w.discover(|oid| {
            visited.push(oid);
            Ok(ControlFlow::Continue(()))
        })
        .unwrap();
        assert_eq!(visited, vec![m, b, a, c]);

        // Break aborts the walk.
        let mut w = RevWalk::new(objects);
        w.push(m).unwrap();
        let mut visited = Vec::new();
        w.discover(|oid| {
            visited.push(oid);
            Ok(if oid == a {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            })
        })
        .unwrap();
        assert_eq!(visited, vec![m, b, a]);

        // first_parent confines discovery to the first-parent chain.
        let mut w = RevWalk::new(objects);
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
        let mut w = RevWalk::new(objects);
        w.push(m).unwrap();
        assert!(w.discover(|_| anyhow::bail!("boom")).is_err());

        // No pushes: no visits, clean return.
        let w = RevWalk::new(objects);
        w.discover(|_| panic!("must not be visited")).unwrap();
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
    fn fuzz_random_dags() {
        for seed in 0..48u64 {
            let mut rng = Rng(seed);
            let t = TestRepo::new();
            let n = 20 + rng.below(30);
            let mut commits: Vec<gix_hash::ObjectId> = Vec::new();
            for i in 0..n {
                let parents: Vec<gix_hash::ObjectId> = if commits.is_empty() || rng.below(12) == 0 {
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
                commits.push(t.commit(&format!("c{i}"), &parents));
            }
            let mut tips = Vec::new();
            for _ in 0..1 + rng.below(3) {
                tips.push(commits[rng.below(commits.len())]);
            }
            let pruned: HashSet<gix_hash::ObjectId> = commits
                .iter()
                .copied()
                .filter(|_| rng.below(6) == 0)
                .collect();
            assert_rev_walk(&t.repo, &tips, &no_prune(), &format!("fuzz {seed}"));
            assert_rev_walk(&t.repo, &tips, &pruned, &format!("fuzz {seed} pruned"));

            let tip = commits[rng.below(commits.len())];
            let base = commits[rng.below(commits.len())];
            assert_range_walk(&t.repo, tip, base, &format!("fuzz {seed} range"));
        }
    }
}
