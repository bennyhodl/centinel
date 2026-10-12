# Classify

A corpus holds a lot of nothing: navigation shells, meeting calendars, staff directories,
holiday schedules. All of it is collected, extracted and indexed, and most of it will never
answer a question anybody asks. What counts as nothing depends on who is asking, so it cannot
be decided once, inside the pipeline. It is a judgment you make, against questions you can
see and change.

**Classify** is where you write those questions and run them. A model reads each document
and answers them as probabilities. You decide what each answer does: take the document out
of search, put a tag on it, or only score it.

Classifying changes **usage**, never the archive. Collected bytes and the log are not
touched, every decision can be undone, and a document a classifier excluded comes back the
moment you move a threshold or restore it.

## What leaves the machine

This is where Centinel sends corpus text to somebody else. Collection, extraction, indexing
and search run on the machine, and so does embedding, unless you have pointed it at a remote
model. See [Embeddings](../internals/embed.md).

- Text goes to **Jev**, TypeSafe's model, only when a person starts a run, and only for the
  documents that run covers. Nothing classifies in the background, and `centinel run` never
  does.
- Each request carries one complete document, or its head, middle and tail when it is longer
  than 80 kB, with the gap marked, plus the questions that apply to it.
- The server reads `TYPESAFE_API_KEY` from its environment, then from a `.env` in its working
  directory, then from a `.env` in the corpus root. The key is never sent to the browser.
- It costs money. The page prices a run before you start it.

## The questions

Every question is one of two kinds.

- **Yes or no.** One probability. At or above the threshold, its action applies.
- **Choice.** One probability for each option, adding up to one, and each option has its
  own action. A choice must have an `other` option, or a document that fits nothing is
  forced into a wrong one. Describe each option by its purpose, in the words a reader sees on
  the page, and say which one or two it is most likely confused with.

An action is **exclude** (take the document out of search), **tag** (put the answer on the
document so search can filter on it) or **keep** (score it and do nothing). When a choice has
several exclude options, their probabilities are added, so a page that is half navigation
and half calendar is still junk.

Scores from a question's **review floor** up to its threshold are not decided. They are held
for a person, and [Review](review.md) is where they go.

### Meaning and policy

A question has two halves, and they change differently.

| | What it is | Changing it |
|---|---|---|
| **Meaning** | the wording, the kind, and each option's id and description | makes a new version, and the next run asks it again |
| **Policy** | the threshold, the review floor and the actions | re-decides the stored scores, and sends nothing to the model |

That split is why you can try a threshold freely. Moving it changes what search returns
immediately, and costs nothing.

### Follow-ups

A question can be asked only of the documents another question has already placed. Its
`when` names an answer as a tag: `page_kind:record` for an option of a choice, or `spending`
for the yes of a yes-or-no question. A question with no `when` is asked of every document. A
follow-up is asked only where its parent's answer landed on that tag.

A document is never sent a question its parent did not lead to, so a follow-up costs nothing
for the documents that do not reach it. A follow-up a document was not asked is **not
asked**, which is not the same as no: it holds no score, sits in no review band, and leaves
nothing pending.

If a person or a later run moves a parent's answer, the follow-ups under the old answer go
with it, and are asked again if the document reaches them.

## The canvas

The page is one canvas. Drag to move it, scroll to zoom.

- **Source** picks which chain to look at. A chain's first question is its source, asked of
  every document. Its answers run along its foot, and each follow-up hangs below the answer
  it is asked after.
- **Add** offers the question groups Centinel ships, a blank yes-or-no question and a blank
  choice. A group you already have shows as added. A store that has never saved questions
  gets every group the first time anything reads them, and the menu offers back any you
  later remove.
- Click a question to edit it: its id, its kind, the wording, each option with its
  description and action, the threshold and the review floor. **Asked of** moves it under
  another question's answer, or back to being a source. Hover an answer and press **+** to
  ask a follow-up under it. Deleting a question moves its follow-ups up to the answer it
  followed.
- Each question has a checkbox that decides whether a run asks it.
- **Save tree** writes your edits. Saving refuses a `when` that names no answer in the set,
  and a chain that leads back to itself.

