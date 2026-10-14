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
| | **Sources** | What the archive collects: investigate an address, add it, run it, see what each source holds and where it links off-site. Administrators only. [Sources](../operate/sources.md) |
| Collection | **Schedules** | Create, edit, pause and run collection schedules. [Schedules](../operate/schedules.md) |
| | **History** | Inspect collection attempts across sources and schedules, and repeat their source scope. |
| | **Ingest URLs** | Archive specific URLs under an existing source. Extract and index them afterwards from the source page. |
| Classifiers | **Classify** | Write the questions that sort the corpus, and run them. [Classify](../use/classify.md) |
| | **Runs** | What each run asked, decided and cost. [Runs](../use/runs.md) |
| | **Review** | Check the model's answers one document at a time. [Review](../use/review.md) |
| System | **Health** | Check required binaries, model servers, corpus storage and scheduler status. |
| | **Maintenance** | Verify vectors, review a repair, and re-embed missing chunks. |
| Agent | **Connect** | The command that gives Claude, Cursor or another client this corpus. [Agents and programs](../use/agents.md) |
| | **Skills** | Jobs an agent can do end to end, installed with one command. |

Opening a result takes you to the reader, which shows the document as it was collected
beside the text that was extracted from it. [Reading a document](../use/read.md)

At the foot of the sidebar, **Working now** lists whatever the server is doing, such as a
classifier run or a collection. Click one to follow its log. Below it is your account: open
it for **Spend** (every model call and what it cost, [Spend](../use/spend.md)), this
documentation, the light and dark toggle, **Admin** (who may use this archive) and signing
out.

## Administrator actions

Collection, classifier and system controls require an administrator's browser session.
Readers and MCP clients cannot start these actions. Sources also provides settings and
individual extraction, transcription, indexing and embedding stages.

Saving a schedule applies it to subsequent fires when this server runs the scheduler.
`centinel web` and `serve --no-schedule` can save schedules but leave automatic collection
disabled. Start `centinel serve` to fire them.

History reruns repeat the attempt's source scope using current defaults. They do not
replay the original flags, which older journal records do not retain. Vector maintenance
repairs derived data and preserves archived documents and source logs.

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
