# Benchmarks

gvc isn't built yet. These runs time stock git, and the git and GitHub API calls that `gvc clone` will make, as the baseline Phase 9 of [PLAN.md](PLAN.md) measures gvc against.

## Clone to a usable checkout

Run on 2026-11-14 on an Intel Core Ultra 9 285 (24 threads, 125 GB) over a home connection, with Linux 7.0, git 2.53.0 and an authenticated GitHub token. Every clone landed on tmpfs, so the disk drops out. Each method ran three times per repository and each figure is the median, except the Linux full clone, which ran once.

| Method | What runs | Ends when |
|---|---|---|
| `git clone` | `git clone` | Every file is checked out, with full history |
| `git clone --depth=1` | `git clone --depth=1 --single-branch` | Every file is checked out, with one commit |
| `gvc clone` | `git ls-remote`, then `git clone --filter=blob:none --depth=1 --no-checkout --single-branch` beside [`scripts/bench/tree-sizes`](scripts/bench/tree-sizes), in parallel | The commit, every tree, and every blob's size are local; files fetch on first read |

The gvc stand-in overstates gvc's time by one round trip, since `git clone` repeats the ref listing that gvc makes once, and leaves out two steps gvc adds: writing the index and mounting.

| Repo | Files | Method | Time | Downloaded | On disk |
|---|--:|---|--:|--:|--:|
| torvalds/linux | 97,371 | `git clone` | 518 s | 6.6 GB | 8.3 GB |
| torvalds/linux | 97,371 | `git clone --depth=1` | 40 s | 304 MB | 2.0 GB |
| torvalds/linux | 97,371 | `gvc clone` | 4.4 s | 11.1 MB | 3.4 MB |
| facebook/react | 7,275 | `git clone` | 61 s | 1.2 GB | 1.2 GB |
| facebook/react | 7,275 | `git clone --depth=1` | 2.8 s | 11.4 MB | 53.1 MB |
| facebook/react | 7,275 | `gvc clone` | 1.1 s | 642 KB | 308 KB |
| danthegoodman1/lionturtle | 83 | `git clone` | 1.1 s | 5.7 MB | 8.1 MB |
| danthegoodman1/lionturtle | 83 | `git clone --depth=1` | 0.5 s | 1.7 MB | 4.1 MB |
| danthegoodman1/lionturtle | 83 | `gvc clone` | 0.6 s | 10.5 KB | 33.1 KB |

- **Downloaded** is the size of the clone's packs, close to the bytes received. For the stand-in, it adds the tree API's JSON as received with gzip.
- **On disk** is `du -sb` of the clone.
- **Files** counts `git ls-files` for stock clones and the tree API's blobs for the stand-in.

### Notes

- **The Linux size walk.** The recursive tree call truncates at 100,000 entries or 7 MB, so `tree-sizes` lists the root and walks each subtree in parallel: 26 calls and 7.7 MB of gzipped JSON, taking 3.8 s, 4.1 s and 4.3 s across the three runs. Without gzip, the same walk took 5.0 to 8.2 s.
- **GitHub's server time.** It dominates the Linux bootstrap and varied: the whole stand-in took 4.1 s, 4.4 s and 8.4 s, and the blobless fetch alone has taken anywhere from 1.1 s to 15 s.
- **Small repositories.** For lionturtle, a shallow clone took 0.50 s against the stand-in's 0.57 s, since both come down to a round trip or two.

The raw runs are in [docs/bench/clones.jsonl](docs/bench/clones.jsonl).

### Reproducing

```console
scripts/bench/clones --work /dev/shm --out docs/bench/clones.jsonl danthegoodman1/lionturtle facebook/react
scripts/bench/clones --work /dev/shm --full-runs 1 --out docs/bench/clones.jsonl torvalds/linux
scripts/bench/charts   # rewrites docs/images/clone-*.svg and prints the table above
```

`--work` must hold a full Linux clone, about 8 GB.

## Agent workloads: the G1 estimate

Gate G1 in [PLAN.md](PLAN.md) decides whether gvc gives online agent workloads a clear win. gvc isn't built, so this estimate assembles gvc's cost from the pieces it will run, each measured on its own, and measures two stock setups end to end. It shows whether the gate is reachable and which assumptions carry the risk; G1 itself runs on Phase 3's build.

### The session

