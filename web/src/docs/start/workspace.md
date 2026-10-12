# The workspace

The workspace is Centinel in a browser. It is where you search the corpus, read what was
collected, decide what is noise, check the model's work, and see what it all cost. Most
people never need more than this and the install.

```bash
centinel web
```

That serves the page at `http://127.0.0.1:8787/web` and opens it. Nothing else starts: no
scheduler, no collection. Opening the workspace to look at a corpus is not a request to
change it.

## What is here

The sidebar groups the pages. It opens as a rail of icons; the toggle at the top expands it,
and each entry carries a count once it does.

| Group | Page | What it is for |
|---|---|---|
| Archive | **Search** | Find documents by their words, narrowed by source, usage, classifier score and address. [Searching](../use/search.md) |
| | **Spend** | Every model call and what it cost. [Spend](../use/spend.md) |
| Classifiers | **Classify** | Write the questions that sort the corpus, and run them. [Classify](../use/classify.md) |
| | **Runs** | What each run asked, decided and cost. [Runs](../use/runs.md) |
| | **Review** | Check the model's answers one document at a time. [Review](../use/review.md) |
| Agent | **Connect** | The command that gives Claude, Cursor or another client this corpus. [Agents and programs](../use/agents.md) |
| | **Skills** | Jobs an agent can do end to end, installed with one command. |

Opening a result takes you to the reader, which shows the document as it was collected
beside the text that was extracted from it. [Reading a document](../use/read.md)

At the foot of the sidebar, **Working now** lists whatever the server is doing, such as a
classifier run or a scheduled collection. Click one to follow its log. Below it are this
documentation and the light and dark toggle.

## What it cannot do

The workspace reads the corpus and records decisions about it. It does not collect.

- You add a source, run collection, set a schedule, pull model weights and check a host
  from the command line and the config file. That is the [Operator](operator.md) job, and it
  is the one part of Centinel that is deliberately not a button.
- It does not change the archive. Collected bytes and the log are never written from the
  page. Everything the workspace records (questions, answers, reviews, a person's overrides)
  lives in `workspace/`, and every decision it makes can be undone.

The split is on purpose. What gets collected should not depend on whoever happens to have a
page open. See [The shape](../internals/shape.md).

## Where it runs

`centinel web` opens a server of its own on this machine. If a `centinel serve` is already
answering on that address, it reuses that server, but only when it is the same build of the
same version on the same corpus. Anything else is refused by name, so the page you opened is
never quietly talking to an older process. A banner at the top of the page says so if the
page and the server disagree about the version: stop the old one, start it again, reload.

`centinel serve` serves the same page at `/web`, together with MCP and the schedules, so a
corpus that collects on a timer also has a workspace. See [The server](../operate/serve.md).

To use this machine's page on a corpus that lives elsewhere:

```bash
centinel web --server https://box.tailnet.ts.net
```

No local store is opened. Both machines must run the same release.

## Next

[Your first corpus](first-corpus.md) walks from nothing to a corpus you can search in the
workspace.
