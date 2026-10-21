def solve(data):
    right = {item['key']: item['value'] for item in data['right']}
    return [[item['key'], item['value'], right[item['key']]] for item in data['left'] if item['key'] in right]
