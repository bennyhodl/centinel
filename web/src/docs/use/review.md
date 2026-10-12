# Review

A model's answers are probabilities, and a threshold turns them into decisions. **Review** is
where a person checks those decisions, one document at a time. It does two things at once. It
corrects what search returns, and it measures how often the model and you agree, which tells
you which questions to reword and where to move a threshold.

## The card

Review shows one document. The left is its text. The right, headed **Model vs you**, is every
saved question with what the model called and how sure it was, and the answer you give.

- The model's call reads as an option and its probability for a choice, or a probability and
  yes or no for a yes-or-no question. A question the model is unsure about is marked **unsure**
  and shown in amber, because that is where your answer teaches the most. The first unsure
  question is selected when the card opens.
- A question the document was not asked reads **not asked**.
- Change an answer by choosing an option, or **Yes** or **No**. If you disagree with the model
  the card says so. Under each question a line says how often you have agreed with it so far.

The queue offers documents the model was unsure about first, the ones with an answer between
a question's review floor and its threshold. After those it offers a random sample of the
decided ones, so you can see how it does where it is confident. Documents you have reviewed
are skipped.

### Keys

| Key | Does |
|---|---|
| **←** | Junk: records the document as junk and moves on |
| **→** | Keep: records it as a record and moves on |
| **Enter** | Records the card as it stands, with your changes, and moves on |
| **1**–**9** | Selects that question |
| **Y**, **N** | Answers the selected yes-or-no question |

The arrow keys are offered when the saved questions include the junk gate. Clicking
**Record & next** does what Enter does.

## What a review does

A review is not only a label kept for later. It acts at once.

- An excluded page you call a record is **restored** to search.
- A page that was kept, which you call junk, is **excluded**.
- A **yes** to a question that tags puts the tag on the document, marked as a person's. A
  **no** takes the model's tag away.

Your word on a document is an **override**. It stands through every later run and every
threshold change, until a person says otherwise. The latest review of a document replaces the
one before it whole, and a question you leave alone falls back to the model.

Nothing here changes the archive. Collected bytes and the log are untouched, and every review
is one line in `workspace/workspace.db`.

## Agreement

**Agreement**, at the top right once you have reviewed anything, opens the measurement.
For each question it shows how many reviews had a model score to compare against, how often
the policy's decision matched yours, and precision and recall at the current threshold. For a
yes-or-no question it also shows the threshold that would have matched most.

Use it to decide what to change.

- **Low agreement on one question** usually means the wording or the option descriptions.
  Reword it, run a [preview](classify.md) over the documents you reviewed, and read the
  measurement again.
- **Good agreement at a different threshold** is a policy change. Move the threshold. It
  costs nothing, because it re-decides the stored scores.

Reviewed documents are the regression set for every later change, so review a spread and not
only the hard ones. `centinel evaluate` prints the same measurement, with a `--json` form an
agent can read.

Next: [Spend](spend.md).
