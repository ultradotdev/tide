#!/usr/bin/env python3
"""Tiny stdio LSP server used by tide-lsp tests."""

import argparse
import json
import sys
import os
import time


def read_message():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        if line.lower().startswith(b"content-length:"):
            length = int(line.split(b":", 1)[1].strip())
    if length is None:
        return None
    body = sys.stdin.buffer.read(length)
    return json.loads(body)


def send(payload):
    body = json.dumps(payload).encode("utf-8")
    sys.stdout.buffer.write(b"Content-Length: " + str(len(body)).encode() + b"\r\n\r\n" + body)
    sys.stdout.buffer.flush()


def offset(text, position):
    line, character = position["line"], position["character"]
    index, length = 0, len(text)
    while line > 0 and index < length:
        char = text[index]
        index += 1
        if char == "\r":
            if index < length and text[index] == "\n":
                index += 1
            line -= 1
        elif char == "\n":
            line -= 1
    units = 0
    while index < length and units < character and text[index] not in "\r\n":
        units += 2 if ord(text[index]) > 0xFFFF else 1
        index += 1
    return index


def apply_change(text, change):
    if "range" not in change:
        return change["text"]
    start = offset(text, change["range"]["start"])
    end = offset(text, change["range"]["end"])
    return text[:start] + change["text"] + text[end:]


def sync_capability(options):
    if options.sync is None:
        return None
    if options.sync.isdigit():
        return int(options.sync)
    capability = {"openClose": True, "change": {"none": 0, "full": 1, "incremental": 2}[options.sync]}
    if options.save == "plain":
        capability["save"] = True
    elif options.save == "include-text":
        capability["save"] = {"includeText": True}
    return capability