[`scripts/bench/agent-replay`](scripts/bench/agent-replay) replays a session shaped like agent traces: `git status`, `git log -n 10`, line-range reads of 30 files, greps of 6 named directories, 3 edits, `git diff`, `git show --stat HEAD~3`, `git show HEAD~5:<path>`, a commit, and a whole-tree grep. A shallow workspace that lacks the history deepens by 20 generations first, as an agent would after the error. Grep output goes to a pipe, as to an agent's tool: GNU grep stops at its first match when it writes to `/dev/null`.

### How each side was measured

- **Stock**, end to end, by [`scripts/bench/workspaces`](scripts/bench/workspaces): `git clone --depth=1` per workspace, and one blobless depth-1 clone with a `git worktree` per workspace, for 1 and 8 workspaces. Local steps (greps) come from the native rows below.
- **gvc's network work**, three runs each, by [`scripts/bench/gvc-fetches`](scripts/bench/gvc-fetches), on the `.git` that `gvc clone` builds: the history window, one batched fetch per read directory, the grep directories' blobs, `git show` with git's own lazy fetches, the tarball, and adding a workspace (`git worktree add --no-checkout` plus an index read from the trees).
- **gvc's local work**, by [`scripts/bench/fuse-overhead`](scripts/bench/fuse-overhead), through [`bench/passfs`](bench/passfs), a read-only FUSE passthrough with hour-long kernel caching, zero-message opens and `readdirplus`, against the same checkout on tmpfs.
- **The estimate**, by [`scripts/bench/g1-estimate`](scripts/bench/g1-estimate), from medians.

### Results

#### torvalds/linux

| Measure | `git clone --depth=1` each | blobless clone + worktrees | gvc, estimated |
|---|--:|--:|--:|
| Ready, 1 workspace | 38 s | 39 s | 4.45 s |
| Ready, 8 workspaces | 57 s | 87 s | 4.97 s |
| Downloaded, 1 workspace | 300.4 MB | 301.1 MB | 11.1 MB |
| Downloaded, 8 workspaces | 2.4 GB | 301.2 MB | 11.1 MB |
| Disk, 8 workspaces | 15.9 GB | 13.8 GB | 87.5 MB |
| Session, back to back (cold / warm) | 33 s | 3.85 s | 2.72 s / 0.46 s |
| Whole-tree grep (cold / warm) | 0.84 s | 0.84 s | 14 s / 0.86 s |
| Session downloads beyond the bootstrap | — | — | 6.5 MB, or 274.5 MB once the tree is streamed in |

#### facebook/react

| Measure | `git clone --depth=1` each | blobless clone + worktrees | gvc, estimated |
|---|--:|--:|--:|
| Ready, 1 workspace | 2.48 s | 4.57 s | 1.15 s |
| Ready, 8 workspaces | 3.64 s | 5.55 s | 1.24 s |
| Downloaded, 1 workspace | 11.1 MB | 11.5 MB | 642.0 KB |
| Downloaded, 8 workspaces | 89.1 MB | 11.5 MB | 642.0 KB |
| Disk, 8 workspaces | 425.1 MB | 346.2 MB | 9.8 MB |
| Session, back to back (cold / warm) | 1.25 s | 1.51 s | 1.84 s / 0.82 s |
| Whole-tree grep (cold / warm) | 0.04 s | 0.04 s | 1.61 s / 0.04 s |
| Session downloads beyond the bootstrap | — | — | 1.1 MB, or 10.2 MB once the tree is streamed in |

#### danthegoodman1/lionturtle

| Measure | `git clone --depth=1` each | blobless clone + worktrees | gvc, estimated |
|---|--:|--:|--:|
| Ready, 1 workspace | 0.53 s | 0.88 s | 0.58 s |
| Ready, 8 workspaces | 0.73 s | 0.89 s | 0.62 s |
| Downloaded, 1 workspace | 1.7 MB | 1.7 MB | 10.5 KB |
| Downloaded, 8 workspaces | 13.6 MB | 1.7 MB | 10.5 KB |
| Disk, 8 workspaces | 32.9 MB | 20.8 MB | 98.7 KB |
| Session, back to back (cold / warm) | 1.14 s | 1.22 s | 0.83 s / 0.45 s |
| Whole-tree grep (cold / warm) | 0.01 s | 0.01 s | 0.54 s / 0.01 s |
| Session downloads beyond the bootstrap | — | — | 3.1 MB, or 1.7 MB once the tree is streamed in |

#### Criteria

Cold sessions run back to back with no think time. The last column has the session's
whole-tree grep 5 s after the workspace is ready, the 10th percentile in agent traces,
while gvc streams the tree in the background from the moment it's ready.

