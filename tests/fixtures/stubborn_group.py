"""Owned descendant used to test TERM/KILL settlement, not elapsed-time guesses."""
import os
from pathlib import Path
import signal
import time

child = os.fork()
if child == 0:
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    # File existence is the readiness signal used by the parent/tests. Publish
    # only after the PID is complete, not between open(O_CREAT) and write().
    Path("descendant.pid.tmp").write_text(str(os.getpid()))
    os.replace("descendant.pid.tmp", "descendant.pid")
    while True:
        time.sleep(0.1)
else:
    while not Path("descendant.pid").exists():
        time.sleep(0.01)
    while not Path("leader-exit").exists():
        time.sleep(0.01)
