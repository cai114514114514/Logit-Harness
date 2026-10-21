import json
import sys
from core import solve

for line in sys.stdin:
    request = json.loads(line)
    answer = {"name": 'hold-brackets', "version": 1} if request.get("op") == "about" else solve(request["data"])
    print(json.dumps(answer, ensure_ascii=False))
