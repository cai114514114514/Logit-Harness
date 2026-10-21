def solve(data):
    return [min(data['cap'], data['base'] * 2 ** i) for i in range(max(1, data['attempts']))]
