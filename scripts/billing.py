#!/usr/bin/env python3
"""Export uncertain-call evidence for billing review; never modify the ledger or call a provider."""
import argparse
import json
import re
import sqlite3
from datetime import datetime, timedelta, timezone
from pathlib import Path


def readonly(path):
    return sqlite3.connect(path.resolve(strict=True).as_uri() + "?mode=ro", uri=True)


def export(home, account):
    with readonly(home / "money.sqlite") as money:
        row = money.execute("SELECT cap,used,uncertain FROM accounts WHERE name=?", (account,)).fetchone()
        if row is None:
            raise ValueError("account does not exist; no account will be created")
        reserved = money.execute(
            "SELECT COALESCE(SUM(amount),0) FROM charges WHERE account=? AND state='reserved'", (account,)
        ).fetchone()[0]
        groups = {}
        for charge, amount, session in money.execute(
            "SELECT c.id,c.amount,s.session FROM charges c LEFT JOIN charge_sessions s USING(id) "
            "WHERE c.account=? AND c.state='uncertain' ORDER BY s.session,c.id", (account,)
        ):
            group = groups.setdefault(session, {"session": session, "charges": [], "failures": []})
            group["charges"].append({"id": charge, "reserved_nano_rmb": amount})

    with readonly(home / "sessions.sqlite") as sessions:
        for session, group in groups.items():
            if session is None:
                continue
            for sequence, stamp, raw in sessions.execute(
                "SELECT sequence,timestamp,data FROM events WHERE session=? ORDER BY sequence", (session,)
            ):
                event = json.loads(raw)
                if event["type"] != "diagnostic":
                    continue
                message = event["message"]
                status = re.search(r"provider HTTP (\d{3})\b", message)
                network = "error sending request for url (https://api.deepseek.com/responses)" in message
                if status is None and not network:
                    continue
                request = re.search(r"request_id:\s*([0-9a-f-]{36})", message)
                # Export identifiers/categories only, never request bodies or credentials.
                group["failures"].append({
                    "event_sequence": sequence,
                    "timestamp_utc": stamp,
                    "timestamp_beijing": datetime.fromisoformat(stamp).astimezone(
                        timezone(timedelta(hours=8))).isoformat(),
                    "http_status": int(status.group(1)) if status else None,
                    "category": "provider_http_error" if status else "network_send_error",
                    "provider_request_id": request.group(1) if request else None,
                })
    return {
        "created": datetime.now(timezone.utc).isoformat(),
        "account": account,
        "balance_nano_rmb": dict(zip(("cap", "used", "uncertain", "reserved"), (*row, reserved))),
        "basis": "Conservative configured-rate accounting, not an invoice.",
        "association": "Charges and failures are linked by session only. The ledger has no request-ID foreign key; no exact charge-to-request pairing is asserted.",
        "billing_evidence_received": False,
        "reconciled": False,
        "sessions": list(groups.values()),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--home", type=Path, default=Path("artifacts/campaign"))
    parser.add_argument("--account", default="weft-first-evaluation")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = export(args.home, args.account)
    # Preserve previous exports rather than replacing their audit timestamps.
    with args.output.open("x") as destination:
        json.dump(result, destination, ensure_ascii=False, indent=2)
        destination.write("\n")
    print(args.output.resolve())


if __name__ == "__main__":
    main()
