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
