# The server

`centinel serve` answers the workspace page at `/web`, MCP at `/mcp`, and the ops API, all
on one port, and fires the configured [schedules](schedules.md).

```bash
centinel serve                     # in this terminal, on http://127.0.0.1:8787
centinel serve --tailscale         # and on this machine's tailnet, over HTTPS
```

## `serve` or `web`

`centinel web` is for looking. It serves the workspace and opens it, and it never starts the
scheduler, because a person opening a page to review documents did not ask for a collection
run. `centinel serve` is for a machine that works on its own: it serves the same page, plus
MCP and the ops API, and it fires the schedules. Run `serve` where the corpus lives, and
`web` wherever you want to look at it.

Everything the server is doing that runs for a while is a **job**, and the workspace lists
the active ones under **Working now** in the sidebar, with each job's step, its count and the
item in hand. Click one to follow its log: every page fetched, document read or document
scored, with failures in their own colour. A scheduled collection and a classifier run both
appear. A `centinel run` typed in another terminal is a separate process, and does not.
Jobs live in the server's memory, so a restart empties the list.

## On the tailnet

`--tailscale` keeps the server on loopback and has Tailscale proxy to it, so the workspace
and MCP are reachable from every device on your tailnet at the machine's MagicDNS name:

```
web    https://box.tailnet.ts.net/web
mcp    https://box.tailnet.ts.net/mcp
```

It takes tailnet port 443 when that is free, and 8787 when 443 already serves something
else — another program's mapping is never replaced. `--tailscale-port N` pins one port.
The mapping comes down when the server stops; one left behind by a server that was killed
is reclaimed by the next.

The machine needs Tailscale running with MagicDNS and HTTPS certificates enabled for the
tailnet. If `tailscale serve` has never run there, run it once by hand and follow the link
it prints. On Linux, `sudo tailscale set --operator=$USER` lets it configure serve without
root.

Centinel has no authentication of its own. Anyone your tailnet policy lets reach this
machine can search and read the corpus. They can also use the workspace's writes: change the
classifier questions, record reviews, restore documents to search, and **start classifier
runs, which spend your TypeSafe credit**. Those writes check only that the request came from
the page's own address, which stops another website from making them, and is not a login.
Nobody can start a collection run, which only the config file can, and nothing the workspace
writes touches the archive. Reach for tailnet access only where everyone on it should be
allowed to do those.

An agent elsewhere on the tailnet connects to the `mcp` address:

```bash
claude mcp add --transport http centinel https://box.tailnet.ts.net/mcp
```

## As a service

```bash
centinel serve start --tailscale   # install it, start it, print where it answers
centinel serve status              # running or not, the addresses, the log
centinel serve restart             # with the flags it was started with
centinel serve stop                # stop it; it no longer starts at login
```

`start` takes every flag `serve` takes and installs `serve` as a service of your login: a
launchd agent on macOS, a systemd user unit on Linux. It replaces a service already
installed, and refuses while a `centinel serve` you started by hand is serving the same
store. It returns once the server answers, with the end of the log if it does not.

The unit records the store, the binary, the flags, and the `PATH` and API keys of the
shell that ran `start` — a service inherits nothing from your shell, so change a key and
run `start` again. The unit file is readable only by you. Output goes to `serve.log` in
the store.

On Linux a user service stops at logout unless lingering is on; `status` says so, and
`sudo loginctl enable-linger "$(id -un)"` turns it on. macOS runs the agent while you are
logged in.

## The workspace against another machine

```bash
centinel web --server https://box.tailnet.ts.net
```

Opens this binary's workspace page against the corpus on that server. Nothing is read
from the local store. Both machines must run the same release.
