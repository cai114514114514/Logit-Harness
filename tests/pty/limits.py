"""Offline PTY check of unlimited task limits; no paid API or native window."""
import argparse
import hashlib
import json
import tempfile
from pathlib import Path

from team_fixture import Client, Provider, sessions, stop

parser = argparse.ArgumentParser()
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
output = args.output.resolve()
output.mkdir(parents=True, exist_ok=True)
provider = Provider()
with tempfile.TemporaryDirectory(prefix='loh-limits-') as directory:
    home = Path(directory)/'home'
    cwd = Path(directory)/'workspace'
    home.mkdir(mode=0o700)
    cwd.mkdir()
    (home/'config.toml').write_text(f'base_url="http://127.0.0.1:{provider.server.server_port}"\ntokens=100\nseconds=2\nuser_skills=false\n')
    client = Client(args.binary.resolve(), home, cwd)
    try:
        client.wait(lambda: 'Ctrl-D' in client.text())
        client.send('/plan\r')
        client.wait(lambda: len(sessions(home)) == 1)
        client.wait(lambda: '只读' in client.text())
        client.send('/budget unlimited\r')
        client.wait(lambda: sessions(home)[0][1]['budget']['limit'] == 2**64-1)
        state = sessions(home)[0][1]
        assert state['seconds'] == 2**64-1
        assert state['campaign'] is None
        assert state['status'] == 'waiting', 'changing limits must not automatically resume work'
        client.send('/budget\r')
        client.wait(lambda: 'unlimited' in client.text())
        client.save(output, 'current-budget')
        assert '18446744073709551615' not in client.text()
        for key in ['tokens', 'seconds']:
            client.send(f'/config {key} unlimited\r')
            client.wait(lambda: f'{key} = "unlimited"' in (home/'config.toml').read_text())
        client.send('/new\r')
        client.wait(lambda: '原任务继续后台' in ''.join(client.chunks))
        client.send('/plan\r')
        client.wait(lambda: len(sessions(home)) == 2)
        assert all(s['seconds'] == 2**64-1 and s['budget']['limit'] == 2**64-1 for _, s in sessions(home))
        assert not provider.requests, 'local budget/default operations must not call the model'
        result = dict(passed=True, binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),
                      actual_macos_pty=True, native_windows=False, provider_requests=0,
                      current_limits_updated=True, new_defaults_unlimited=True,
                      existing_work_not_resumed=True, money_configuration_unchanged=True)
        (output/'receipt.json').write_text(json.dumps(result, indent=2)+'\n')
        print(json.dumps(result, indent=2))
    finally:
        client.close()
        stop(home)
        provider.close()
