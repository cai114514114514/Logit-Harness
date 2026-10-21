"""A real line-delimited MCP stdio fixture, implementing the current protocol."""
import json
import sys
import time
from pathlib import Path

for line in sys.stdin:
    request = json.loads(line)
    if "id" not in request:
        continue
    method = request["method"]
    if method == "server/discover":
        result = {
            "resultType": "complete",
            "supportedVersions": ["2026-07-28"],
            "capabilities": {"tools": {}},
            "ttlMs": 0,
            "cacheScope": "private",
        }
    elif method == "tools/list":
        result = {"tools": [{"name": "echo", "description": "Echo actual input", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}}]}
    elif method == "tools/call":
        if request["params"]["arguments"]["text"] == "slow":
            Path("started").write_text("running")
            time.sleep(10)
            Path("late").write_text("obsolete")
        result = {"resultType": "complete", "content": [{"type": "text", "text": request["params"]["arguments"]["text"]}], "isError": False}
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "error": {"code": -32601, "message": "Unknown method"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