def diagnostic(uri, message):
    return {"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"diagnostics":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"message":message}]}}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--sync")
    parser.add_argument("--save")
    parser.add_argument("--start-log")
    parser.add_argument("--stderr-lines", type=int, default=0)
    parser.add_argument("--init-delay", type=float, default=0)
    parser.add_argument("--crash-after-init", action="store_true")
    # Comma-separated optional features: rangeFormatting, declaration, typeDefinition, implementation.
    parser.add_argument("--capabilities", default="")
    options = parser.parse_args()
    if options.start_log:
        with open(options.start_log, "a") as file:
            file.write(f"{os.getpid()}\n")
    for index in range(options.stderr_lines):
        sys.stderr.write(f"mock stderr {index}\n")
    sys.stderr.flush()
    opened = set()
    documents = {}
    commands = {}
    cancelled = set()
    changes = []
    saves = []
    while True:
        message = read_message()
        if message is None:
            return
        method = message.get("method")
        if method is None and "id" in message:
            request_id = commands.pop(str(message["id"]), None)
            if request_id is not None:
                send({"jsonrpc":"2.0","id":request_id,"result":message.get("result",message.get("error"))})
        elif method == "$/cancelRequest":
            cancelled.add(message["params"]["id"])
        elif method == "workspace/executeCommand":
            command=message["params"]["command"]
            arguments=message["params"].get("arguments") or []
            log=os.environ.get("TIDE_MOCK_LSP_LOG")
            if log:
                with open(log,"a") as file: file.write(json.dumps({"command":command,"documents":documents})+"\n")
            if command == "fixture.exitParent":
                pid=os.fork()
                if pid==0: time.sleep(30);os._exit(0)
                send({"jsonrpc":"2.0","id":message["id"],"result":pid});os._exit(0)
            elif command == "fixture.order":
                uri=arguments[0]
                if documents.get(uri,{}).get("text","").startswith("fixed"):
                    send({"jsonrpc":"2.0","id":message["id"],"result":True})
                else: send({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32602,"message":"dependent command ran before edit synchronization"}})
            elif command == "fixture.cancel":
                continue
            elif command == "fixture.cancelSeen":
                send({"jsonrpc":"2.0","id":message["id"],"result":bool(cancelled)})
            elif command in ("fixture.applyNumeric","fixture.applyString"):
                server_id=message["id"] if command.endswith("Numeric") else "server-edit"
                commands[str(server_id)]=message["id"]
                send({"jsonrpc":"2.0","id":server_id,"method":"workspace/applyEdit","params":{"edit":{"changes":{"file:///tmp/mock.rs":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":2}},"newText":"ok"}]}}}})
            elif command == "fixture.floodRequests":
                # Requests whose replies cannot be written while this server sleeps.
                for index in range(arguments[0]):
                    send({"jsonrpc":"2.0","id":f"flood-{index}","method":"workspace/configuration","params":{"items":[{"section":"mock"}]}})
                time.sleep(2)
                send({"jsonrpc":"2.0","id":message["id"],"result":"flooded"})
            elif command == "fixture.floodDiagnostics":
                for index in range(arguments[0]):
                    send(diagnostic(f"file:///tmp/flood{index % 3}.rs", f"flood {index}"))
                send({"jsonrpc":"2.0","id":message["id"],"result":"flooded"})
            elif command == "fixture.stalledEdit":
                # Stop reading stdin, ask for an edit, then block on a full stdout pipe.
                time.sleep(0.5)
                send({"jsonrpc":"2.0","id":"stalled-edit","method":"workspace/applyEdit","params":{"edit":{"changes":{"file:///tmp/stress.rs":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"newText":"x"}]}}}})
                for _ in range(256):
                    send(diagnostic("file:///tmp/stress.rs", "p" * 16384))
                send({"jsonrpc":"2.0","id":message["id"],"result":"stalled"})
            elif command == "fixture.corrupt":
                sys.stdout.buffer.write(b"Content-Length: 999999999\r\n\r\n")
                sys.stdout.buffer.flush()
                time.sleep(30)
            elif command == "fixture.syncState":
                uri=arguments[0]
                send({"jsonrpc":"2.0","id":message["id"],"result":{"text":documents.get(uri,{}).get("text"),"changes":changes,"saves":saves}})
            else:
                commands["unknown-request"]=message["id"]
                send({"jsonrpc":"2.0","id":"unknown-request","method":"fixture/unknown","params":{}})
        elif method == "initialize":
            time.sleep(options.init_delay)
            capabilities = {}
            sync = sync_capability(options)
            if sync is not None:
                capabilities["textDocumentSync"] = sync
            for feature in filter(None, options.capabilities.split(",")):
                provider = "documentRangeFormattingProvider" if feature == "rangeFormatting" else feature + "Provider"
                capabilities[provider] = True
            send({"jsonrpc": "2.0", "id": message["id"], "result": {"capabilities": capabilities}})
        elif method == "initialized":
            if options.crash_after_init:
                os._exit(3)
            send({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": {
                    "uri": "file:///tmp/mock.rs",
                    "diagnostics": [{
                        "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
                        "severity": 1,
                        "message": "mock diagnostic",
                        "source": "mock",
                    }],
                },
            })
        elif method == "textDocument/didOpen":
            opened.add(message["params"]["textDocument"]["uri"])
            documents[message["params"]["textDocument"]["uri"]]=message["params"]["textDocument"]
        elif method == "textDocument/didChange":
            document=message["params"]["textDocument"].copy()
            text=documents.get(document["uri"],{}).get("text","")
            for change in message["params"]["contentChanges"]:
                changes.append(change)
                text=apply_change(text,change)
            document["text"]=text;documents[document["uri"]]=document
        elif method == "textDocument/didSave":
            saves.append(message["params"])
        elif method == "textDocument/formatting":
            format_options=message["params"]["options"]
            text=(" "*format_options["tabSize"] if format_options["insertSpaces"] else "\t")+"formatted"
            send({"jsonrpc":"2.0","id":message["id"],"result":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"newText":text}]})
        elif method == "textDocument/rangeFormatting":
            start=message["params"]["range"]["start"]
            send({"jsonrpc":"2.0","id":message["id"],"result":[{"range":{"start":start,"end":start},"newText":"ranged "}]})
        elif method in ("textDocument/declaration", "textDocument/typeDefinition", "textDocument/implementation"):
            # Each request answers with two locations named after it, so callers can tell them apart.
            line = {"textDocument/declaration": 1, "textDocument/typeDefinition": 2, "textDocument/implementation": 3}[method]
            uri = message["params"]["textDocument"]["uri"]
            send({"jsonrpc":"2.0","id":message["id"],"result":[
                {"uri": uri, "range": {"start": {"line": line, "character": 0}, "end": {"line": line, "character": 1}}},
                {"uri": uri, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}},
            ]})
        elif method == "textDocument/didClose":
            opened.discard(message["params"]["textDocument"]["uri"])
        elif method == "textDocument/completion" and message["params"]["textDocument"]["uri"] not in opened:
            send({"jsonrpc":"2.0", "id":message["id"], "error":{"code":-32602,"message":"document is not open"}})
        elif method == "textDocument/completion":
            send({"jsonrpc": "2.0", "id": message["id"], "result": {"items": [
                {"label": "mock_fn", "insertText": "mock_fn()", "kind": 3, "detail": "fn()"},
            ]}})
        elif method == "textDocument/hover":
            send({"jsonrpc": "2.0", "id": message["id"], "result": {"contents": {"kind": "plaintext", "value": "mock hover"}}})
        elif method == "textDocument/definition":
            send({"jsonrpc": "2.0", "id": message["id"], "result": [{
                "uri": "file:///tmp/mock.rs",
                "range": {"start": {"line": 2, "character": 3}, "end": {"line": 2, "character": 8}},
            }]})
        elif method == "textDocument/codeAction" and message["params"]["textDocument"]["uri"].endswith("action.rs"):
            uri=message["params"]["textDocument"]["uri"]
            send({"jsonrpc":"2.0","id":message["id"],"result":[{"title":"Fix and Run","edit":{"changes":{uri:[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":3}},"newText":"fixed"}]}},"command":{"command":"fixture.order","arguments":[uri]}}]})
        elif method == "textDocument/codeAction":
            send({"jsonrpc": "2.0", "id": message["id"], "result": [{
                "title": "Mock quick fix",
                "kind": "quickfix",
                "isPreferred": True,
                "edit": {"changes": {"file:///tmp/mock.rs": [{
                    "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 3}},
                    "newText": "pub fn main() {}"
                }]}}
            }]})
        elif method == "shutdown":
            send({"jsonrpc": "2.0", "id": message["id"], "result": None})
        elif method == "exit":
            return
        elif "id" in message:
            send({"jsonrpc": "2.0", "id": message["id"], "result": None})


if __name__ == "__main__":
    main()
