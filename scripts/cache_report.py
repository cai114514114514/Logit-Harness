#!/usr/bin/env python3
"""Read provider-reported cache usage and request-prefix churn without making API calls."""
import argparse
from collections import Counter, defaultdict
from datetime import datetime, timezone
import json
from pathlib import Path
import sqlite3


def totals():
    return {"calls": 0, "input": 0, "cached": 0, "output": 0, "zero_hit_calls": 0}


def add(total, event):
    if not 0 <= event["cached"] <= event["input"]:
        raise ValueError("invalid provider cache usage")
    total["calls"] += 1
    for key in ("input", "cached", "output"):
        total[key] += event[key]
    total["zero_hit_calls"] += int(event["cached"] == 0)


def group(session):
    experiment = session.get("evaluation")
    if experiment:
        return Path(experiment["directory"]).parent.parent.name
    return "engineering"


def report(database, session_id=None, requests=False):
    # Read-only mode refuses to create a misspelled database or modify a campaign.
    connection = sqlite3.connect(database.resolve().as_uri() + "?mode=ro", uri=True)
    with connection:
        states = {key: json.loads(raw) for key, raw in connection.execute("SELECT id,state FROM sessions")}
        usage = defaultdict(totals)
        churn = defaultdict(Counter)
        previous = {}
        roles = {}
        last_sequence = 0
        for sequence, session, raw in connection.execute("SELECT sequence,session,data FROM events ORDER BY sequence"):
            if session_id and session != session_id:
                continue
            last_sequence = sequence
            event = json.loads(raw)
            actor = event.get("actor", "")
            if event["type"] == "model_requested" and requests:
                path = Path(event["artifact"])
                if not path.is_file():
                    churn["missing"]["request_artifacts"] += 1
                    continue
                request = json.loads(path.read_text())
                label = request["instructions"].splitlines()[0]
                role = label.removeprefix("ROLE: ")
                roles[session, actor] = role
                key = (session, actor, role)
                before = previous.get(key)
                if before:
                    counts = churn[role]
                    counts["transitions"] += 1
                    changed = before["instructions"] != request["instructions"]
                    counts["instructions_changed"] += int(changed)
                    counts["tools_changed"] += int(before["tools"] != request["tools"])
                    prefix = request["input"][:len(before["input"])] == before["input"]
                    counts["input_prefix_changed"] += int(not prefix)
                    if changed:
                        pairs = zip(before["instructions"].splitlines(), request["instructions"].splitlines())
                        first = next((new.split(":", 1)[0] for old, new in pairs if old != new), "length")
                        counts[f"first_changed_line:{first}"] += 1
                previous[key] = request
            if event["type"] != "usage":
                continue
            # Compact requests have separate cache boundaries; actor identity alone
            # cannot reliably identify their role without the request artifact.
            keys = ["all", f"group:{group(states[session])}"]
            if (session, actor) in roles:
                keys.append(f"role:{roles[session, actor]}")
            for key in keys:
                add(usage[key], event)
    connection.close()
    if session_id and session_id not in states:
        raise ValueError(f"unknown session: {session_id}")
    for total in usage.values():
        total["missed"] = total["input"] - total["cached"]
        total["hit_ratio"] = total["cached"] / total["input"] if total["input"] else None
    return {
        "created": datetime.now(timezone.utc).isoformat(),
        "database": str(database.resolve()),
        "session": session_id,
        "last_event_sequence": last_sequence,
        "definition": "sum(reported cached input) / sum(reported input); unknown usage excluded",
        "usage": dict(sorted(usage.items())),
        "prefix_changes": {key: dict(value) for key, value in sorted(churn.items())},
        "limitation": "Request equality is cache eligibility evidence, not a measured provider cache hit or latency gain.",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", type=Path, required=True)
    parser.add_argument("--session")
    parser.add_argument("--requests", action="store_true", help="also inspect local request artifacts")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = json.dumps(report(args.database, args.session, args.requests), ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(result)
    else:
        print(result, end="")


if __name__ == "__main__":
    main()
