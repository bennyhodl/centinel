# Spend

**Spend** is every model call Centinel has made and what it cost. It exists because some of
the work is paid for: a [classifier run](classify.md) sends documents to Jev, and embeddings
can be computed by a cloud provider. A number you can read at a glance is how you notice a run
that cost more than you meant it to.

The page is called Spend and not Usage because Usage already means a corpus decision, whether
a document is included in search.

## What counts

One line per call goes into `spend/YYYY-MM.jsonl` in the store: a document Jev scored (every
level of its chain), a batch of chunks embedded, a query vector made for a search. Each line
carries the model as it was called, its tokens, and its cost.

- **A cost is fixed on the day of the call.** It is worked out from one price table in the
  source at the moment the call is made. A later price change re-prices nothing, which is
  what a bill says too.
- **A local model is priced as the cloud.** `qwen3-embedding-4b` running on your machine is its
  own line on the page, never filed under a cloud provider, but it is priced at the cloud model
  with the same weights. The tokens are exact, since the weights and the tokenizer are the
  same. The page says what running it there would have cost.
- **A model with no price is counted and left unpriced.** Its cost is unknown, not zero, so
  its line shows tokens and `Unpriced`, and it has no cost curve.
- A preview run is paid for, so it is here. A failed write to the ledger does not stop a run,
  because by then the money is spent.

## The page

At the top right, choose what to measure and over how long.

| Control | Options |
|---|---|
| **Metric** | **Cost** or **Tokens** |
| **Window** | **Past 24h**, **7 days**, **30 days** or **90 days**. The past 24 hours are drawn by the hour, the longer windows by the day. |
| Refresh | Reads the ledger again |

The left of the page is the headline figure with its call count, and a row for each model:
who ran it, its share of the cost or the tokens, and its tokens. When local models ran, the
headline splits into what was **billed** and what the local ones would have cost at cloud
prices. The right is a chart of the metric over the window, one series per model.

Below that, **Totals** gives tokens, calls, billed, local at cloud prices and unpriced tokens,
then the metric by **provider** and by **stage**. The stage says what the model was doing:

| Stage | The call was |
|---|---|
| `classify` | scoring a document against a question |
| `embed` | embedding chunks into the vector table |
| `query` | embedding a search query |

**Breakdown** is the same figures as a table, by model or by hour or day. Click a model to
see its price, its cost and tokens by stage, and its own curve.

The page reads whatever the server has recorded, so a spend from a `centinel classify` or an
`embed` at a terminal appears here too.

Next: [Agents and programs](agents.md).
