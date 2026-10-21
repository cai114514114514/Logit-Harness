"""Owned descendant used to test TERM/KILL settlement, not elapsed-time guesses."""
import os
from pathlib import Path
import signal
import time

child = os.fork()
if child == 0:
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    Path("descendant.pid").write_text(str(os.getpid()))
    while True:
        time.sleep(0.1)
else:
    while not Path("descendant.pid").exists():
        time.sleep(0.01)
    while not Path("leader-exit").exists():
        time.sleep(0.01)