| Repo | Ready, 1 workspace | Ready, 8 workspaces | Bytes, 8 workspaces | Cold session, no whole-tree grep | Cold session with a whole-tree grep | Whole-tree grep 5 s after ready |
|---|--:|--:|--:|--:|--:|--:|
| torvalds/linux | 8.5× faster | 11.5× faster | 27× fewer | 7.17 s vs 43 s (0.17×) | 21 s vs 44 s (0.48×) | 19 s vs 44 s (0.42×) |
| facebook/react | 2.2× faster | 2.9× faster | 18× fewer | 2.99 s vs 3.73 s (0.80×) | 4.60 s vs 3.78 s (1.22×) | 6.31 s vs 7.52 s (0.84×) |
| danthegoodman1/lionturtle | 0.9× faster | 1.2× faster | 161× fewer | 1.41 s vs 1.66 s (0.85×) | 1.94 s vs 1.67 s (1.16×) | 5.58 s vs 5.53 s (1.01×) |

### FUSE's cost

With zero-message opens (`FUSE_NO_OPEN_SUPPORT`), the kernel stops sending open and release for each file, and warm runs match native. The first pass over files costs a round trip per file:

| Repo | Step | Native | FUSE, first pass | FUSE, warm |
|---|---|--:|--:|--:|
| torvalds/linux | `find` over the tree | 0.055 s | 0.045 s | 0.044 s |
| torvalds/linux | `git status`, no fsmonitor | 0.075 s | 0.082 s | 0.080 s |
| torvalds/linux | `git status`, fsmonitor hook | 0.024 s | 0.024 s | 0.026 s |
| torvalds/linux | 6 directory greps | 0.036 s | 0.145 s | 0.037 s |
| torvalds/linux | Whole-tree grep | 0.839 s | 2.673 s | 0.856 s |
| facebook/react | `find` over the tree | 0.008 s | 0.009 s | 0.008 s |
| facebook/react | `git status`, no fsmonitor | 0.009 s | 0.012 s | 0.011 s |
| facebook/react | `git status`, fsmonitor hook | 0.006 s | 0.006 s | 0.006 s |
| facebook/react | 6 directory greps | 0.019 s | 0.029 s | 0.019 s |
| facebook/react | Whole-tree grep | 0.043 s | 0.165 s | 0.044 s |

Without zero-message opens, every open and release reached the daemon: on react, `find` ran 6.4× slower than native, `git status` without fsmonitor 5.9×, and six whole-tree greps 3.2×. Unprivileged FUSE passthrough fails with `EPERM`, since Linux requires `CAP_SYS_ADMIN` to register a backing file.

### Streaming the tree beats a shallow clone

Alternating the two on Linux, `git clone --depth=1` took 36.4 s, 36.9 s and 24.5 s, while downloading and extracting the commit's tarball took 10.1 s, 13.4 s and 14.2 s. gvc's worst case, streaming in the whole tree for a whole-tree grep, finishes before the shallow clone would.

### Caveats

- **GitHub's latency swings.** The same 37-blob read-ahead took 28.4 s, 0.31 s and 0.48 s; the tarball 39.8 s, 11.5 s and 10.3 s; the history window 10.1 s, 3.5 s and 3.3 s. Stock and gvc ran at different times, except the alternating check above. G1 must alternate them.
- **No writes through FUSE.** The passthrough is read-only, so builds and writes to the upper layer are unmeasured.
- **Left out of gvc's side:** hashing streamed files against their tree oids, and the mount itself.
- **Think time.** Agents pause between commands, which this replay leaves out except in the last column; gvc can download during those pauses and stock setups have nothing to download.

### Reproducing

```console
scripts/bench/workspaces shallow torvalds/linux 1 --work /dev/shm --out docs/bench/agents-sessions.jsonl
scripts/bench/workspaces worktree torvalds/linux 8 --work /dev/shm --out docs/bench/agents-stock.jsonl --no-session
scripts/bench/gvc-fetches torvalds/linux --work /dev/shm --out docs/bench/gvc-fetch.jsonl
(cd bench/passfs && cargo build --release)
git clone --depth=1 https://github.com/torvalds/linux /dev/shm/linux
PASSFS_NO_OPEN=1 PASSFS_READDIRPLUS=1 scripts/bench/fuse-overhead torvalds/linux /dev/shm/linux \
  bench/passfs/target/release/passfs --out docs/bench/fuse.jsonl
scripts/bench/g1-estimate
```
