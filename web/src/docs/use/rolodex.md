# Rolodex

The people this corpus names **by linking to them**.

A page says who an address belongs to in one place a scan can trust: the text of the link.

```html
<a href="mailto:lviera@tampagov.net">Luis Viera</a>          <!-- a name and an address -->
<a href="/council/viera">Council Member Luis Viera</a>        <!-- a name and a page about them -->
<a href="https://x.com/LuisVieraTampa">Luis Viera</a>          <!-- a name and an account -->
```

Each of those is a **mention**. Names in running text — minutes, a PDF, a transcript — need a
model to find, and are not read yet.

```bash
centinel rolodex                     # every name, most-named first
centinel rolodex --source tampa      # the names one source's pages link
centinel rolodex --name viera        # names holding every word given
centinel rolodex show "Luis Viera"   # one name: addresses, accounts, pages about them, every source
```

## Every source a name comes up in

`show` lists every source the name appears in, two ways:

```console
$ centinel rolodex show "Luis Viera"

Luis Viera  1 page in 1 source

  pages about them
    https://www.tampa.gov/city-council/about-us/luis-viera  1 page

  sources
    tampa  linked from 1 page · named in 1 document
      https://www.tampa.gov/city-council
```

- **linked from** — pages whose links name them: the Rolodex's own evidence, listed under
  the source.
- **named in** — indexed documents whose text holds the name **word for word**, under the
  name asked for and every spelling the links used. Minutes, PDFs and transcripts count.
  Every one, not a ranked top ten: this is not `search`, which ranks passages and matches
  each word anywhere in a chunk.

A name no link carries still has its sources: `show "Jane Castor"` answers with the
documents that write it. The match is exact, so `Pam Iorio` and a roster's `Iorio Pam` are
two questions. Without an index, document text is not searched, and the answer says so
rather than reading as nowhere.

On the web, **Rolodex** in the sidebar is the same list: search a name, narrow it to one
source, and open a name for its page — every source it comes up in, its addresses, accounts
and pages about them, and a graph of the person, the sources around them and what their links
point at. Any approved account can read both. A name no link carries has a page too: search
it, and the empty list offers to look for it in document text.

---

## Where mentions come from

`collect` already walks every link on every page it stores, to write down the
[crumbs](../operate/crumbs.md). The same walk records the mentions, on the same ledger row:

- a `mailto:` link, and Cloudflare's protected form of one — a site behind Cloudflare
  serves no plain `mailto:` at all;
- a link to a profile on a social platform — `x.com/<handle>`, `facebook.com/<page>`,
  `linkedin.com/in/<name>` and a dozen more. A share button or a single post is not a
  profile;
- any other link whose text is a person's name — a bio page, a staff directory entry.

**What counts as a name is deliberately narrow:** two to four capitalised words, no digits,
no word that names a place, a body or a page, **and one word a given name** — one the Social
Security Administration recorded for at least a thousand births since 1880. A title in front
is dropped, so `Council Member Luis Viera` and `Luis Viera` are one entry, and a roster
written surname first (`Iorio Pam`) still counts. A false yes is an entry for "Billing
Questions"; a false no is a link not kept. The second costs less.

The given name is what does the work. A site's menu is title-cased phrases too: on four
tampa.gov pages the shape alone admitted 98 entries and 45 were navigation. With the given
name, 50 entries, every one a person. The list is `crates/centinel-core/data/given-names.txt`,
and its header says how it was made. A name rare in the US — `Duff` — is not in it, and a link
naming only such a person is not kept.

An address or a handle whose link text is not a name — "Email", "Facebook" — is kept, under
no name, and counted at the foot of the list.

## A name, not a person

An entry is **a name as written**. Two Jane Does in two cities are one entry, and
`Jane A. Doe` is another. Merging people on anything fuzzier than the spelling is identity
resolution, and a wrong merge corrupts the record quietly. Every entry carries the sources
and pages that named it, so you can tell two people apart; the store never claims they are
one.

## A corpus collected before the Rolodex

Mentions are derived, like crumbs: they are read out of the page, and the page is in the
pool. A ledger row written before mentions were read has none, and the list says how many
pages that is rather than reading as a corpus of nobody:

```console
  ⚠ 236 pages were recorded before names were read, so nobody they link is here —
    `centinel crumbs --rescan` reads them once
```

`centinel crumbs --rescan`, or **Record its crumbs** on a source's page, reads them.
