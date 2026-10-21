#!/usr/bin/env python3
"""Read saved Logit- Harness events without launching a daemon or calling a model."""
import argparse
import json
import time
from datetime import datetime
from pathlib import Path


def describe(event):
    kind = event["type"]
    if kind == "status":
        return f"{event['status']}: {event['reason']}"
    if kind == "user":
        return f"user: {event['message']}"
    if kind == "revised":
        return f"revision {event['revision']}: {', '.join(event['changed'])}"
    if kind == "message":
        return f"{event['actor']}: {event['text']}"
    if kind == "action_started":
        return f"start {event['tool']} [{event['id']}]"
    if kind == "action_finished":
        return f"finish [{event['id']}] success={event['success']}"
    if kind == "process_started":
        return f"process [{event['id']}] pid={event['pid']}"
    if kind == "diagnostic":
        return event["message"]
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("events", type=Path)
    parser.add_argument("--after", type=int, default=0, help="skip sequence numbers through this value")
    parser.add_argument("--limit", type=int, help="maximum displayed events")
    parser.add_argument("--speed", type=float, default=0, help="0 prints immediately; 1 uses recorded timing")
    parser.add_argument("--json", action="store_true", help="include every event verbatim")
    args = parser.parse_args()
    if args.speed < 0 or (args.limit is not None and args.limit < 0):
        parser.error("speed and limit must be nonnegative")
    events = json.loads(args.events.read_text())
    previous = None
    displayed = 0
    for event in events:
        if event["sequence"] <= args.after:
            continue
        text = json.dumps(event, ensure_ascii=False) if args.json else describe(event)
        if text is None:
            continue
        if args.limit is not None and displayed >= args.limit:
            break
        stamp = datetime.fromisoformat(event["timestamp"])
        if args.speed and previous is not None:
            time.sleep(max(0, (stamp - previous).total_seconds()) / args.speed)
        previous = stamp
        line = text if args.json else f"{event['sequence']} {event['timestamp']}  {text}"
        print(line, flush=True)
        displayed += 1


if __name__ == "__main__":
    main()
