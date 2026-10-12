# Searching

There are two searches, and they answer different questions.

| | Workspace **Search** | `centinel search`, MCP, HTTP |
|---|---|---|
| Returns | documents | ranked passages |
| Matches | every word you typed, in the same passage | meaning as well as words |
| Narrow by | source, usage, classifier score, address | source, tags |
| Best for | browsing, sorting, finding a known document | answering a question and citing it |

Use the workspace to find your way around. Use the passage search when you want the best
paragraph for a question, with the provenance to cite it.

## In the workspace

**Search** opens on one box. It lists nothing until you ask, because a corpus of hundreds of
thousands of documents is not a list. The line above the box counts what is stored by kind
(web pages, PDFs, spreadsheets) and says how many sources there are.

Type words, or pick filters, or both, and press the arrow. The filters sit along the foot of
the box, and each one opens its own panel and shows what it would find.

| Filter | What it does |
|---|---|
| **Classifiers** | Pick one question and a score range. A bar for each tenth of the score shows how the matching documents spread, with your range lit. |
| **Sources** | Pick any number. Each shows how many documents it holds under the other filters. |
| **Usage** | **Included** or **Excluded** (by the classifiers or by a person), or **Pending**, which are still waiting on an answer from a question that applies to them. |
| **Address** | Documents whose address or title contains this text. |

Each filter counts with the others applied but not its own, so you can see what choosing it
would do before you choose it. Before a search, the filters wait in the box and go with
your words. After one, they refine the results at once. **Clear** drops them all, and the
arrow at the left of the box starts a new search.

### What a match is

A document matches when one of its passages contains **every word** you typed. Case does
not matter and word endings are folded, so `fees` finds `fee`. Each word is otherwise taken
as written, so `budget-2026` and `AT&T` are searched as typed and not parsed as syntax.
Nothing is matched by meaning. A document about "drinking water" is not found by a search
for `potable`, and the list is not ranked by how well a document answers you.

If you know the document is there and the words are not, or you do not know the words, use
the passage search below.

### The results

One row per document: its title, its source, its address, how large it is, the tags and
scores the classifiers gave it, and the short hash of the original bytes at the right.
Excluded documents stay in the list, dimmed and marked **excluded**, until you choose
**Included** under Usage. Click a row to open it in the [reader](read.md).

When a search names a single source, **Classify this source** appears beside the count and
takes you to the Classify page with that source chosen.

A search across a large corpus can take a while. The page shows the seconds it has been
working, and what it already found stays up while a refinement runs.

## Ranked passages

This is the search for questions. It is a command, an MCP tool and an HTTP route, all one
definition, and it is how an agent searches. [Agents and programs](agents.md) wires one up.

```bash
centinel search "stormwater drainage fee"
centinel search "budget" --source agartha -n 20
centinel search "lobbyist" --snippet-chars 0        # whole chunk, not an excerpt
centinel search "stormwater" --tag record_type:minutes --not-tag record_type:data_table
```

| Flag | Default | Meaning |
|---|---|---|
| `-n`, `--limit` | 10 | maximum results |
| `--source` | all | restrict to one source |
| `--snippet-chars` | 400 | characters of the passage to return; `0` returns all of it |
| `--tag` | none | keep documents with this tag. Repeat for AND |
| `--not-tag` | none | drop documents with this tag |

A tag is spelled the way the classifier answers are stored: a yes-or-no question's id, or
`choice:option`, such as `record_type:minutes`. The filter reaches both arms of the search.
A tag no saved question defines is refused with the list to check, so a typo is an error
rather than an empty result. The tags are the ones [Classify](classify.md) puts on documents.

### What comes back

One ranked passage per result, with everything needed to cite it:

