# Centinel — Federation Specification

**Status:** partial. **Three of twelve decisions locked** — §3 the node key, §5 the transport, §6 the slice. §1 and §2 are settled and binding. §3.4, §4 and §7–§10 are **holes**, each naming the ticket that fills it, and **nothing there is buildable yet**.

**Map:** [MAP: Centinel federation — sharing, durability and restore between city nodes](https://github.com/bennyhodl/centinel/issues/19)
**Depends on:** [`docs/SPEC.md`](SPEC.md) — every constraint there binds here and is not reopened
**Evidence:** none yet. Two research tickets outstanding — [#20](https://github.com/bennyhodl/centinel/issues/20), [#21](https://github.com/bennyhodl/centinel/issues/21). **Every locked decision here was made ahead of its verification.**
**Last updated:** 2026-08-06

---

## 1. What federation is for

Centinel runs **one device per city**. A Cleveland node, a Tampa node, hundreds more across the country, each keeping its own corpus. Two jobs are missing from that picture.

**Durability.** Ten million documents on one machine is one machine away from gone. An archive that cannot survive its own disk is not an archive.

**Cross-pollination.** A question about Florida procurement should reach Tampa, Hillsborough and Orlando without one operator having to own all three. The value of a city archive rises when it can be read beside its neighbours.

At six hundred cities the whole is **terabytes**, so pulling everything is not the mechanism. **Slices are.**

### 1.1 Not HTTP, and not `scp`

The shape is BitTorrent-like: seeding and serving, not a point-to-point copy arranged by hand. Four candidates were named at the outset — BitTorrent, Iroh, Nostr, the Lightning gossip protocol. **§5 settles it: Iroh.** The seeding *mental model* survives that decision; the BitTorrent protocol does not.

### 1.2 The two jobs are one mechanism

Durability and cross-pollination look like two features and are one transfer. **There is no blind-custodian role.** A peer that holds your corpus holds it as a foreign store it can read like any other, and durability is the *consequence* of somebody holding a copy rather than a separate protocol with its own obligations.

The cost of that, accepted: **nobody is obliged to hold anything.** A Source no other operator finds interesting is a Source with one copy. §10 has to say so out loud rather than let restore imply a guarantee that nothing in the design provides.

### 1.3 Two layers, not one

Federation is **not only a file transfer**. There is a **control plane** — signed messages between peers — sitting on top of the **data plane** that moves blobs:

> *I would like to peer* · *this is what I hold* · *send me this slice* · *I am restoring, this is me* · *I am revoking our pact*

The two need not ride the same protocol. §5 settles the data plane on **Iroh**. The control-plane **envelope** was deliberately left unspent until §6 settled what a slice is — **§6 has now settled**, so the envelope is the next thing to spend, and §6.2's two-round exchange is the first concrete message set it has to carry. A **Nostr-shaped signed event** is the leading candidate, and the reason to want one is not authentication — Iroh's connection already authenticates — but **durability and transferability**: a signed event survives being stored, forwarded, and checked later by somebody who was not in the conversation. An authenticated channel proves who you are talking to *now*, and the pact needs a claim that outlives the call.

### 1.4 Locked scope

Settled in charting. **Do not relitigate.**

| | |
|---|---|
| **Peering** | **Invite-only and pairwise.** Two operators exchange keys out of band and sign a mutual agreement. No stranger connects. Every edge is a deliberate human act, not a discovery result. |
| **Foreign corpora** | **Their own store** — read-only, never mixed into yours. Cross-store search is federation at query time. |
| **Where it runs** | Inside **`centinel serve`**: its own crate in the workspace, a flag on `serve`, **the same binary**. No second daemon and no second executable. |
| **Durability** | A consequence of a peer holding a copy. **No custodian role**, and therefore no corpus encryption. |
| **Trust** | **The peer's signature is the provenance.** It says where data came from. It does not prove the host served it. |
| **What travels** | **Blobs, derived text and the log, together.** §6 sharpened this: a slice takes whole Sources, so **the log is never filtered** — `log/<source>/` travels entire. Embeddings and the index are **optional** and the receiving operator's call. |
| **The unit** | **A slice is `(chapter, source ids)`** — whole Sources inside one chapter, indivisible. **Pull only**, on the receiver's schedule. §6. |
| **Confidentiality** | **Not a driver.** The corpus is public record; integrity, authenticity and provenance are what this design buys. |

`centinel-whisper` is a second binary for a linker reason (SPEC §3.6) that does **not** apply here. Federation has no such excuse, and taking one would be a decision this section already refuses.

---

## 2. Cross-cutting constraints

These bind every open decision below.

### 2.1 Everything in SPEC §2 still binds

Local-only inference, files as the only truth, no second runtime, provenance not optional. Federation adds a network; it removes no constraint.

### 2.2 The file tree is what makes this tractable

`blobs/` and `log/` are truth; `current/`, `centinel.db` and `vectors.lance/` are derived and rebuildable (SPEC §5). **The thing to move is a file tree**, and every blob already carries a hash that names it. A transfer verifies itself for free against `blob_sha`, and a derived layer that fails to arrive is a rebuild rather than a loss.

### 2.3 One process, one binary

The federation listener is a flag on `centinel serve`. A transport that needs its own daemon, a second runtime, or a C library with an unhappy build story fails **SPEC §2.3** before it is judged on merit.

### 2.4 Provenance now has a second axis

SPEC §2.4 requires a result to be citable to a specific document fetched at a specific time by a named tool at a named version. Federation adds: **by whom**.

A result drawn from a foreign store **must name the key that vouched for it**, and must never read as though this node witnessed it. This is the whole reason foreign corpora stay separate, and it is the constraint most likely to be quietly traded away for a convenience in §8.

### 2.5 There is a web of trust whether or not one is designed

Once a peer signs what it holds, vouches for what it sends, and identifies itself when it asks to restore, **the keys form a trust graph**. Declining to design it does not remove it; it only leaves it accidental.

Hold this distinction, because losing it grows a second destination:

| | |
|---|---|
| **A web of trust** | Deciding that **a key is who it claims to be**. **In scope** — §3. |
| **Attestation** | Proving **`tampa.gov` really served these bytes**, without relying on the peer's honesty. **Out of scope** — §13. |

**The graph exists, but §3 gave it no job.** Recovery does not run through peers, so nothing in this design fails if the trust graph is never analysed as one. That is deliberate: the version where *n*-of-*m* pacted peers vouch that a new key is Tampa is the web of trust doing real work, and it is deferred to [#32](https://github.com/bennyhodl/centinel/issues/32). Until then the graph is a description of who pacted with whom, not a mechanism.

### 2.6 Public record — an assumption, flagged

Confidentiality is not what this design buys. If that is wrong — if any part of a corpus is not public record — **§3 changes shape** and this line is where the change starts.

---

## 3. Identity and keys — **one key per node**

> **Settled** by [One key per node, backed up by the operator](https://github.com/bennyhodl/centinel/issues/29). The **pact** — what two operators sign — remains open in §3.4, owned by [Node identity and the pact](https://github.com/bennyhodl/centinel/issues/24).

**Fixed by §1.4:** invite-only, pairwise, signed by both sides, keys exchanged out of band.

### 3.1 The decision

**A node has exactly one key.** It is the ed25519 keypair §5 supplies with the transport. There is no second keypair, no recovery key, no threshold and no quorum. **The key is the identity, and the identity is the node.**

**The key derives from a seed the operator can write down.** It is not an opaque random keyfile that exists only on one disk. This is the whole backup story, and it is what makes one key sufficient rather than reckless: the operator holds a transcribable secret, off the machine, before the machine matters.

**Setup is a first-run ceremony**, and it is where the seed is made:

1. **Name** the node.
2. **Chapter** — the organisation it belongs to. Undefined as a domain term; see §6.
3. **Generate the seed**, with an option to contribute **additional entropy**.
4. The ed25519 node key derives from that seed.

### 3.2 Why

- **It is enough to peer two cities.** A pairwise pact, a signed control message, a slice request and a restore claim each need one signing key and a peer that recognises it. Nothing this spec has locked needs more.
- **Ceremony cost decides how many pacts ever exist.** One key at setup and a phrase on paper is near the floor of what a working design can cost a human.
- **A phrase is a backup an operator will actually make.** A binary keyfile invites `cp` to the same disk; a phrase invites paper, and paper survives the machine.
- **It forecloses nothing.** Threshold signatures, social recovery and revocation keys are additions *on top of* a node key. Peers verify a signature against a key they accepted, and that stays true however the signature is later produced. Deferred wholesale to [#32](https://github.com/bennyhodl/centinel/issues/32) — §13.

### 3.3 Accepted costs

**Lose the seed and lose the identity.** There is no recovery path, because a recovery path is precisely what was deferred. A node whose seed is gone cannot prove to any peer that it is Tampa — so §10 must say plainly that such a node is a **new node inheriting a corpus**, not the old node returning. That is not a gap in the design. It *is* the design, and it is only honest if an operator reads it before they need it.

**Every pact must be remade by hand.** A lost seed does not only cost the identity; it costs every pairwise agreement that named it. An operator with twenty pacts performs the out-of-band ceremony twenty times. That cost is the argument that eventually buys [#32](https://github.com/bennyhodl/centinel/issues/32).

**The backup is a human procedure, not a mechanism.** The durability story now rests on one step, performed once, by one person, at setup. Setup must make it hard to skip, and `centinel doctor` should be able to ask whether it happened.

**Rotation is answered by absence.** There is no ceremony for retiring a compromised key while keeping the identity. A compromised key means a new node identity and fresh pacts.

### 3.4 Still open — the pact

Owned by [#24](https://github.com/bennyhodl/centinel/issues/24). Where the private key lives on disk and what `doctor` reports · whether a **human-readable name** travels to a peer and what stops it being a lie · what the signed message contains · whether it is symmetric · whether it authorizes pull only, or the push described as *"I can give you whatever"* · the out-of-band ceremony, which is the entire human act · whether a pact is **record or configuration** · pact revocation and expiry, and what revocation does **not** mean about data already pulled · and what identity means when a **corpus changes hands** to a successor, which is neither a lost key nor a dead machine.

---

## 4. The peer message protocol

> **Open.** Owned by [The peer message protocol](https://github.com/bennyhodl/centinel/issues/30), blocked on §3.4 — the pact, not the key. The envelope is **deliberately unspent** until §6 settles what a slice is.

The control plane of §1.3. The pact owns the **agreement**, the transport owns the **bytes**, discovery owns **one message** — this section owns **the protocol they all speak**.

**Constrained by §5.** Messages ride the Iroh connection, and whatever envelope is adopted must sign with the **ed25519 node key**. If a Nostr-shaped envelope is taken, it must ride that key rather than bring secp256k1 with it — otherwise it reopens the key collision §5 closed.

**Open:** the message set, starting from *I would like to peer* · *this is what I hold* · *send me this slice* · *I am restoring, this is me* · *I am revoking* · what a **signed envelope** carries — sender key, timestamp, the pact it belongs to, a nonce · **replay**, because a signed *"send me this slice"* replayed a year later is still validly signed · whether there is a **session** or every message stands alone · **delivery assumptions**, given that a home machine switched off for a week is the common case and not the edge one · **versioning**, so two nodes on different Centinel versions fail comprehensibly.

**The restore case is the hard one.** A node rebuilding from nothing has no corpus, no log, and possibly no record of who its peers were. **Whatever it presents to prove it is itself is the real backup**, and it has to survive the fire that took the machine. Too weak and anybody can claim another city's corpus; too strong and a lost key is a lost archive.

---

## 5. Transport — **Iroh**

> **Decided.** [Pick the transport](https://github.com/bennyhodl/centinel/issues/23), closed 2026-08-05. Verification outstanding — see §5.4.

**Iroh carries both the connection and the bytes.** There is no separate session layer and no second protocol for control messages at the transport level.

### 5.1 Why

- **Rust-native and embeddable** in the existing tokio/axum `serve` process. SPEC §2.3 forbids a second daemon or a second language runtime, and most candidates fail that before merit is considered.
- **QUIC gives an authenticated, persistent, multiplexed connection by construction.** This is what *deleted* the session layer rather than filling it.
- **An ed25519 node identity arrives with the transport**, so §3's pact can sign with the key the connection already proves. One key, not two.
- **Content-addressed transfer** suits a store where every blob is already named by its hash, and **hole-punching with relay fallback** suits nodes on residential connections.

### 5.2 Why not the others

| | |
|---|---|
| **BitTorrent** | A torrent is a **fixed set**. A corpus that grows on every collection run would mean a new torrent forever; the DHT is public; pairwise authorization is not in the model. The seeding *mental model* survives (§1.1); the protocol does not. |
| **Lightning gossip** | Needs a channel graph to mean anything, and bulk transfer is far off its design centre. **Dropped entirely, including BOLT 8** — QUIC already provides the persistent authenticated session that was the only part worth salvaging. Layering Noise over QUIC is a second handshake on an already-authenticated channel. |
| **Nostr** | Hopeless as a data plane at hundreds of gigabytes. Its **signed-event envelope is not rejected** — it is retained, deliberately unspent, as the leading candidate for §4. |
| **IPFS** | Superseded by its own lineage: Iroh grew out of the Rust IPFS effort. |

### 5.3 Accepted costs

1. **A young ecosystem.** Federation sits on Iroh's API surface and its relay infrastructure, and churn is a real risk. The mitigation is structural rather than contractual: **the file tree stays the truth** (§2.2), so replacing the transport is a rewrite of one crate and not of the store.
2. **Relay dependence for NAT traversal.** Hole-punching fails on some networks and the fallback is infrastructure neither peer runs. Permitted — a relay carries bytes and cannot forge a signed record — but it is a dependence, and this section must eventually state what two peers can still do when every relay is gone.
3. **The decision precedes its verification.**

### 5.4 Resting on facts not yet verified

[#21](https://github.com/bennyhodl/centinel/issues/21) confirms these, and **if any is wrong this decision reopens**:

- that `iroh-blobs` verifies a **byte range**, not only a whole blob — a slice transfer that can only verify complete blobs cannot resume honestly;
- that a request for **a set of hashes** is expressible without a full manifest exchange first, which is what the whole slice model rests on;
- that it embeds with **no C toolchain** entering `cargo build`;
- that the application can **sign its own payloads with the node key**, rather than that key being sealed inside the transport — §3 depends on this;
- **whether relays store-and-forward** for a peer switched off for a week, or only carry live traffic. This single fact decides whether §4 needs a relay network behind it.

---

## 6. What a slice is — **`(chapter, source ids)`**

> **Settled** by [A slice is (chapter, source ids), and a Source is indivisible](https://github.com/bennyhodl/centinel/issues/22).

**Federation adds exactly one noun to Centinel: `chapter`.** Nothing else in the domain model moves.

### 6.1 Chapter

A **chapter** is the human-readable name of **one node's corpus**. One node, one chapter, bound to the node key of §3 — *a key has a chapter*.

Its job is **discovery**: it names *whose* corpus you are addressing, before you name anything inside it. **A chapter is not a place.** It answers *whose collection is this*, and geography never enters the record.

**On disk, a chapter is a store root.** Yours is `~/.centinel`, exactly as today. A peer's chapter is **its own root**, sibling to yours. Nothing *inside* a store changes — `log/<source>/YYYY-MM.jsonl` is untouched — and §1.4's *never mixed* becomes true **by construction** rather than by a rule somebody obeys. §8 owns where those roots sit.

### 6.2 A slice, and how one is asked for

A slice is **`(chapter, [source ids])`** — whole Sources inside one chapter. It is asked for in **two rounds**:

1. *What Sources do you have for this chapter?* → the peer answers with a **list of Sources**, a list a person can read.
2. *Send me these.* → you name the Sources you want, one or several.

**A peer inventories Sources, never Resources.** This is why discovery is bounded by construction: the unbounded-manifest problem does not arise at this layer at all. §9 owns what travels beside the list.

### 6.3 A Source is indivisible

**Whole log and whole blobs, together.** No cut by time, no cut by content kind, no filtered log. One rule, and **nothing anywhere in the model represents a partial holding.**

The reason this is the right rule rather than merely the simplest:

- **`DiscoveryRun` snapshots survive intact.** `CONTEXT.md`: *a truncated snapshot looks exactly like a source that shrank, so nothing may silently cap one.* Every finer cut considered — a month range, documents-but-not-audio — would have broken that and forced a new record type admitting it was an extract. Indivisibility buys the rule for free.
- **`ResourceStatus` liveness travels whole**, with every record attached to no single blob.
- **Restore is trivially complete** within a Source: it is there or it is not, with no third state and no reassembly of overlapping partials.

### 6.4 Pull only

You subscribe to `(chapter, source)` and **your own node re-pulls on a schedule you set**. It feels like a subscription; mechanically it is a repeated pull.

**No push** — so the pact authorizes *read access to named Sources*, not delivery. **No peer-side state** — a peer answers questions and never remembers who is subscribed or how far they got. **An offline peer is not a failure** — a home machine switched off for a week misses a cycle and catches up, which is the common case and costs nothing to handle.

### 6.5 Accepted costs

**You take the audio with the documents.** An operator who wants Tampa's procurement documents also takes every hour of meeting audio Tampa ever collected, which at a mature node may be most of the bytes. **The Source is the only knob, and it belongs to the collecting operator** — someone who wants documents shared and audio not splits them into two Sources, in config, at collection time. **A receiving operator has no filter whatsoever.** This constrains how people configure Sources long before they ever federate, and nobody will guess it, which is why it is written here.

**Staleness is bounded by your own interval and nothing else.** Durability becomes a function of a schedule the *receiving* operator sets. That is the honest place for it, and it means a peer collecting furiously between your cycles is protected only up to your cadence.

**The first pull of a mature Source is enormous and atomic in intent** — one logical transaction in the hundreds of gigabytes. Resumability is load-bearing here, not a nicety; it is fact 1 on §11's verify list.

**Blobs duplicate across chapters.** Separate roots mean a federal PDF held by three chapters is stored three times. The duplication is trivially visible — both cities compute the same `blob_sha` by construction — but deduplicating across roots would mix corpora deliberately kept apart. §8 owns the trade.

**"Everything around Florida" was answered by changing the question.** The phrase this effort opened with is served by enumerating the chapters you have pacted with and the Sources you want from each. **Florida lives in your peer list, never in the record.** That follows from invite-only peering — a question can never reach a node you have not pacted with — so it is consistent rather than a gap. But the original ask was not satisfied on its own terms, and this document should not pretend otherwise.

### 6.6 A note on `CONTEXT.md`

`chapter` is a domain noun and `CONTEXT.md` is where domain nouns are sharpened — but that file records terms *as the code makes them real*, and no federation code exists. The definition lives here until something implements it.

---

## 7. What travels

> **Partly locked.** The layers are settled; the shape is owned by [What travels, exactly](https://github.com/bennyhodl/centinel/issues/25), blocked on §6.

**Fixed — the layers:**

| Layer | Travels | Why |
|---|---|---|
| **Blobs** | **Yes** | The evidentiary layer, and the only one no peer can regenerate. Verifies on arrival against `blob_sha` for free. |
| **The log** | **Yes, entire, per Source** | Truth, not derived. The only place an address, a date, a liveness change or a discovery delta is written down. **§6 made it indivisible: `log/<source>/` travels whole and is never filtered**, so a transfer is always a real corpus and no record ever has to admit being an extract. |
| **Derived text** | **Yes** | Extraction and OCR are cheap to redo; **a council transcript is hours of Whisper** on hardware the receiver may not have. "Derived" and "cheap" are not the same word. |
| **Embeddings, index** | **Optional** | The operator's call. Vectors are portable because the embedding model is **fixed for every install** (SPEC §6.2) and chunk identity is `hash(chunk_text)` (SPEC §6.1), so a peer's vectors land in this node's space cleanly. |

**Open:** the transfer unit · whether a filtered log is the same records or a new record type that admits it is an extract — *a partial log that looks whole is a lie about completeness* · which records travel, and specifically what happens to **DiscoveryRun snapshots, whose entire value is that they are complete** and which a slice truncates by definition · whether `Derivation` and `Underivable` travel, without which the receiver re-attempts forever · whether foreign derived text stays attributed to **the tier that made it** (SPEC §2.1 — quality varies by machine) · whether a better local tier re-derives from the blob now held · what verifies a **log record**, which is a claim and not content · what happens to a blob whose bytes do not match the hash offered.

---

## 8. The foreign store and cross-store retrieval

> **Narrowed by §6.** Owned by [The foreign store](https://github.com/bennyhodl/centinel/issues/26), blocked on §7.

**Fixed:** foreign corpora are their own stores, read-only, never mixed. Search across them is federation at query time. **§2.4 is not negotiable here** — every result names its witness.

**§6 named the thing.** A foreign store **is a peer's chapter, at its own root, sibling to yours** — same layout as a native store, because it *is* a store: one chapter, one root, one corpus. *Never mixed* holds **by construction** rather than by a rule to obey, and a whole peer is one directory to delete. That closes the layout question; where the roots sit, and what search does with them, is what remains.

**Open:** where foreign roots sit, given that the store root *is* the identity of a corpus · whether a foreign store has the same layout as a native one · whether the receiver indexes foreign stores into its own `centinel.db` — cheap search that mixes the corpora in the one place they were meant to stay apart — or each foreign store carries its own index and every query fans out · what blob **deduplication** does across stores, since two cities holding the same federal PDF have the same `blob_sha` by construction · whether search spans foreign stores by default · **the handle rule**: anything Centinel prints, Centinel takes back by prefix, so a foreign hash printed beside a native one must resolve · what `list` and the run report count · what removing a foreign store does to a result cited from it last week.

---

## 9. Discovery

> **Mostly settled by §6.** What remains is owned by [Discovery: what does a peer hold](https://github.com/bennyhodl/centinel/issues/27).

The operator: *"I connected to a peer, I can see how much they have — a verification."* The constraint was: **a peer holding ten million documents cannot answer by listing ten million addresses.**

**§6 dissolved that rather than solving it.** Round one of the slice exchange *is* the advertisement — *what Sources do you have for this chapter?* — and **a peer inventories Sources, never Resources.** The answer is a list a person can read, and nothing about it scales with the size of the corpus. Pulled on demand, never pushed; a subscription is local state on the receiver.

**Open:** what travels **beside** the Source list — a count, a size, a last-collected date, a merkle root over the Source's blob set — since the list alone does not answer *"I can see how much they have"* · whether the answer is **signed**, and what that adds over the pact and an authenticated connection · whether a cycle is a **set difference against the receiver's held-ledger** (git's remote-tracking refs, which pull-only makes the natural shape) or a fresh question each time · what happens when a peer **renames or drops** a Source, now that a Source id is a wire identifier and a subscription key · whether "how much they have" is a **count you believe** or something you can check · what a node does when a peer's answer contradicts what that peer sent last month · **whether reach is simply your peer list** — and if so, saying plainly that the network's reach is a human's address book and a city with no pacts is invisible.

---

## 10. Restore

> **Open.** Owned by [Restore: rebuilding a dead node from its peers](https://github.com/bennyhodl/centinel/issues/28), blocked on §7, §9 and the prior-art research.

A city node's disk dies. This section says what brings the corpus back.

**Constrained by §3.** The first half of *"what must the operator have kept"* is already answered: **the seed**, written down at setup. Restore has no other authentication story, because §3 deferred every alternative. So this section must state the consequence in the operator's own words rather than leave it to be discovered: **a node whose seed is gone is a new node inheriting a corpus, not the old node returning** — it cannot prove to any peer that it is Tampa, and every pact it held must be made again by hand. §3.3 records this as an accepted cost; §10 is where an operator will actually read it.

**Open:** what else the operator must have kept beside the seed — a pact file, a peer list — and whether losing *those* while keeping the seed is recoverable · one peer or several, and what to do with three overlapping partial copies · whether restored data is **yours again** or stays foreign with your own key as its witness, which would make a restored node structurally different from the one that died · **how completeness is checked**, when the log is both the answer to "what did I have" and one of the things that was lost · whether restore rebuilds the derived layers.

The authentication half — *how a node with nothing proves it is itself* — belongs to §4 and is the sharpest requirement in this document.

**Constrained by §6.** Restore is *"I need Tampa and I need Hillsborough"* — **named Sources from named chapters**, the same two-round exchange as any other pull. Because a Source is indivisible, **completeness within a Source is free**: it came back whole or it did not come back. Completeness *across* Sources is the real question, and **the holes are Source-shaped** — never partial Sources, only missing ones, which is a far easier thing to report to an operator than a corpus with gaps inside it.

**Two limits this section must state rather than imply:** data collected since the last cycle is **gone**, bounded by the interval each peer chose; and a peer that subscribed to three of your Sources holds three, so restoring from subscribers reassembles a corpus **missing exactly the Sources nobody found interesting**.

---

## 11. Evidence base

**Empty, and two decisions are already resting on it.** Both §3 and §5 were settled ahead of their verification, on a triage pass made from recall. Two research tickets are outstanding, and their findings land in `docs/research/`:

- [Federated archive prior art](https://github.com/bennyhodl/centinel/issues/20) — rescoped by triage to **Syncthing, Hypercore and Radicle**. Trust model, replication unit, partial replication, infrastructure dependence, and the failure modes people actually hit. LOCKSS, IPFS, Perkeep, git-annex and BitTorrent v2 were cut, with reasons recorded on the ticket.
- [Verify Iroh](https://github.com/bennyhodl/centinel/issues/21) — no longer a survey. Six facts §5 and §3 rest on: verified **range** streaming · **set requests** without a manifest · embedding with no C toolchain · what relays do, and whether they **store-and-forward** for a peer switched off for a week · scale on a residential link · and whether the node key can be **derived from operator-supplied entropy**, which is the fact §3's entire backup story depends on. **Any of them wrong reopens the decision above it.**

The key-scheme survey was **not run**. §3 settled without it, and the field it would have compared is deferred to [#32](https://github.com/bennyhodl/centinel/issues/32).

Where the prior art contradicts §1.4, that is a finding worth more than agreement, and it comes back here as a change to this document.

---

## 12. Not yet specified

Beyond the nine open tickets, and not yet sharp enough to be one:

- **The federation surface on `centinel serve`** — how the peer listener sits beside the HTTP and MCP surfaces, and whether the `#[op]` exposure model (`mcp = false`, `local_only`) grows a third level for peers. Needs §5.
- **The config and CLI surface** — how a peer, a pact and a subscription are declared, and what the verbs are called. Names come after semantics.
- **Peer lifecycle in operation** — a peer that turns bad, deleting a foreign store, what happens to what you already pulled.
- **Politeness between peers.** Half-solved by §6.4: the puller sets its own cadence, so nobody can push at you. The other half got **worse** — a Source is indivisible, so the first pull of a mature one is hundreds of gigabytes in one logical transaction. The `.gov` crawl policy has a shape for rate limiting; whether it transfers is unexamined.
- **The subscription cycle in operation.** How often a node re-pulls, whether cadence is per peer or per Source, what a cycle does when the previous one never finished, and what the operator sees while it runs. §6.4 settles the semantics; the operational shape is fog.
- **Behaviour at six hundred nodes** — whether pairwise pacts still work at that N, or whether reach forces something transitive.
- **Re-derivation from foreign blobs**, past the part §7 owns.

---

## 13. Out of scope

| | Why |
|---|---|
| **Independent attestation of foreign observations** | Timestamping, third-party proof, re-fetch verification — proving `tampa.gov` served these bytes without relying on the peer's honesty. Valuable later, not now. **This spec must not foreclose it** (§2.4, §2.5) but does not design it. **Distinct from the web of trust over keys**, which is §3 and is in scope. |
| **Key recovery beyond a single backed-up seed** | Threshold signatures (FROST), social recovery through the pacts, offline revocation keys, W3C DIDs, key transparency. Filed as [#32](https://github.com/bennyhodl/centinel/issues/32). Every one is an **addition on top of a node key**, not a replacement, so §3 does not foreclose them: peers verify a signature against a key they accepted, however that signature was produced. Revisit when the network stops being an address book. |
| **Proof that a peer still holds your data** | LOCKSS-style polling, proof-of-retrievability, any cryptographic proof of possession. Genuinely worthwhile, and a long way off. It belongs to a network with strangers in it, not to a handful of pairwise pacts where you know exactly who you handed the bytes to. |
| **Open, permissionless peering** | Ruled out by §1.4. A later effort could add it, and §3's identity design should survive that — which is why a key scheme that *assumes* transitive trust is a poor fit and one that merely permits it is not. |
| **Merging a foreign corpus into your own truth** | Foreign stores stay separate and read-only (§1.4, §8). |
| **A blind custodian, and corpus encryption** | Ruled out by §1.2. Follows from public record (§2.6). |
| **The agent layer, and any browsing UI** | Inherited from SPEC §9. |

---

## 14. Reading this as a builder

**Do not build from this document yet.** §1 and §2 are binding constraints on the ten open tickets — they are not a design, and there is not enough here to implement against. §3 and §5 are real decisions, but both rest on facts nobody has verified (§11), so even they are not safe ground for code.

What §1 and §2 *are* good for: judging a proposal. Anything that gives a stranger a connection, that merges a foreign corpus into yours, that adds a second daemon, that assumes transitive trust, or that lets a peer's claim read as your own observation is already refused, and the refusal has a reason written beside it.

Each remaining hole closes when its ticket does. When one closes, the section is rewritten as a decision with its reasoning and its accepted cost, in the style of `docs/SPEC.md` — and the status line at the top of this file counts up.
