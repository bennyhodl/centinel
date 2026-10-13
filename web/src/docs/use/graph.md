# Graph

**Graph** draws the corpus as a map: every source, every host its pages link to, and a line
between them as thick as the log of the pages that carry the link. It is the start of an
ontology of what a government points at — the records portal, the permitting vendor, the
county it shares a clerk with — read off the pages themselves rather than written down by
hand.

The page is built from the same crumb ledgers the [Crumbs](../operate/crumbs.md) command
reads. A source collected before the ledger existed has no edges until its crumbs are
recorded; the source's own page has the button.

## What you are looking at

- **A filled dot is a source.** It is sized by how many addresses its log knows.
- **An outlined dot is a host** nothing collects: sized by how many pages link to it, over
  every source. A greyed dot is a host you refused.
- **A line is pages, not links.** A footer puts the same host on every page of a site, so
  one city is 198,470 links to its utility portal on 11,442 pages. The page count is what
  says how much of the site depends on the host, and it is what the line is drawn from.
- **A host a source collects is not a dot of its own.** The line lands on that source, so
  two sources that link each other are joined rather than each pointing at a copy of the
  other's name.

The layout settles on its own for a few seconds after each read; drag to move and scroll to
zoom.

## Reading it

Hover a line and it says how many pages and links it stands for, with both ends named.
Hover a dot for its icon, name, kind and counts. Everything else fades while you do, so a
busy host reads as its own neighbourhood.

Click a dot and its card opens. For a host: which sources link there and how often, where on
the host the links point (the paths, by how many pages chose each), and a sample of the
referring pages, each one opening the page as it was collected. For a source: the hosts it
links to and the sources that link to it, and a link to its own page.

**Find** a source or host by name and the view moves to it.

## The filter

**At least N pages** hides every line carried by fewer pages, and with it every host no
line reaches. Five is the default: it keeps the systems and drops the one-page mentions,
which on a city of sixteen sources is the difference between six hundred edges and three
and a half thousand. **Hide refused** takes the greyed hosts out of the picture.

The title line says what the filter leaves in and what the whole corpus holds — `461 of
2,847 hosts` — so a quiet view is never mistaken for a quiet corpus. Past four thousand
lines the strongest are kept and the page says so; raise the floor to see the rest.

## Deciding

A host's card carries the same two decisions a crumb has everywhere else:

- **Investigate** fetches the host's front page and reports what collection would make of
  it, as the [Sources](../operate/sources.md) page does for any address.
- **Refuse** writes the corpus-wide ruling `centinel crumbs ignore` writes: the host is
  greyed here, hidden from the crumbs list, and never offered again. **Allow** takes it
  back. The ruling is one file, so the graph, the crumbs list and the command never
  disagree.

Neither happens on its own. Every host you promote becomes a source that drops its own
crumbs, and every one you refuse is what stops the walk.
