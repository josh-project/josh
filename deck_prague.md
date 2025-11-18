Beyond Subtrees: An Algebra for Git
===================================

### 2025/11/28 Prague Rust Meetup 
*Christian Schilling*


<!-- end_slide -->

# **The two camps**

<!-- jump_to_middle -->
<!-- alignment: center -->

*Polyrepo*

**vs**

*Monorepo*

<!-- end_slide -->

# **No good options**

### **Polyrepos are bad:**
- **Fixed** partioning of codebase
- Repo sprawl
- Delayed updates
- Impossible to validate cross-repo changes  
- Dependency management
- Hard to enforce "One version rule" 🦩
- Who uses this?
- Lifetime of references
- Rename a repo?
- leftpad

<!-- pause -->
- Requires heavy tooling investment

<!-- pause -->

### **Monorepos are bad:**
- Huge clones  
- Slow builds / slow CI  
- Risk of tight coupling
- Team boundaries unclear  
- No visibility control (ACL)
- Breaks distributed model ‼️

<!-- pause -->
- Requires heavy tooling investment


<!-- end_slide -->

# **There is a *gap* Between Both Models**

We want:
- Monorepo **consistency** & **sustainability**
- Polyrepo **isolation** & **distribution** 

<!-- pause -->
> We need a bridge!

<!-- pause -->
Attempts:
- git submodule
- git subtree
- Copybara
- Sparse checkout
- Snapshot vendoring
- Package managers
- ...

<!-- pause -->
> **Thesis: All of those solve only **parts** of the problem**

<!-- end_slide -->

# **Whishlist vs Reality**

> 📚 **History preservation**
> 🫵 **git blame should work**
> 📤 **Upstream workflow**
> 🔒 **SHA stability / round-trip**
> 🌍 **Partial sharing / distributed repos**
> ⚡️ **Performance**
> 🧩 **Transforms** (excludes, overlays, ...)
> 🎯 **Focused CI scoping**
> 🧬 **Evolution over time**


> ❌ -> no | 😫 -> painful | ✅ -> yes

<!-- alignment: center -->

|                      | 📚 | 🫵 | 📤 | 🔒 | 🌍 | ⚡️ | 🧩 | 🎯 | 🧬
|----------------------|----|----|----|----|----|----|----|----|----
| Sparse checkout      | ✅ | ✅ | ✅ | ✅ | ❌ | ✅ | ❌ | ❌ | ✅
| git submodule        | ✅ | 😫 | 😫 | ✅ | 😫 | ✅ | ❌ | 😫 | 💩
| git subtree          | ✅ | 💔 | 😫 | ✅ | 😫 | 💀 | ❌ | 😫 | 😫
| Snapshot vendoring   | ❌ | ❌ | ✅ | ❌ | 😫 | ✅ | 😫 | 😫 | 😫
| Copybara             | ❌ | ❌ | 😫 | ❌ | 😫 | 🐌 | ✅ | 😫 | 😫
| Package managers     | ❌ | ❌ | 😫 | ❌ | 😫 | ✅ | 😫 | 😫 | 😫



<!-- pause -->

Josh (Projections) | ✅ | ✅ | ✅ | ✅ | ✨ | ✅ | ✅ | ✅ | ✨


<!-- end_slide -->

<!-- jump_to_middle -->
<!-- alignment: center -->
**Why?**

<!-- pause -->
**How?**

<!-- end_slide -->

# **Why These Tools Fail**

They operate in terms of:
- Files  
- Directories  
- Scripts  
- Imperative **procedures**

<!-- pause -->
But Josh operates operates in terms of:
- Graphs
- Functional **relationships**


<!-- end_slide -->
# **Example usage**

```
git clone https://github.com/josh-project/josh.git josh
```
<!-- pause -->

Same via proxy:
```
git clone https://josh-project.dev/josh.git josh
```
<!-- pause -->

Subtree via proxy:
```
git clone https://josh-project.dev/josh.git:/docs.git josh-docs
```
<!-- pause -->

Subtree via cli, without proxy:
```
josh clone https://github.com/josh-project/josh.git :/docs josh-docs
```

<!-- end_slide -->

<!-- jump_to_middle -->
<!-- alignment: center -->
```
:/docs
```
What is that? 🤔

<!-- end_slide -->

# **An algebra for Git**

Trees and commits are immutable objects.

Filters:
```
Tree → Tree
Commit → Commit
```

<!-- pause -->

Properties:
- deterministic
- composable
- partially invertible

<!-- end_slide -->

# **The Primitive Operations**

1. Subtree
2. Chain
3. Inversion
4. Prefix
5. Compose
6. Exclude

Symbols:
 * p = Path
 * t = Tree
 * A,B,.. = Filter

<!-- end_slide -->

# **Subtree**

```
:/p
```

Keeps `p/**`, drops everything else, strips prefix.

<!-- end_slide -->

# **Chain**

```
:A:B:C
```

Applies filter to the result of previous filter

<!-- pause -->


 Associative:
```
:[:A:B]:C == :A:[:B:C]
```

<!-- pause -->
```
:/a/b/c == :/a:/b:/c
```

<!-- end_slide -->

# **Inversion**

