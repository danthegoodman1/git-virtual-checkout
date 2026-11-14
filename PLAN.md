# Development Plan

## Overarching Goal

Build `gvc` if it gives online agent workloads a clear win, and stop if it doesn't. The workload is many short-lived workspaces on large GitHub repositories, always online: each workspace reads a fraction of the files, searches, runs git, asks questions of history, builds, commits and pushes, often beside other workspaces on the same repository.

`gvc` clones a GitHub repository in seconds at any size and lets stock git work inside it. The working tree is a filesystem mount with two layers: the commit's tree, fetched file by file on first read, under a local layer that holds every edit. git keeps its own repository, index and protocol. gvc writes the index git trusts, answers git's fsmonitor hook, keeps the layers in step as git moves between commits, shows Git LFS content in place of pointers, and serves history on demand. Questions of history that git would answer from local objects, gvc's own commands answer from GitHub's APIs. [docs/feasibility.md](docs/feasibility.md) is the design and the evidence behind it.

Gate G1, after Phase 3, decides whether to go on: replayed agent sessions must beat the best stock setup clearly, or the build stops there.

The core plan ends with comprehensive CI testing, release builds, and performance work on Linux. macOS through FSKit (track M1) and a second git host (track B1) follow it. Non-goals until a phase names them: offline use, Windows, modifying or wrapping the git binary, server-side commits, sparse checkout, and hosts other than GitHub.

## Implementation Principles

- Stock git owns repository semantics. Users run unmodified git; gvc never forks it, wraps it, or shadows its commands.
- The git protocol supplies object bytes, and every object passes a hash check before git or a reader sees it. Host APIs accelerate: sizes, planning, blame. An object rebuilt from an API is used only when its hash matches.
- All filesystem behavior lives in platform-neutral Rust. `gvc-vfs` defines a `Filesystem` trait and its types (node ids, attributes, errno, directory entries, open flags, caller context, invalidations). The working tree (`gvc-overlay`) and the lazy object store (`gvc-odb`) implement it. `gvc-fuse` adapts it to FUSE and holds no logic, so an FSKit adapter can be a Swift shim over the same crates. None of `gvc-vfs`, `gvc-overlay` and `gvc-odb` depends on an OS filesystem crate, and CI checks this.
- The `Filesystem` boundary uses types a C ABI can carry: integer node ids, byte-string names, plain attribute structs, and completion-style replies. Node lifetimes follow explicit lookup counts and forgets, which map onto both FUSE `forget` and FSKit item reclamation.
- A request waiting on the network never blocks another request. Replies complete asynchronously.
- Every fetch is batched. A coalescer turns concurrent misses into one request, and no code path fetches objects one at a time in a loop.
- A write the filesystem acknowledges survives a daemon crash, with POSIX `fsync` semantics for data.
- Tests count fetches. Every test that runs git asserts the requests and bytes the test server saw.
- PR CI is hermetic and finishes in 10 minutes. Network tests against GitHub run nightly.
- Agent workloads decide. Each phase's benchmarks include the agent replay suite (0H), and a change that slows a replayed session doesn't ship.
- Build the smallest implementation that meets the phase gate. Add abstraction when a later phase needs it.
- A change that alters the design updates `docs/feasibility.md` in the same commit.
- Each phase ends with `/code-review high`. Findings are fixed or recorded in the phase ledger before the phase closes.
- Execution order: Phase 0, Phase 1, A1, Phase 2, Phase 3, G1, then Phase 4 through Phase 9, M1 and B1.

## Testing Strategy

