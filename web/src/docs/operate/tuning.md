# Tuning for a machine

Nothing in Centinel is built for one machine. Every figure that depends on the machine is a
key in `centinel.toml`, and this page says how to work each one out. It is written so you
can hand it to an agent with the facts below and ask: *given this machine, what should
`centinel.toml` say, and what should I run?*

## What to collect first

Run these on the machine that holds the store, and give the output to whoever is tuning:

```sh
centinel doctor --json                # memory budget, servers, the vector table's model
nproc                                 # cores
free -b                               # memory
df -h ~/.centinel                     # disk free where the store lives
lsblk -d -o NAME,ROTA,SIZE,MODEL      # spinning or solid state (a VM disk may say 1 either way)
grep -o -m1 'avx512f\|avx2' /proc/cpuinfo   # vector instructions; none means an old CPU
du -sb ~/.centinel/chunks.lance       # the vector table's size on disk
```

Then time a search on a question you know the answer to:

```sh
centinel search -v "storm water project" -n 5
```

`-v` logs `vector arm … ms=…`. That is the number to bring down. The keyword arm and the
fusion come after it in the same log.

## The quantities

Most of the decisions below come from these:

| | How to get it |
|---|---|
| **V**, vectors | `du -sb chunks.lance` ÷ (4 × **d** + 600), or `already_embedded` from `centinel embed --dry-run` |
| **d**, dimensions | the embedding model's width: 4,096 for Qwen3-Embedding-8B, 2,560 for 4B |
| **B**, the memory budget | `doctor`'s memory line: `[resources] memory`, else ¾ of the machine |
| **index cache** | B ÷ 4, between 64 MiB and 6 GiB. Where a search keeps the index it has read |
| **room** | B less the Lance caches (`doctor`'s "after Lance's caches"). What a build trains in |
| **read speed** | sequential: `dd if=<a large file in chunks.lance/data> of=/dev/null bs=4M count=256 iflag=direct` |
| **random read** | the time to read one vector, d × 4 bytes, from a random place: ~0.1 ms on an SSD, several ms on a spinning or network disk |

The vector table is about V × d × 4 bytes. The field store held V = 2.7 million at
d = 4,096: 44 GB.

## The vector index: `[vectors]`

This is the setting that matters most on a large corpus. Without an index, every search
reads the whole vector table. Each option costs, per query:

| `index` | Reads per query | Fits in memory when |
|---|---|---|
| `"none"` | the whole table, V × d × 4 | the table is smaller than the page cache |
| `"ivf_flat"` | `probes` ÷ `partitions` of the table | that share is cached |
| `"ivf_pq"` | `probes` ÷ `partitions` of V × `sub_vectors` bytes, then `refine` × 100 full vectors | V × `sub_vectors` ≤ the index cache |

The field store with no index: 44 GB ÷ 266 MB/s ≈ 170 s, and 107 s measured with a filter.

Work it out in this order:

1. **Under ~100,000 vectors, or a table that fits in memory twice over:** `index = "none"`
   is exact, and fast enough. An index buys nothing yet.
2. **Otherwise `index = "ivf_pq"`**, the default. Size its compressed copy, **`sub_vectors`**
   bytes a vector, to the index cache: the largest of d ÷ 8, d ÷ 16 or d ÷ 32 for which
   V × `sub_vectors` ≤ B ÷ 4. It must divide d. At V = 2.7 M and d = 4,096, d ÷ 16 = 256
   bytes is 0.7 GB, and d ÷ 32 = 128 is 0.35 GB.
3. **`partitions`**: leave it unset (√V, 1,658 at 2.7 M) unless a build runs out of room.
   Fewer partitions train on less memory and read more per probe.
4. **`probes`**: 64 is the default, which is 4% of the codes at 1,658 partitions. Raise it
   when recall is short, and lower it when the vector arm is slow and the codes are not
   cached.
5. **`refine`**: each unit re-reads 100 full vectors, d × 4 bytes each, from wherever they
   are. On an SSD 2–4 costs well under a second. On a disk with several milliseconds a read,
   keep it at 1–2, or give `sub_vectors` more bytes instead.
6. **`ivf_flat`** only where memory is plentiful and an exact order inside each partition
   matters more than speed. It keeps full vectors, so the index is as large as the table.

Building takes its own command, and only runs when you run it:

```sh
centinel vector-index
```

`embed` never builds one, but it says at the end of its report when the table has none or
has another than `[vectors]` names. Run `vector-index` again after changing `index`,
`partitions` or `sub_vectors`, and once the table has grown several times over since the
last build. `probes` and `refine` apply to the next search, with no rebuild.

A build trains in **room**. It reserves all of it and runs alone, so stop other work on the
store first. Training holds two copies of each vector it samples, 2 × d × 4 bytes: 32 KiB at
4,096 dimensions. At full quality that is 256 × `partitions` vectors for the partitions, in
steps sized to the room, and 65,536 for the compressed copy, 2 GiB at 4,096 dimensions. With
less room the partitions train in more, smaller steps, and the compressed copy trains on
fewer vectors, which costs it some accuracy. More `[resources] memory` buys a better copy.
A step never holds fewer than one vector a partition, so a room smaller than `partitions` ×
2 × d × 4 bytes is overrun by that much: lower `partitions` there. The build also needs disk beside the table:
V × `sub_vectors` for `ivf_pq`, or the table's size again for `ivf_flat`.

Check the result against exact search on questions you know:

```sh
centinel search "question"
centinel search --exact "question"
```

If the indexed answer misses passages the exact one finds, raise `probes`, then `refine`.
`vector_bench` measures latency and recall@10 for a size and width without touching the
store; see [Search](../internals/search.md#large-corpora).

## Memory: `[resources] memory`

One budget, B, for everything a run holds: extraction reads, collection bodies, the Lance
caches, and an index build. ¾ of the machine is the default. Set it lower on a machine
shared with other work, such as a GPU server, a database, or a VM whose host balloons
memory away; `doctor` warns about that one. Set it higher only on a machine that runs
nothing else.

```toml
[resources]
memory = "2.5 GiB"
```

The index cache is a quarter of B, at most 6 GiB, so B also decides how large an `ivf_pq`
index stays in memory (step 2 above).

## Embedding: `[inference.embed]`

Embedding waits on the server, not on this machine. `batch` (64) is chunks per request and
`concurrency` (4) is requests in flight, at most 32. A hosted API like OpenRouter ran ~61
chunks a second at the defaults. For a local `llama-server`, the server's own `--ubatch-size`
and `--batch-size` must fit its memory. A width the machine cannot hold returned NaNs or
crashed the server rather than failing cleanly, so step it up until throughput stops rising
and stay below that. See [Inference servers](../reference/inference.md).

## Reranking: `[inference.rerank]`

Every search reranks its top 40 passages. On a GPU that is well under a second. On a CPU it
can be many seconds, which makes it the largest part of a query once the vector index is
built. With no reranker answering, search runs without one and says so. Point it at a GPU
server, or a hosted one, rather than running it on a small machine's CPU.

## Starting points

These are starting points to measure from, not results.

| | Small VM | Laptop | Workstation |
|---|---|---|---|
| Memory, disk | 4 GB, network or spinning disk | 16 GB, SSD | 64 GB+, NVMe |
| `[resources] memory` | default (~2.4 GiB), or less if shared | default (12 GiB) | default (48 GiB) |
| index cache | ~0.6 GiB | 3 GiB | 6 GiB |
| `index` at 2.7 M × 4,096 | `ivf_pq` | `ivf_pq` | `ivf_pq` (the table nearly fits in memory; `none` is seconds once cached) |
| `sub_vectors` | 128 (d ÷ 32, 0.35 GB) | 256 (default) | 512 (d ÷ 8, 1.4 GB) |
| `probes` | 32–64 | 64 | 64–128 |
| `refine` | 1–2 | 2–4 | 4 |
| reranker | a remote GPU, or none | local GPU if there is one | local GPU |

The small VM is the field machine: 3.3 GB, four cores without AVX2, a virtual disk that
reads 266 MB/s and takes a few milliseconds per random read. Its index cache cannot hold
the default 0.7 GB of codes, and every refined vector is a slow random read, so it gets the
smaller copy and less refine. A machine without AVX2 runs the release built for it
(`x86_64-unknown-linux-gnu-v2`), which `install.sh` picks on its own. Its distance
arithmetic is slower, which is one more reason to read fewer vectors.

## For an agent doing this

1. Collect the output of the commands in [What to collect first](#what-to-collect-first).
2. Work out V, d, B, the index cache and the read speeds from it.
3. Decide `[vectors]` in the order of [the vector index](#the-vector-index-vectors), and
   `[resources] memory` if the machine is shared.
4. Write only the keys that differ from the defaults into `centinel.toml`, each with a
   comment giving the arithmetic that chose it.
5. Run `centinel vector-index` if `index`, `partitions` or `sub_vectors` changed.
6. Time `centinel search -v` on three known questions, and compare each with
   `--exact`. Report the vector arm's time before and after, and any passage the index
   missed.
7. If the arm is still slow, find which read dominates (codes not cached, or refine's
   random reads) and adjust that one setting. Then measure again.
