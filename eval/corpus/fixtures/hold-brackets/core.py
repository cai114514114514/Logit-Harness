def solve(data):
    depth, quoted, escaped = 0, False, False
    for ch in data:
        if quoted:
            if escaped: escaped = False
            elif ch == '\\': escaped = True
            elif ch == '"': quoted = False
        elif ch == '"': quoted = True
        elif ch in '([{': depth += 1
        elif ch in ')]}':
            depth -= 1
            if depth < 0: return False
    return depth == 0 and not quoted