- **Unit and property tests** (`proptest`) for pure logic: pkt-line, pack ingest, tree index, index writer, pointer parsing, overlay path table, fsmonitor tokens.
- **VFS conformance:** one model-based suite drives each `Filesystem` implementation directly, with no kernel, and replays the same scenarios through a real mount with syscalls. The FSKit adapter runs the syscall half on macOS in M1.
- **POSIX differential:** seeded random sequences of file operations run on a mount and on a tmpfs directory; results, errors and final trees must match.
- **Git differential:** scripted and seeded random git command sequences run in a stock clone and in a gvc mount of the same fixture; normalized output, refs, index entries, working-tree hashes, `git fsck`, and what the server received must match.
- **Fetch accounting:** `gvc-testkit`'s server counts requests, objects and bytes; tests assert bounds, such as zero blob fetches for untouched files.
- **Fault injection:** latency, drops, timeouts, 5xx, 429 and exhausted rate limits, truncated packs, corrupt objects, a full disk, and `kill -9` of the daemon at random points.
- **Fuzzing** (`cargo-fuzz`) for every parser of remote input: pkt-line, packs, tree JSON, LFS pointers and batch responses, the filter protocol.
- **Git contract tests:** strace-based checks of the git behaviors gvc relies on, across a git version matrix (2.40, Ubuntu's, latest).
- **Real GitHub smoke tests:** nightly runs against danthegoodman1/lionturtle, facebook/react and torvalds/linux.
- **Planted bugs:** `scripts/mutants` applies known bugs to a scratch copy and reports which layer catches each. Every phase adds its own, and every one must be caught.
- **Agent replay benchmark:** scripted sessions shaped like real agent traces, run in 1 and 8 workspaces against the best stock setup (0H). It gates G1 and every later phase.
- **Benchmarks:** on tmpfs, so disk speed drops out. Local-server runs with injected latency are reproducible; GitHub runs feed the README.
- **CI budget:** PR CI runs lint, unit and property tests, FUSE integration, and a fixed differential seed budget in parallel jobs, within 10 minutes. Nightly CI runs 10× the seeds, the git matrix, fuzzing, mutants, GitHub smoke tests and benchmarks, within 60 minutes.

## Phase 0: Scaffold and baselines

Goal:
A public repository with the design, this plan, baseline numbers, a Cargo workspace, CI, and the test server every later phase builds on.

Scope:
- 0A Public GitHub repository with the feasibility study, README and plan.
- 0B Baseline clone benchmarks for lionturtle, react and Linux, with the scripts that produce them.
- 0C Cargo workspace (edition 2024, toolchain pinned in `rust-toolchain.toml`, `license = "Apache-2.0"`), with the crates later phases fill: `vfs`, `fuse`, `overlay`, `git`, `backend`, `store`, `daemon`, `cli`, `testkit`.
- 0D CI: rustfmt, clippy with `-D warnings`, `cargo nextest`, and the dependency check that keeps `vfs`, `overlay` and `odb` free of OS filesystem crates.
- 0E `gvc-testkit` server: smart HTTP through `git http-backend` with protocol v2, `uploadpack.allowFilter` and `uploadpack.allowAnySHA1InWant`, plus request, object and byte counters.
- 0F Fixture generator: seeded repositories with deep trees, long histories, merges, renames, symlinks, executable files, a submodule, LFS-attributed files, `.gitattributes` conversions and non-ASCII names.
- 0G `scripts/mutants`, adapted from inodedb.
- 0H Agent replay benchmark. Each session reads line ranges of files, greps named directories and once the whole tree, runs `git status`, `diff`, `log -n`, `show` and `show <rev>:<path>`, edits, builds and tests, commits and pushes. It runs in 1 and 8 workspaces on Linux, react and lionturtle, against two stock setups: `git clone --depth=1` per workspace, and one blobless clone with a `git worktree` per workspace. It reports time to a ready workspace, session wall time, bytes downloaded and disk used.

Out of scope:
- Any filesystem or protocol code (Phase 1 and later).

Completion gate:
CI passes on `main`, and stock git clones a fixture from the testkit server with `--filter=blob:none` while the counters record the fetch.

Testing plan:
- A testkit test that clones a generated fixture through the server and checks the counters.
- A mutants run that catches one trivial planted bug.

Status ledger:

| Status | Type | Item | Evidence / Gap |
| --- | --- | --- | --- |
| Complete | Work | 0A: Public repository | github.com/danthegoodman1/git-virtual-checkout, with `README.md`, `PLAN.md`, `BENCHMARKS.md`, `docs/feasibility.md`. |
| Complete | Work | 0B: Baseline clone benchmarks | `scripts/bench/clones`, `scripts/bench/tree-sizes`, `docs/bench/clones.jsonl`, `BENCHMARKS.md`; charts from `scripts/bench/charts`. |
| Incomplete | Work | 0C: Cargo workspace | Missing: `Cargo.toml`, `rust-toolchain.toml`, crate skeletons. |
| Incomplete | Work | 0D: CI | Missing: `.github/workflows/ci.yml` and a passing run. |
| Incomplete | Work | 0E: Testkit server and counters | Missing: `crates/testkit` server and its clone test. |
| Incomplete | Work | 0F: Fixture generator | Missing: seeded generator and a test that two runs of one seed produce identical repositories. |
| Incomplete | Work | 0G: Planted-bug tooling | Missing: `scripts/mutants` and a caught trivial bug. |
| Incomplete | Work | 0H: Agent replay benchmark with stock baselines | Missing: session scripts, harness, and baseline results for both stock setups. |
| Incomplete | Decision | Crate `git-virtual-checkout`, binary `gvc` | crates.io's `gvc` belongs to another project. Missing: workspace manifest naming. |
| Incomplete | Gate | CI passes on `main` with the testkit clone test | Missing: CI run id. |

## Phase 1: Git protocol and GitHub backend

Goal:
gvc resolves refs, fetches exactly the objects it asks for in batches, verifies every one, and gets each blob's size from GitHub.

Scope:
- 1A pkt-line and a protocol v2 client over HTTPS: capability advertisement, `ls-refs`, and `fetch` with `want`, `have`, `done`, `filter`, `deepen` and `shallow`, sideband progress and errors, on pooled HTTP/2 connections.
- 1B Pack ingest: stream the pack into the store, decode it with `gix-pack`, and verify each object's hash.
- 1C The `Backend` trait from the design (`ResolveRef`, `FetchObjects`, `ListTree`, with optional `BlobSizer`, `Blamer`, `PathLog`, `Snapshot`), and its git-protocol implementation.
- 1D GitHub backend: tree-API sizes with the truncation walk from `scripts/bench/tree-sizes`, single blobs from raw.githubusercontent.com, auth from `GITHUB_TOKEN`, `gh auth token` or `git credential fill`, and rate-limit handling that honors `x-ratelimit-*` and `retry-after`.
- 1E Shared object store: content-addressed, written by temp file and rename, shared across mounts, bounded by size with LRU eviction.
- 1F Fetch coalescer: concurrent misses within a short window become one request; duplicate requests share one fetch.

Out of scope:
- Any mount (Phase 2), LFS (Phase 5), blame and history APIs (Phase 6).

Completion gate:
Against the testkit server and against GitHub, gvc resolves HEAD, fetches every tree at HEAD, gets sizes for all 97,371 Linux paths, and fetches 1,000 named blobs in at most two requests, with every object hash verified.

Testing plan:
- Protocol tests against `git http-backend`, including `deepen` from a non-tip commit and `filter=blob:none`.
- Mock-API tests for tree truncation, secondary rate limits and token refresh.
- Fuzz targets for pkt-line, pack ingest and tree JSON.
- Planted bugs: a skipped hash check, a dropped `filter` line.

Status ledger:

| Status | Type | Item | Evidence / Gap |
| --- | --- | --- | --- |
| Incomplete | Work | 1A: Protocol v2 client | Missing: client and tests against `git http-backend`. |
| Incomplete | Work | 1B: Pack ingest with hash checks | Missing: ingest and a corrupt-object test. |
| Incomplete | Work | 1C: `Backend` trait and git-protocol backend | Missing: trait and implementation. |
| Incomplete | Work | 1D: GitHub backend | Missing: sizes, raw blobs, auth, rate limits, with mock tests. |
| Incomplete | Work | 1E: Shared object store | Missing: store, eviction, and a concurrent-writer test. |
| Incomplete | Work | 1F: Fetch coalescer | Missing: coalescer and a fetch-count test. |
| Incomplete | Decision | HTTP stack | Default: `reqwest` with rustls and HTTP/2. Missing: confirmation in 1A. |
| Incomplete | Gate | Linux sizes and 1,000 blobs in two requests, all verified | Missing: test output against the testkit server and GitHub. |
| Incomplete | Test | Fuzz targets run clean for 10 minutes each | Missing: `fuzz/` targets and a run log. |
| Incomplete | Test | Planted bugs caught | Missing: mutants report. |

## A1: History questions through GitHub

Goal:
Agents get answers to history questions from GitHub in seconds, with no local history, in any checkout of a GitHub repository, gvc's or not.

Scope:
- A1A `gvc log [<rev>] [-n <n>] [-- <path>]`: commits from GraphQL `history`, in git's `--oneline` and a `--format` subset, and as `--json`.
- A1B `gvc blame [-L <range>] <path> [<rev>]`: GraphQL `blame`, in `git blame`'s default and `--porcelain` formats, and as `--json`.
- A1C `gvc show <rev>[:<path>]` and `gvc diff <a> <b> [-- <path>]`: commits and patches from the REST commit and compare endpoints; file contents from raw.githubusercontent.com.
- A1D `gvc merge-base <a> <b>`: the REST compare endpoint.
- A1E Local commits on top: with unpushed commits, `gvc blame` runs `git blame <base>..HEAD` locally and fills the lines older than the base from GitHub's blame of the base; `gvc log` lists the local commits first. Uncommitted edits count as `git blame` counts them.
- A1F Agent integration: stable `--json` schemas, and a short instructions file agents load that says when to use each command in place of git's.
- A1G A request budget per command, a response cache keyed by commit id (answers about a commit never change), and an error that names the exhausted quota and its reset time.

Out of scope:
- Pickaxe searches (`-S`, `-G`) and code search, which GitHub's APIs can't answer exactly.

Completion gate:
On git/git, react and Linux, `gvc blame` matches `git blame --porcelain`'s line attributions on 50 sampled files, `gvc log -- <path>` matches its documented `git log` equivalent on the same files, and each command answers within 5 s at p50 for files under 5,000 lines.

Testing plan:
- Recorded GitHub responses in PR CI; live comparisons against full clones nightly.
- A test that pins which `git log` options GitHub's path history matches, for history simplification and renames.
- Composition tests with local commits and uncommitted edits.
- Planted bugs: an off-by-one in `-L` ranges, a dropped local commit.

Status ledger:

| Status | Type | Item | Evidence / Gap |
| --- | --- | --- | --- |
| Incomplete | Work | A1A: `gvc log` | Missing: command and comparison tests. |
| Incomplete | Work | A1B: `gvc blame` | Missing: command and attribution tests. |
| Incomplete | Work | A1C: `gvc show` and `gvc diff` | Missing: commands and patch-equality tests. |
| Incomplete | Work | A1D: `gvc merge-base` | Missing: command and tests. |
| Incomplete | Work | A1E: Local commits on top | Missing: composition and its tests. |
| Incomplete | Doc | A1F: Agent instructions and JSON schemas | Missing: instructions file and schema tests. |
| Incomplete | Work | A1G: Request budget, cache, quota errors | Missing: implementation and a quota-exhaustion test. |
| Complete | Decision | GitHub's blame is trustworthy as an answer | `docs/feasibility.md`: GraphQL blame matched local `git blame` on all 4,146 lines of git/git's Makefile in 3.7 s. |
| Incomplete | Gate | Attributions and logs match on 50 files per repository; p50 within 5 s | Missing: nightly comparison run. |
| Incomplete | Test | Planted bugs caught | Missing: mutants report. |

## Phase 2: Read-only virtual checkout

Goal:
`gvc clone` mounts any GitHub repository within seconds, every file reads correctly, and the filesystem logic sits behind the platform-neutral `Filesystem` trait.

Scope:
- 2A `gvc-vfs`: the `Filesystem` trait and its types; async completion; node lookup counts and forgets; invalidation events; an optional caller context (pid on Linux); errno mapping.
- 2B `gvc-fuse`: serves any `Filesystem` through `fuser` 0.18 with multi-threaded dispatch and async replies, `default_permissions`, entry and attr timeouts, `keep_cache`, and clean unmount.
- 2C `gvc-overlay` lower layer: a compact in-memory tree index (interned names, sorted children, mode, oid, size), stable inode numbers, a fixed timestamp from the commit, symlinks prefetched in one batch at clone, executable bits, submodules as empty directories.
- 2D `gvc clone`: `ls-refs`, then `git clone --filter=blob:none --depth=1 --no-checkout` for `.git` beside the size walk, with the mount up at once. A lookup waits only on its own directory: a directory not yet listed comes from one non-recursive tree-API call while the full listing loads. git commands work once the full listing and the index land.
- 2E Read path: first read fetches through the coalescer into the store; a miss also queues its directory's unread blobs, up to a size bound, so a search that reads files one at a time still fetches in batches; failures surface as `EIO` with a logged cause; reads offline fail fast.
- 2G Whole-tree scans: when reads sweep across directories, gvc streams the commit's tarball into the store, checking each file against its tree oid, so a whole-tree grep costs about one shallow clone, once per commit for every workspace that shares the store.
- 2F `gvc mount`, `gvc unmount`, `gvc status`, and the daemon's lifecycle: one process per mount, a pid file, a control socket, all mounts sharing the store.

Out of scope:
- Writes (Phase 3); moving between commits (Phase 4); LFS content (Phase 5).

Completion gate:
`gvc clone torvalds/linux` lists its root within 2 s of start and writes its index within 10 s; `find . -type f | wc -l` matches `git ls-tree -r`; reading every file of facebook/react through the mount reproduces every tree oid.

Testing plan:
- VFS conformance suite, model-based, against the lower layer, and the same scenarios through the FUSE mount.
- A latching test: lookups during an injected 2 s listing delay all complete, and an unrelated mount stays responsive.
- A read-everything test that bounds fetch requests to files / 100.
- The dependency check from 0D passes with `gvc-overlay` in the workspace.
- Planted bugs: a wrong size, a lost symlink target, a blocking fetch on the dispatch thread.

Status ledger:

| Status | Type | Item | Evidence / Gap |
| --- | --- | --- | --- |
| Incomplete | Work | 2A: `gvc-vfs` trait and types | Missing: crate, with a doc test of the boundary types' C layout. |
| Incomplete | Work | 2B: `gvc-fuse` adapter | Missing: adapter and mount tests. |
| Incomplete | Work | 2C: Lower layer | Missing: tree index, attributes, special entries. |
| Incomplete | Work | 2D: `gvc clone` bootstrap with latching | Missing: command and latching test. |
| Incomplete | Work | 2E: Read path and errors | Missing: read tests, including offline and failure injection. |
| Incomplete | Work | 2F: Mount lifecycle commands | Missing: commands and lifecycle tests. |
| Incomplete | Work | 2G: Scan detection and streamed hydration | Missing: detector, tarball path, and a whole-tree grep timed against `git clone --depth=1`. |
| Incomplete | Decision | One daemon per mount, sharing the on-disk store | Missing: confirmation once 2F runs two mounts. |
| Incomplete | Gate | Linux root listed within 2 s and index written within 10 s; react's oids reproduced | Missing: timed run and verification log. |
| Incomplete | Test | VFS conformance, direct and through FUSE | Missing: suite and CI job. |
| Incomplete | Test | Planted bugs caught | Missing: mutants report. |

## Phase 3: Writable checkout, status, commit and push

Goal:
Edits land in the local layer, survive crashes, and stock `git status`, `diff`, `add`, `commit` and `push` work without fetching any file the user left alone.

Scope:
- 3A Upper layer: copy-up on a write open (no fetch with `O_TRUNC`), create, mkdir, unlink, rmdir, rename with `RENAME_NOREPLACE` and `RENAME_EXCHANGE`, symlink, chmod, truncate, utimens, fsync; whiteouts; `ENOTSUP` for xattrs.
- 3B Persistence and recovery: an overlay journal with ordered fsyncs, upper files outside the mount, and a remount that restores the exact tree.
- 3C Index writer: every entry from the tree index, stat data equal to what the filesystem reports, the cache-tree extension, index v4; config `core.checkStat=minimal`, `core.trustctime=false`, `core.untrackedCache=true`, `core.fsmonitor`, `gc.auto=0`, `maintenance.auto=false`.
- 3D fsmonitor hook (protocol v2): `gvc hook fsmonitor` asks the daemon over its socket; tokens come from the write log; an unknown token answers with a full rescan.
- 3E Prefetch every `.gitignore` and `.gitattributes` blob at clone, since git reads them on every status.
- 3F Zero-fetch accounting for untouched files across status, diff, add, commit and push.
- 3G A recent history window: in the background after clone, `git fetch --filter=tree:0 --deepen=20`, so `git log -n` runs locally. On Linux that is 546 commits and 512 KB in 1.9 s; `--deepen=999` pulled all 1.47 million commits (804 MB), since depth counts generations through every merge.
- 3H Build outputs at native speed: writes under ignored build directories (`target/`, `node_modules/`, `build/`, and any listed in config) go through FUSE passthrough to native files.

Out of scope:
- Re-basing after checkout (Phase 4); LFS writes (Phase 5).

Completion gate:
On Linux and react mounts, the git differential scenarios for status, diff, add, commit and push pass with zero blob fetches for untouched files, and a daemon killed mid-write remounts with every acknowledged write intact.

Testing plan:
- POSIX differential against tmpfs, seeded.
- Git differential for status, diff, add, commit, amend, push, untracked and ignored files.
- Index tests: stock git reads the generated index with `git ls-files --debug` and opens no file during `git status` (strace).
- fsmonitor token tests across daemon restarts.
- Crash tests: `kill -9` at random points under a write workload, then remount and compare with the model.
- Planted bugs: a stat mismatch, a lost whiteout, an fsync acknowledged before the journal.

Status ledger:

| Status | Type | Item | Evidence / Gap |
| --- | --- | --- | --- |
| Incomplete | Work | 3A: Upper layer operations | Missing: implementation and POSIX differential. |
| Incomplete | Work | 3B: Journal and recovery | Missing: journal, remount, crash tests. |
| Incomplete | Work | 3C: Index writer and repo config | Missing: writer and stock-git read tests. |
| Incomplete | Work | 3D: fsmonitor hook | Missing: hook, socket protocol, token tests. |
| Incomplete | Work | 3E: Ignore and attribute prefetch | Missing: prefetch and fetch-count test. |
| Incomplete | Work | 3F: Zero-fetch accounting | Missing: counter assertions in every git differential scenario. |
| Incomplete | Work | 3G: Recent history window | Missing: background deepen and a `git log -n 50` test with zero fetches. |
| Incomplete | Work | 3H: Build outputs at native speed | Missing: passthrough for ignored build directories and a `cargo build` timed against a native checkout. |
| Incomplete | Decision | Overlay metadata store | Options: lionturtle, redb, or a journal of our own. Missing: choice and its crash-test results. |
| Incomplete | Decision | Hard links in the working tree | Default: `EPERM`. Missing: a survey of tools that create them in checkouts. |
| Incomplete | Gate | Differential scenarios pass with zero untouched fetches; crash recovery intact | Missing: CI run and crash-test log. |
| Incomplete | Test | Planted bugs caught | Missing: mutants report. |

## G1: Agent go/no-go

Goal:
Decide from measurements whether gvc gives online agent workloads a clear win, before building Phases 4 through 9.

Scope:
- G1A Run the agent replay benchmark (0H) on Phase 3's build: 1 and 8 workspaces on Linux, react and lionturtle, with 8 `gvc clone`s sharing one store against the two stock setups.
- G1B Compare each measure with the best stock setup for that scenario.
- G1C Record the result and the decision in this ledger; on a no-go, record what would change it.

Out of scope:
- Tuning to pass the gate beyond what Phases 2 and 3 scope; a miss goes back to design.

Completion gate:
A go needs every criterion; a miss on any is a no-go until a design change addresses it.
- On Linux and react, a ready workspace at least 5× sooner than the best stock setup.
- On Linux and react at 8 workspaces, at least 5× fewer bytes downloaded per added workspace.
- Every replayed session, including those with a whole-tree grep and a build, within 1.1× the best stock setup's wall time.
- On lionturtle, within 1.1× the best stock setup on every measure.

Testing plan:
- Three runs per configuration on tmpfs against GitHub, reporting medians, with the commands in `BENCHMARKS.md`.

Status ledger:

| Status | Type | Item | Evidence / Gap |
| --- | --- | --- | --- |
| Incomplete | Work | G1A: Agent replay on Phase 3's build | Missing: results in `BENCHMARKS.md`. |
| Incomplete | Work | G1B: Comparison with the best stock setup | Missing: comparison table. |
| Incomplete | Decision | G1C: Go or no-go | Missing: the decision and its evidence. |
| Incomplete | Gate | All four go criteria met | Missing: recorded runs. |

## Phase 4: Moving between commits, worktrees and history

Goal:
Every git command that moves HEAD works through the mount, the local layer shrinks back to the user's real edits after each move, and new worktrees and commit jumps take one mount or one index write.

Scope:
- 4A Re-base on `post-index-change`, `post-checkout`, `post-merge` and `post-rewrite`: point the lower layer at HEAD's tree, drop upper files that match it, and fix their index stat data under `index.lock`.
- 4B `gvc switch <rev>`: on a clean tree, move HEAD, swap the lower layer, and regenerate the index without touching blobs; on a dirty tree, refuse and name `git switch`.
- 4C `gvc worktree add`, `remove` and `list`: `git worktree add --no-checkout`, a new mount, and its index, sharing the store.
- 4D Background history hydration: `git fetch --unshallow --filter=blob:none`, with progress and `gvc hydrate --history`.
- 4E `gvc prefetch <rev> <path>...` for commands that fetch one blob per file, such as `git checkout <rev> -- <path>`.
- 4F `gvc hydrate`: the 2G tarball stream on request, with the mismatched files fetched by oid, or `git backfill` where the host prefers.
- 4G Hook chaining: run the user's hooks, `core.hooksPath` and git-lfs's hooks after gvc's.

Out of scope:
- History beyond the shallow window before hydration finishes (Phase 6).

Completion gate:
The git differential scenarios for switch, merge, rebase, cherry-pick, stash, reset `--hard`, restore, pull and worktree add pass; after switching between two commits and back, the upper layer holds only the user's edits; `gvc switch` between Linux `v6.0` and `master` finishes within 2 s.

Testing plan:
- Git differential for every command in the gate, including conflicts and aborted rebases.
- Upper-layer size assertions after each move.
- Hydration tests with `export-ignore` and `export-subst` fixtures.
- Hook-chaining tests with a pre-existing user hook and `core.hooksPath`.
- Planted bugs: a re-base that keeps a stale upper file, an index fix-up without the lock.

Status ledger:

| Status | Type | Item | Evidence / Gap |
| --- | --- | --- | --- |
| Incomplete | Work | 4A: Re-base hooks | Missing: hooks, daemon handling, lock discipline. |
| Incomplete | Work | 4B: `gvc switch` | Missing: command and timing on Linux. |
| Incomplete | Work | 4C: `gvc worktree` | Missing: commands and shared-store test. |
| Incomplete | Work | 4D: History hydration | Missing: background fetch and progress. |
| Incomplete | Work | 4E: `gvc prefetch` | Missing: command and fetch-count test. |
| Incomplete | Work | 4F: `gvc hydrate` | Missing: tarball path with hash checks, and its fallback. |
| Incomplete | Work | 4G: Hook chaining | Missing: chaining and tests. |
| Incomplete | Gate | Movement scenarios pass; upper layer returns to edits only; Linux switch within 2 s | Missing: CI run and timing log. |
| Incomplete | Test | Planted bugs caught | Missing: mutants report. |

## Phase 5: Git LFS

Goal:
LFS files read as their content wherever the tree holds a pointer, with no smudge step and no git-lfs install, and adding and pushing LFS files works.

Scope:
- 5A Detect LFS paths with `git check-attr --source=<rev> --stdin filter` and a pointer parse, at clone and on every re-base.
- 5B Present content: size from the pointer; download through the batch API with cached signed URLs, falling back to media.githubusercontent.com; a block cache filled by `Range` requests; a SHA-256 check once every block is present.
- 5C `gvc hook lfs-filter`: git's long-running filter protocol, cleaning content into the store and a pointer, and smudging a pointer to itself.
- 5D Index size fix-ups for LFS paths during the Phase 4 re-base.
- 5E Pre-push upload through the batch API's `upload` and `verify` actions.
- 5F `.lfsconfig` and `lfs.url`; git-lfs's store layout through `lfs.storage`; `lockable` files shown read-only.

Out of scope:
- LFS file locking through the locks API.

Completion gate:
The LFS differential scenarios match a stock git-lfs checkout: contents, `git status`, adding, committing and pushing a new LFS file, and the server holding its object; a 100-byte read of a large LFS file fetches one block.

Testing plan:
- A mock LFS server with `Range` support and counters.
- LFS differential against stock git-lfs, which CI installs for the reference side only.
- Fuzz targets for pointers, batch responses and the filter protocol.
- Planted bugs: a size taken from the pointer blob, a skipped SHA-256 check, a pointer pushed without its object.

Status ledger:

| Status | Type | Item | Evidence / Gap |
| --- | --- | --- | --- |
| Incomplete | Work | 5A: LFS path detection | Missing: detection and attribute-edge tests. |
| Incomplete | Work | 5B: Content presentation and block cache | Missing: implementation and ranged-read test. |
| Incomplete | Work | 5C: LFS filter process | Missing: filter and protocol tests. |
| Incomplete | Work | 5D: Index size fix-ups | Missing: fix-up and status test after checkout. |
| Incomplete | Work | 5E: Pre-push upload | Missing: hook and server-side check. |
| Incomplete | Work | 5F: Config, interop, lockable | Missing: tests with git-lfs reading gvc's store. |
| Incomplete | Gate | LFS differential matches; one block per small read | Missing: CI run and counter log. |
| Incomplete | Test | Planted bugs caught | Missing: mutants report. |

## Phase 6: Lazy object store and fast history

Goal:
A repository too large to hydrate shows its full history at once, for the stock git commands agents run anyway: commits, trees and blobs arrive as git reads them, and log, show, blame and pickaxe searches run fast with output identical to a full clone. Linux's commits alone are 804 MB, so it needs this phase.

Scope:
- 6A `gvc-odb`, a second `Filesystem`: a read-only loose-object view (`xx/<38 hex>`) served by the same adapter; resolves only ids a ref or a served object references, and answers every other id with `ENOENT` without a network call.
- 6B No-shallow mode: `objects/info/alternates` points at the store's mount, the shallow file goes, and each commit request reads ahead its next N ancestors in one fetch.
- 6C Command-aware prefetch: the caller's command line, from the caller context, picks a planner: commit ranges for `log`, every version of a path for `blame` (`git log --full-history -m --raw`), and the named paths for `checkout <rev> -- <path>`.
- 6D `core.abbrev=12`, and short-SHA resolution through the API in gvc commands.
- 6E Git contract tests: strace-based checks across the version matrix of direct loose-object opens, `access()` probes on writes, and abbreviation readdirs.

Out of scope:
- Serving packfiles from the store; `git fsck` and `rev-list --objects --all` without hydration.

Completion gate:
On a Linux mount with no hydration, `git log -n 1000`, `git log -- kernel/fork.c` and `git blame kernel/fork.c` match a full clone's output byte for byte, and the blame finishes within 15 s.

Testing plan:
- Git differential for log, log `-p`, blame, describe and merge-base across the shallow boundary a Phase 4 mount would have.
- Negative-lookup tests: `git commit` makes no fetch for new objects.
- Planner tests that bound fetch requests for blame and log.
- The git contract tests on every matrix version, nightly.
- Planted bugs: an unknown id that triggers a fetch, a readahead that skips a parent, a planner that misses side branches.

Status ledger:

| Status | Type | Item | Evidence / Gap |
| --- | --- | --- | --- |
| Incomplete | Work | 6A: `gvc-odb` filesystem | Missing: implementation and conformance run. |
| Incomplete | Work | 6B: No-shallow mode with readahead | Missing: alternates setup and readahead tests. |
| Incomplete | Work | 6C: Command-aware prefetch | Missing: planners and fetch-bound tests. |
| Incomplete | Work | 6D: Abbreviations and short SHAs | Missing: config and resolution tests. |
| Incomplete | Work | 6E: Git contract tests | Missing: strace harness and matrix job. |
| Incomplete | Gate | Linux log and blame identical to a full clone; blame within 15 s | Missing: timed differential run. |
| Incomplete | Test | Planted bugs caught | Missing: mutants report. |

## Phase 7: Comprehensive testing in CI

Goal:
Every behavior the design names has a test that fails when it breaks, PR CI finishes within 10 minutes, and nightly CI covers what PR CI samples.

Scope:
- 7A PR pipeline: lint, unit and property tests, FUSE integration split with `nextest --partition`, and the differential suite at a fixed seed budget, in parallel jobs with `rust-cache` and prebuilt git binaries.
- 7B Nightly pipeline: 10× the seeds, the git version matrix, the contract tests, fuzzing for 10 minutes a target, mutants, GitHub smoke tests, and an arm64 runner.
- 7C Fault-injection suite across every network and crash fault in the testing strategy, run on the differential scenarios.
- 7D Seeded random git differential at scale: generated command sequences over generated fixtures, each failure replayable from its seed and saved as a regression test.
- 7E `EVIDENCE.md`: each behavior the design names, the tests and planted bugs that check it, and the gaps.
- 7F Determinism: no sleeps in tests, no retries, a flaky test treated as a bug; seeds printed on failure.

Out of scope:
- Performance gating beyond instruction counts (Phase 9).

Completion gate:
PR CI's median wall time over 20 runs is 10 minutes or less; nightly CI passes seven nights running; every planted bug is caught; `EVIDENCE.md` lists no untested behavior.

Testing plan:
- The CI pipelines themselves, measured over 20 PR runs and seven nightly runs.
- A deliberate flaky test, to prove the pipeline reports it with its seed.

Status ledger:

| Status | Type | Item | Evidence / Gap |
| --- | --- | --- | --- |
| Incomplete | Work | 7A: PR pipeline within 10 minutes | Missing: workflow and timing over 20 runs. |
| Incomplete | Work | 7B: Nightly pipeline | Missing: workflow and seven green nights. |
| Incomplete | Work | 7C: Fault-injection suite | Missing: suite and run log. |
| Incomplete | Work | 7D: Seeded differential at scale | Missing: generator, replay command, regression-test harness. |
| Incomplete | Doc | 7E: `EVIDENCE.md` | Missing: document. |
| Incomplete | Work | 7F: Determinism rules | Missing: lint or review check, seed printing. |
| Incomplete | Gate | PR median within 10 minutes; seven green nights; all planted bugs caught | Missing: run ids and mutants report. |

## Phase 8: Builds and releases

Goal:
Each merge to `main` publishes binaries that install and run on stock Linux machines.

Scope:
- 8A Release builds for x86_64 and arm64 Linux: static musl binaries with LTO; `fuser` without libfuse, so `fusermount3` is the only runtime need.
- 8B GitHub releases with tarballs and `SHA256SUMS`, and `git-virtual-checkout` on crates.io.
- 8C Workflows and `RELEASING.md` in the style of lionturtle and inodedb: version bump and release on each merge to `main`.
- 8D Versioned on-disk formats for the store and overlay journal, with tests that the previous release's state opens.
- 8E `gvc doctor`: checks FUSE, `fusermount3`, git version, hooks, config and credentials, and says how to fix each.

Out of scope:
- macOS and Windows builds (M1 covers macOS).

Completion gate:
A release from `main` installs on clean Ubuntu 22.04 and 24.04 containers on x86_64 and arm64, passes `gvc doctor`, and passes the GitHub smoke suite.

Testing plan:
- A release-candidate job that runs the smoke suite in each container.
- Format-compatibility tests against the previous release's state.

Status ledger:

| Status | Type | Item | Evidence / Gap |
| --- | --- | --- | --- |
| Incomplete | Work | 8A: Static release builds | Missing: build job and binaries. |
| Incomplete | Work | 8B: GitHub releases and crates.io | Missing: first release. |
| Incomplete | Doc | 8C: Release workflows and `RELEASING.md` | Missing: workflows and doc. |
| Incomplete | Work | 8D: Versioned on-disk formats | Missing: format versions and compatibility tests. |
| Incomplete | Work | 8E: `gvc doctor` | Missing: command and tests on a misconfigured machine. |
| Incomplete | Gate | Release passes smoke tests on four clean containers | Missing: release run id. |

## Phase 9: Performance

Goal:
gvc meets measured targets for bootstrap, status, metadata, reads and memory, with benchmarks on tmpfs that CI can rerun, and the README's numbers come from gvc itself.

Scope:
- 9A Bench harness (`crates/bench`, `scripts/bench`): the agent replay suite, clone to first listing, clean and dirty `git status`, `find` and `ls -R`, cold-read p50 and p99, warm read throughput, a build-like read of every source file, `gvc switch`, and blame. The store, upper layers and baseline checkouts live on tmpfs, against the testkit server at 0, 20 and 80 ms injected round trips and against GitHub.
- 9B Targets, each measured against a stock checkout on tmpfs in the same run:
  - clone to first listing: Linux within 1 s;
  - clone to a written index: Linux at or under 4.4 s, the median of the git and API calls that stand in for `gvc clone` today;
  - clean `git status` on Linux: within 50 ms with fsmonitor, and within 2× stock without it;
  - `find` over Linux on a warm mount: within 2× stock;
  - warm sequential reads: at least 80% of stock throughput;
  - a full-tree read: at most files / 100 fetch requests;
  - daemon memory for a Linux mount: at most 150 MB.
- 9C Optimizations: a size walk that issues every subtree call at once from the fetched trees; FUSE passthrough for upper and hydrated files; entry, attr and `readdirplus` caching; read-ahead and coalescing windows; HTTP/2 multiplexing; parallel tree parsing; a compact tree index; multiple FUSE channels; FUSE over io_uring where the kernel has it; fewer allocations per request.
- 9D Regression gates: instruction counts (`iai-callgrind`) for the hot paths on every PR; wall-clock benchmarks nightly with a tolerance.
- 9E README, `BENCHMARKS.md` and charts regenerated from `gvc` runs in place of the git commands that stand in for it today.

Out of scope:
- Tuning for macOS (M1).

Completion gate:
Every 9B target is met in a recorded run, `BENCHMARKS.md` lists the commands that reproduce it, and the README's charts come from `gvc`.

Testing plan:
- The bench harness, run three times per configuration, reporting medians.
- The instruction-count gate failing on a planted 10% slowdown.

Status ledger:

| Status | Type | Item | Evidence / Gap |
| --- | --- | --- | --- |
| Incomplete | Work | 9A: Bench harness on tmpfs | Missing: harness and first recorded run. |
| Incomplete | Gate | 9B: Linux first listing within 1 s, index within 4.4 s | Missing: recorded run. |
| Incomplete | Gate | 9B: Clean status within 50 ms with fsmonitor, 2× stock without | Missing: recorded run. |
| Incomplete | Gate | 9B: `find` within 2× stock | Missing: recorded run. |
| Incomplete | Gate | 9B: Warm reads at 80% of stock | Missing: recorded run with passthrough. |
| Incomplete | Gate | 9B: Full-tree read within files / 100 requests | Missing: counter log. |
| Incomplete | Gate | 9B: Daemon memory within 150 MB on Linux | Missing: RSS measurement. |
| Incomplete | Work | 9C: Optimizations | Missing: each change with its before and after. |
| Incomplete | Work | 9D: Regression gates | Missing: PR instruction-count job and nightly wall-clock job. |
| Incomplete | Doc | 9E: README and benchmarks from `gvc` | Missing: regenerated `BENCHMARKS.md` and charts. |

## M1: macOS through FSKit

Goal:
gvc runs on macOS through an FSKit extension that is a Swift shim over the same Rust crates, with every filesystem behavior unchanged.

Scope:
- M1A `gvc-ffi`: a C ABI over `gvc-vfs`'s `Filesystem` and the daemon's control surface, with a generated header.
- M1B A Swift FSKit extension (`FSUnaryFileSystem`) that maps FSKit items to node ids and replies through completions.
- M1C Caller identity for command-aware prefetch, or a fallback to command-free planners where FSKit gives none.
- M1D Signing, notarization and installation of the extension.
- M1E The VFS conformance suite's syscall half and the git differential suite on macOS runners.

Out of scope:
- macFUSE and FUSE-T.

Completion gate:
On Apple silicon, the VFS conformance suite and the git differential suite pass through the FSKit extension.

Testing plan:
- The shared suites on a macOS runner, nightly.

Status ledger:

| Status | Type | Item | Evidence / Gap |
| --- | --- | --- | --- |
| Incomplete | Work | M1A: `gvc-ffi` C ABI | Missing: crate and header. |
| Incomplete | Work | M1B: Swift FSKit extension | Missing: extension. |
| Incomplete | Decision | M1C: Caller identity under FSKit | Missing: what FSKit exposes, and the planner fallback. |
| Incomplete | Work | M1D: Signing and installation | Missing: signed build. |
| Incomplete | Gate | Shared suites pass through FSKit | Missing: macOS CI run. |

## B1: A second git host

Goal:
A GitLab or Gitea repository clones and works like a GitHub one, through the git-protocol backend plus that host's accelerators.

Scope:
- B1A Host detection and auth for the new host.
- B1B Its accelerators: tree listings with sizes where the API offers them, and blob fetch on first `getattr` where it doesn't.
- B1C The GitHub smoke suite, ported to the new host.

Out of scope:
- Hosts without partial-clone support.

Completion gate:
The git differential suite passes against the new host's server in CI.

Testing plan:
- The differential suite against a containerized Gitea or GitLab.

Status ledger:

| Status | Type | Item | Evidence / Gap |
| --- | --- | --- | --- |
| Incomplete | Work | B1A: Host detection and auth | Missing: implementation. |
| Incomplete | Work | B1B: Host accelerators | Missing: implementation and fallback test. |
| Incomplete | Work | B1C: Smoke suite on the new host | Missing: suite run. |
| Incomplete | Gate | Differential suite passes against the new host | Missing: CI run. |