| Field | What it is |
|---|---|
| `text` | the passage |
| `title` | the document's own name |
| `heading` | the markdown heading trail the passage sits under |
| `source`, `url` | which corpus, which address |
| `observed_at` | when we fetched it |
| `tool` | which extraction pipeline produced this text |
| `blob_sha` | hash of the **original bytes as served** — the evidentiary anchor |
| `derived_sha` | hash of the derived text the character span indexes into |
| `chunk_hash` | hash of the passage itself |
| `char_start`, `char_end` | the span inside the derived text |
| `tags` | the tags the classifiers or a person put on the document |
| `also_at` | other addresses carrying this identical passage |

`also_at` is not decoration. The same paragraph appears on fifty pages of a council site,
and each of those addresses is its own document with its own bytes and its own history.
Each entry carries its own hash, so you can open any of them.

### Two arms, and why the second one matters

**BM25 catches exact tokens.** Names, motions, ordinance numbers, dollar figures — most of
what people actually search meeting records for.

**The vector arm closes the vocabulary gap.** Measured on a real corpus: `"drinking water
sampling results"` returns *nothing* from keyword search, because the water report says
`PWSName`, `Analyte` and `UCMR 5`, and the only chunk containing the word "drinking" is a
tax table about *Drinking Places (Alcoholic Beverages)*. BM25 is behaving correctly and is
still useless.

Both run, both return their top 100, the two rankings are fused, and a cross-encoder
reranks the survivors. There is no flag to skip the reranker, because the measured gap is
too large to make it an option. [Search](../internals/search.md) has the mechanism.

### Read the header line

```
stormwater drainage fee    2 results · bm25→rerank
! keyword search only — no vectors at ~/.centinel/chunks.lance — run `centinel embed` first
```

The `method` field names which stages actually ran, and it is assembled from what ran
rather than written out per call site. Four values are possible:

| `method` | What it means |
|---|---|
| `bm25` | keyword only, unreranked — the weakest answer this tool gives |
| `bm25→rerank` | keyword, reranked — no vector table yet |
| `bm25+vector→rrf` | both arms fused, unreranked — reranker weights missing |
| `bm25+vector→rrf→rerank` | everything ran |

**This is the field to read first.** A rank is a position *inside* a set and says nothing
about the size of that set. The fusion weights by rank alone, so the vector arm's rank 1
counts exactly the same whether it was drawn from 397,830 vectors or from 2,309. A partly
embedded corpus therefore does not degrade gently — it promotes confident results from a
tiny pool and looks identical to a complete one.

So `no_vectors` and `no_rerank` carry *why* a stage did not run. An absent stage is a
different answer, not a slower one.

### What a search cannot tell you

How much of the corpus the vector arm could see. The report used to print a share from a
chunk count, and at 21.7 million chunks that count took 99 seconds on every search; the
share also ignored your filters and counted stale vectors. The honest figure is a set
difference, and `centinel embed --dry-run` prints it: chunks indexed, already embedded,
remaining, stale.

A chunk's absence from an arm is a fact about what has been **processed**, never about
whether it answers your question. The same holds one stage earlier: a PDF that failed
extraction is not in the index at all, and no search will report its absence. `centinel
list`, the run report and `centinel embed --dry-run` are where coverage lives.

If you searched for something you are confident is in the corpus and got nothing, the
order to check is:

1. Was it collected? `centinel list` shows resource counts and liveness per source.
2. Was text derived from it? The run report counts unreadable documents per stage.
3. Was it indexed? `centinel embed --dry-run` prints the chunks indexed.
4. Was it embedded? The same report prints how many of them have a vector.
5. Was it excluded? A classifier or a person may have taken it out of search. The
   workspace's **Usage** filter shows what is excluded, and
   [Review](review.md) is where a person puts one back.

### Cost

A warm process answers in about a second — the reranker dominates. A cold CLI invocation
pays the model load every time: 11 seconds measured on a corpus with no vector table, so
with only the 0.6B reranker loaded. A query that also builds the 4B embedder pays more.

`centinel serve` and `centinel mcp` load both once and keep them. If you are going to ask
more than a couple of questions, ask them through one of those.

Next: [Reading a document](read.md).