### What ships

| Group | Kind | Does |
|---|---|---|
| **Junk gate** | choice `page_kind`: `record`, `service_info`, `navigation`, `calendar_or_directory`, `unreadable`, `other` | **Excludes** at 0.90 for the junk options together, and holds 0.50 to 0.90 for review |
| **Record type** | choice | tags agenda, minutes, law text, budget document, public notice, staff report, plan or study, contract or procurement, application or form, data table |
| **Topics** | fifteen yes-or-no questions | tags what the text is about: laws, real estate, budget, public participation, transportation, and so on |
| **Who decided** | choice | tags the body whose act the text records |
| **Decision stage** | choice | tags proposed, adopted, in effect, reported or informational |
| **Who it touches** | four yes-or-no questions | tags property owners, businesses, residents, the government itself |
| **Actionability** | two yes-or-no questions | tags a deadline to act by, and an outside organization named in the text |

Only the junk gate excludes. Everything else tags, because a tag is not a usage decision.
Run the gate alone first, over every document, since its exclusions are live the moment each
answer lands. Then run the rest.

## Running it

A run asks the checked questions about a set of documents. There are four ways to start one.

**Run classifiers**, the bar at the top of the canvas, is the usual one. Choose a source or
all of them. It shows how many documents that is, about how many tokens, and what it would
cost. **Classify N documents** scores every indexed document in that scope, excluded ones
too, with no cap. If you have edited the tree, it saves it first.

**Run**, at the top right, opens a dialog for a narrower run.

| Setting | What it does |
|---|---|
| **Documents** | **A sample** of a size you choose, or **All matching** |
| **Source**, **Usage** | Narrow the documents |
| **More filters** | Text, address, or a score band of an existing question |
| **Output** | **Save a run**, or **Preview only** |
| **Model** | The Jev model. The default is the one in `[classify]` in the config |
| **In flight** | Documents out to Jev at once, from 1 to 32 |
| **Evaluation date** | Recorded with the run |

A **preview** scores the same documents and shows the results on the page, but writes
nothing: no run, no exclusions, no tags. It is how you try a reworded question on a hundred
documents before saving it. It still costs what a run costs, and it is counted in
[Spend](spend.md).

**Test a document**, also at the top right, sends one document you pick through the checked
chain as a preview. It asks only the questions on that document's path and lights the answers
Jev gave on the canvas.

The reader's **Classify** runs every saved question on one document, and **Classify this
source** on a Search page does the same for a source. See [Reading a document](read.md).

Rerunning is normal. Rewording a question, moving a threshold and retagging are all reasons to
run the corpus again, and a document is sent again whether or not it already has an answer.

### While it runs

Every answer is live the moment it is written. A document whose junk probability clears the
gate is out of search at once, every tag that clears its threshold can be filtered on, and the
review band waits for a person. The run appears under **Working now** in the sidebar, with a
log of each document as it is scored, and in [Runs](runs.md).

A request Jev refuses with a rate limit or a server error is tried again, up to four times.
A `401`, `402` or `403` is different. No key, no credit and no permission are facts about the
account, not the document, so the run **stops** and says so in Jev's own words. Everything
already answered stays. **Continue** in [Runs](runs.md) carries it on over the documents that
are still unanswered.

## At a terminal

The same runs, from a shell. They are operator commands, so they exist at a terminal and
nowhere remote.

```bash
centinel classify                                # every indexed document, every saved question
centinel classify --source agartha
centinel classify --document a5923354            # a URL, part of one, or a blob hash
centinel classify --question page_kind
centinel classify --dry-run                      # documents, tokens, estimated cost; nothing sent
centinel classify --preview                      # scores shown, nothing written
centinel classify --continue run-…               # carry on a run that stopped
centinel questions                               # the saved set, versions and policy
centinel questions --add-defaults                # append any shipped question the set lacks
```

The questions live in `workspace/questions.jsonl` and everything they produce in
`workspace/workspace.db`, which is **truth** and is not rebuilt with the index. The search
index attaches it, so deleting and rebuilding `centinel.db` loses no answer and no verdict.
See [The store](../internals/store.md).

Next: [Runs](runs.md).
