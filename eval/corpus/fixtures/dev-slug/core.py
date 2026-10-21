import re

def solve(data):
    return re.sub(r'[^a-z0-9]', '_', data.lower()).strip('_')
