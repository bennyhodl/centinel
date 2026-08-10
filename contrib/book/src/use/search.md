# Searching

```bash
centinel search "stormwater drainage fee"
centinel search "budget" --source agartha -n 20
centinel search "lobbyist" --snippet-chars 0        # whole chunk, not an excerpt
```

| Flag | Default | Meaning |
|---|---|---|
| `-n`, `--limit` | 10 | maximum results |
| `--source` | all | restrict to one source |
| `--snippet-chars` | 400 | characters of the passage to return; `0` returns all of it |

## What comes back

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
| `also_at` | other addresses carrying this identical passage |

`also_at` is not decoration. The same paragraph appears on fifty pages of a council site,
and each of those addresses is its own document with its own bytes and its own history.
Each entry carries its own hash, so you can open any of them.

## Two arms, and why the second one matters

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

## Read the header line

```
stormwater drainage fee    2 results · bm25→rerank · 397,830 chunks indexed
! keyword search only — no vectors at ~/.centinel/vectors.lance — run `centinel embed` first
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

So the report always carries `total_chunks_indexed` beside `vectors_indexed`, and the
terminal prints the share whenever it is not 100%. `no_vectors` and `no_rerank` carry
*why* a stage did not run. An absent stage is a different answer, not a slower one.

## What a search cannot tell you

A chunk's absence from an arm is a fact about what has been **processed**, never about
whether it answers your question. The same holds one stage earlier: a PDF that failed
extraction is not in the index at all, and no search will report its absence. `centinel
list` and the run report are where coverage lives.

If you searched for something you are confident is in the corpus and got nothing, the
order to check is:

1. Was it collected? `centinel list` shows resource counts and liveness per source.
2. Was text derived from it? The run report counts unreadable documents per stage.
3. Was it indexed? `total_chunks_indexed` in the search report.
4. Was it embedded? `vectors_indexed` in the same report.

## Cost

A warm process answers in about a second — the reranker dominates. A cold CLI invocation
pays the model load every time: 11 seconds measured on a corpus with no vector table, so
with only the 0.6B reranker loaded. A query that also builds the 4B embedder pays more.

`centinel serve` and `centinel mcp` load both once and keep them. If you are going to ask
more than a couple of questions, ask them through one of those.

Next: [Reading a result](read.md).
