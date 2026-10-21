def solve(data):
    cache, output = {}, []
    for op in data['ops']:
        if op[0] == 'set':
            cache[op[1]] = (op[2], op[3])
        else:
            value, time = cache[op[1]]
            output.append(value if op[2] - time <= data['ttl'] else None)
    return output
