# Runs

Every run of the [classifiers](classify.md) is kept. A run asks Jev your questions about a
set of documents, and **Runs** is its ledger: what each run asked, how it went, what it
decided, and what it cost. What a run decides is already live in search as each answer
lands, so this page is for watching it and for understanding what happened.

## The ledger

The top of the page counts what was spent this week, and how many runs are scoring now. Below
it, newest first, each run is one row: when it started and how long it took, what it asked
and of how many documents, and how it ended.

A run is in one of these states.

| State | Meaning |
|---|---|
| **Scoring now** | Documents are out to Jev. |
| **Finished** | Every document was answered, or recorded with the reason it was not. |
| **Stopped** | Jev refused with a `401`, `402` or `403`: no key, no credit, or no permission. The row carries Jev's own words. Everything already answered stays. |
| **Interrupted** | The server stopped while the run was going. Its answers so far are kept. |
| **Failed** | The run could not start or finish, and says why. |

A **preview** is not kept here. It shows in the dialog that started it, and it appears in
[Spend](spend.md) because it was paid for.

Click a row to open the run.

## One run

A run that is going shows live: how far it has got, how long it has taken and an estimate of
what is left, the documents out to Jev with how long each has waited and which request of its
chain it is on, and the latest answers as they land. It is the same run you can follow under
**Working now** in the sidebar.

When it stops, the same card gives the totals, and a run that failed shows its errors grouped
by what Jev said.

The totals count documents by what the policy decided about them.

| Outcome | Means |
|---|---|
| **Exclude** | Out of search, because a question's exclude action cleared its threshold. |
| **Review** | Held for a person: some answer fell between a question's review floor and its threshold. |
| **Tagged** | Kept, with at least one tag from a question that tags. |
| **Keep** | Scored, and nothing applied. |
| **Not asked** | No question of this run reached it. A follow-up whose parent did not lead here, for instance. |
| **Error** | Jev could not answer. The result keeps Jev's own error text. |

Click a total to filter the results to it. The results table is sorted and filtered on the
server, so a run over the whole corpus never goes to the browser at once. Click a score column
to sort by it, high to low first. Each document links to the [reader](read.md).

## What a run records

Each run keeps the exact questions it asked, with their versions, the scope or the documents
it covered, the model, the evaluation date, its settings, tokens, duration, throughput,
errors, and the cost when the model has a price. The results are the policy's reading of the
stored answers, so moving a threshold changes what a finished run reports without sending
anything again.

## Carrying on and repeating

- **Continue** is offered on a run that **stopped** or was **interrupted**. It keeps the same
  scope, questions and settings, and asks only the documents no run in the chain has
  answered. Fix the account first if Jev refused it.
- **Run again** repeats a run: the same scope, or the same documents when it was a sample,
  with the same questions and model. Use it after changing a question, or on a corpus that
  has grown.

A run you start from the [Classify](classify.md) page opens here at once, showing the run as
it starts.

Next: [Review](review.md).
