#!/usr/bin/env python3
"""A zeropty plugin in Python, standard library only: the whole protocol in one file (docs/PLUGINS.md).

It adds the command `hello [name]`, which lists the sessions (asking zeropty for them) and the ones
created and ended since it attached, which it learns from events.

    python3 examples/plugins/hello.py &        # ZEROPTY_SOCK picks the server, as for the CLI
    zeropty hello you                          # -> hello, you! sessions: ...
    zeropty list-plugins                       # -> hello: running (api 1) ...
"""
import json
import os
import socket
import sys

path = os.environ.get("ZEROPTY_SOCK") or os.path.expanduser("~/.zeropty/mux.sock")
conn = socket.socket(socket.AF_UNIX)
conn.connect(path)
lines = conn.makefile("rb")


def send(msg):
    conn.sendall((json.dumps(msg) + "\n").encode())


def receive():
    line = lines.readline()
    if not line:
        sys.exit(0)  # zeropty went away (kill-server, or the plugin was removed)
    return json.loads(line)


# 1. Attach: a name, the events wanted, the commands added.
send({"argv": ["plugin-attach", "-n", "hello", "-e", "session-created,session-exited", "-c", "hello"], "cwd": "/"})
attached = receive()
if attached["code"] != 0:
    sys.exit(attached["err"])

created, exited = [], []
backlog = []  # events and calls that came while waiting for a reply of ours
next_id = 1


def command(argv):
    """Runs a zeropty command; its reply comes back with our id."""
    global next_id
    my_id, next_id = next_id, next_id + 1
    send({"id": my_id, "argv": argv, "cwd": "/"})
    while True:
        msg = receive()
        if msg.get("id") == my_id:
            return msg
        backlog.append(msg)


def handle(msg):
    if "event" in msg:  # 2. Events: {"event": "session-created", "session": "w"}
        (created if msg["event"] == "session-created" else exited).append(msg["session"])
    elif "call" in msg:  # 3. Calls of our command: answered with the same number
        name = msg["argv"][1] if len(msg["argv"]) > 1 else "there"
        sessions = command(["list-sessions", "-F", "#{session_name}"])["out"].split()
        out = f"hello, {name}! sessions: {' '.join(sessions)}\n"
        out += f"created since attach: {' '.join(created) or '-'}\nended since attach: {' '.join(exited) or '-'}\n"
        send({"reply": msg["call"], "code": 0, "out": out, "err": ""})


while True:
    handle(backlog.pop(0) if backlog else receive())
