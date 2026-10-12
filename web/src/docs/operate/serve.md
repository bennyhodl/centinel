# The server

`centinel serve` answers the workspace page at `/web`, MCP at `/mcp`, and the ops API, all
on one port, and fires the configured [schedules](schedules.md).

```bash
centinel serve                     # in this terminal, on http://127.0.0.1:8787
centinel serve --tailscale         # and on this machine's tailnet, over HTTPS
```

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
machine can search and read the corpus and record classifier reviews; nobody can start a
collection run, which only the config file can.

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