```
:F:invert[:F]:F == F
```

<!-- pause -->

Inverse of chain
```
:invert[:A:B] == :invert[:B]:invert[:A]
```

<!-- pause -->


Not **all** filters *have* inverses!
...that's ok.

<!-- end_slide -->

# **Prefix**

```
:prefix=p
```

Adds one level of hierarchy to the tree

<!-- pause -->

inverse of `:prefix=p` -> `:/p` (subdir)
```
:prefix=p:/p == :/
:prefix=p:/p:prefix=p == :prefix=p
:/:prefix=p == :prefix=p
```

<!-- pause -->

inverse of `:/p` -> `:prefix=p`
```
:/p:prefix=p:/p == :/p
:/p:/ == :/p
```

<!-- pause -->

Shorthand notation: `::p/` == `:/p:prefix=p` (selection)

inverse of `::p/` -> `::p/`

<!-- end_slide -->

# **Compose**

```
:[:A, :B, :C, ...]
```

Like "union" of trees

<!-- pause -->

Associative
```
:[:A, :B, :C] == :[:[:A, :B], :C] == :[:A, :[:B, :C]]
```

<!-- pause -->

Distributive
```
:[:X:A, :X:B] == :X:[:A, :B]
:[:A:X, :B:X] == :[:A, :B]:X
```

<!-- end_slide -->

Not commutative
```
:[:A, :B] != :[:B, :A]
```
<!-- pause -->

(except when `:invert[:[:A, :B]] == :[:A, :B]`)

<!-- pause -->

Inverse
```
:invert[:[:A, :B]] == :[:invert[:A], :invert[:B]]
```


<!-- end_slide -->

# **Exclude**

```
:exclude[:F]
```

Keep all files except those in :F

<!-- pause -->

Inverse:
```
:invert[:exclude[:F]] == :exclude[:F]
```

<!-- end_slide -->

# Push via filter

Given HEAD in the full main with tree t⁰, the projected HEAD has the tree:
```
t₀ = :F(t⁰)
```
<!-- pause -->

With modifications we get (the updated projection HEAD)
```
t₀ -> t₁
```

<!-- end_slide -->

How do we get t¹?
<!-- pause -->
```
:t¹ == invert[:F](t₁)
```
<!-- pause -->
... no, missing files
<!-- pause -->
```
:t¹ == :[     
    :invert[:F](t₁)
    :/(t⁰)
]
```
<!-- pause -->
... no, does not handle deletions
<!-- pause -->

```
:t¹ == :[ 
    :invert[:F](t₁)
    :exclude[:F:invert[:F]](t⁰)
]
```
<!-- pause -->

(it does get a bit more complicated with merges, but this is the basic idea)

<!-- end_slide -->

<!-- jump_to_middle -->
<!-- alignment: center -->
Filters are configuration *about* the __relationships__ between repos...

<!-- pause -->
Where to put them?

<!-- end_slide -->

# **Stored Filters - Versioned Architecture**

Stored Filters:
- Live inside the repo as `*.josh` files
- Versioned 
- Evolve with code  
- Define per-commit projections  
- Remove external configuration

<!-- pause -->

<!-- incremental_lists: true -->

Common use case:
- Each target gets it's own repo, including it's dependencies
- Perfect sandbox: Only declared parts of the repo are available
- SHA changes only if real dependencies change
- Exclude docs from the SHA
- Include *only* docs in the SHA
- ...

Save filter as `path/to/filter.josh` and then:

```
:+path/to/filter
```

<!-- pause -->
Yes, stored filters can reference other stored filters

<!-- end_slide -->

# **GraphQL API — Query Projections Without Cloning**

Query:
- Projected trees  
- Commit graphs  
- History under projections  
- Hashes ✨

<!-- pause -->

-> CI can make accurate decisions on what to build *before* cloning any repo ⚡️

<!-- end_slide -->

# **Future Directions**

<!-- incremental_lists: true -->

- Projection aware merge bot
- Projection aware (stacked!) code review
- Scriptable filters
- Path-level ACLs  
- Support (much!) larger repos
- Use git+josh as database for issues, wikis, ...

<!-- end_slide -->

<!-- incremental_lists: true -->

# **Summary**
1. Monorepo & Polyrepo are both painful
2. The gap can be bridged
3. Relationships over procedures
4. Once projections are "given" a lot of possibilities open up

<!-- end_slide -->

# **Q&A**
<!-- jump_to_middle -->
<!-- alignment: center -->
Questions?

<!-- end_slide -->

```bash +exec
FILTER="::josh-cli/"
git archive $(josh-filter $FILTER) | tar -t | tree --fromfile
```
<!-- end_slide -->

```bash +exec
FILTER=":/josh-cli"
git archive $(josh-filter $FILTER) | tar -t | tree --fromfile
```
<!-- end_slide -->

```bash +exec
FILTER="::*.toml"
git archive $(josh-filter $FILTER) | tar -t | tree --fromfile
```
<!-- end_slide -->

```bash +exec
FILTER=":[::josh-core/,::josh-cli/]:exclude[::**/*.rs]"
git archive $(josh-filter $FILTER) | tar -t | tree --fromfile
```
<!-- end_slide -->

