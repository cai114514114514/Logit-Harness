def solve(data):
    parts = []
    for item in data.split('/'):
        if item in ('', '.'): continue
        if item == '..':
            if parts: parts.pop()
        else: parts.append(item)
    return '/' + '/'.join(parts) if parts else ''
