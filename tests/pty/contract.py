"""Contract-ledger screens in a real PTY, against an OFFLINE scripted provider.

This is a presentation check, not model-quality evidence: every model turn is
scripted below, while loh itself runs unmodified and really executes the reads,
patches and commands in a temporary workspace. No native window is opened.

    python3 tests/pty/contract.py --size 80x24 [--light] [--no-color] [--reject]
"""
import argparse
import json
import os
import tempfile
import time
from pathlib import Path

import pexpect
import pyte

from team_fixture import Client, Grid, Provider, call, message, stop

ROOT = Path(__file__).resolve().parents[2]
ORIGINAL = '把 JSON 存储改成 SQLite，保留 CLI 行为与现有用户数据。'
CORRECTION = '迁移要在一个事务里完成；已有的 JSON 数据要自动导入，重复启动不能重复导入。'

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, default=ROOT/'target/debug/loh')
parser.add_argument('--output', type=Path, default=ROOT/'artifacts/contract-ledger-2026-10-26')
parser.add_argument('--size', default='80x24', help='COLUMNSxROWS')
parser.add_argument('--light', action='store_true', help='answer the background query with white')
parser.add_argument('--no-color', action='store_true', help='run with NO_COLOR=1')
parser.add_argument('--reject', action='store_true', help='the verifier rejects r4')
args = parser.parse_args()
columns, rows = (int(n) for n in args.size.split('x'))
variant = '-'.join([f'{columns}x{rows}'] + (['light'] if args.light else [])
                   + (['nocolor'] if args.no_color else []) + (['reject'] if args.reject else []))
output = (args.output/variant).resolve()
output.mkdir(parents=True, exist_ok=True)


def outputs(request):
    """Function results the runtime returned to this actor so far."""
    results = []
    for item in request.get('input', []):
        if item.get('type') == 'function_call_output':
            try:
                results.append(json.loads(item['output']))
            except (TypeError, ValueError):
                results.append(item['output'])
    return results


def evidence(request):
    return [r['evidence'] for r in outputs(request) if isinstance(r, dict) and 'evidence' in r]


def patch(path, content):
    return call('apply_patch', edits=[dict(path=path, expected='missing', operation='create', content=content)])


SCRIPT = {
    'main': [
        lambda r: [message('我会把目标记成 3 条可核对的条款，先读存储接口和现有测试。'),
                   call('extract_requirements', quotes=['把 JSON 存储改成 SQLite', '保留 CLI 行为与现有用户数据'])],
        lambda r: [call('set_plan', steps=[
            dict(id='s1', description='SQLite 存储与迁移', requirements=['r2'], dependencies=[], files=[]),
            dict(id='s2', description='CLI 兼容回归', requirements=['r3'], dependencies=[], files=[]),
            dict(id='s3', description='整体验证', requirements=['r1'], dependencies=['s1', 's2'], files=[])])],
        lambda r: [call('read_file', path='src/storage.rs')],
        lambda r: [call('focus_step', step='s2')],
        lambda r: [call('check', command='sh tests/cli.sh')],
        lambda r: [call('focus_step', step='s1')],
        lambda r: [patch('src/storage/sqlite.rs', ''.join(f'// line {i}\n' for i in range(112)))],
        lambda r: [call('shell', command='sh tests/storage.sh')],
        lambda r: [message('测试发现第二次启动重复导入。接下来在事务里写迁移标记，再重跑测试。'),
                   call('shell', command='sleep 25; sh tests/storage.sh')],
        # After the correction the plan must cover the new clause.
        lambda r: [message('已按 v2 调整：先把迁移包进事务，再给 r4 补重复启动测试。'),
                   call('set_plan', steps=[
                       dict(id='s1', description='事务内迁移与迁移标记', requirements=['r2', 'r4'], dependencies=[], files=[]),
                       dict(id='s2', description='CLI 兼容回归', requirements=['r3'], dependencies=[], files=[]),
                       dict(id='s3', description='整体验证', requirements=['r1'], dependencies=['s1', 's2'], files=[])])],
        lambda r: [call('focus_step', step='s1')],
        lambda r: [patch('src/storage/migrate.rs', ''.join(f'// step {i}\n' for i in range(64)))],
        lambda r: [call('check', command='sh tests/all.sh')],
        lambda r: [message('实现完成，交给独立验收。')],
        # Only reached when the verifier rejects: keep a visible rerun going.
        lambda r: [message('收到。补跑 import_once_on_restart，通过后再请求验收。'),
                   call('shell', command='sleep 25; sh tests/all.sh')],
    ],
    'verifier': [
        lambda r: [call('read_file', path='src/storage/migrate.rs')],
        lambda r: [call('read_file', path='src/storage.rs')],
        lambda r: [call('check', command='sh tests/all.sh')],
        lambda r: [call('report_verification', summary='存储已改为 SQLite。', checks=[
            dict(requirement=f'r{i}', passed=not (args.reject and i == 4),
                 evidence=[] if args.reject and i == 4 else evidence(r)[-1:],
                 reason='引用的证据绑定旧步骤版本；迁移标记改动后没有重跑' if args.reject and i == 4
                 else '当前检查通过') for i in range(1, 5)])],
    ],
}


class Scripted(Provider):
    """Team fixture transport with this scenario's script per role."""
    def __init__(self):
        super().__init__()
        self.turns = {}
        self.errors = []
        fixture = self

        def do_POST(handler):
            request = json.loads(handler.rfile.read(int(handler.headers['Content-Length'])))
            fixture.requests.append(request)
            role = request.get('instructions', '').split('\n')[0].removeprefix('ROLE: ').strip()
            for result in outputs(request)[-1:]:
                if isinstance(result, dict) and 'error' in result:
                    fixture.errors.append((role, result['error']))
            if role == 'steering':
                items = [call('classify_input', intent='refine', reason='scripted correction',
                              updates={'r2': '把 JSON 存储改成 SQLite，迁移在单一事务内完成'},
                              additions=['已有 JSON 数据自动导入，重复启动不重复导入'], answer='')]
            else:
                script = SCRIPT.get(role, [])
                turn = fixture.turns.get(role, 0)
                fixture.turns[role] = turn + 1
                if turn < len(script):
                    items = script[turn](request)
                else:
                    items = [call('wait_input')] if role == 'main' else [message('完成')]
            body = ('data: '+json.dumps(dict(type='response.completed', response=dict(
                status='completed', model='offline-fixture', output=items,
                usage=dict(input_tokens=2400, output_tokens=300, input_tokens_details=dict(cached_tokens=2000)))))+'\n\n').encode()
            handler.send_response(200)
            handler.send_header('Content-Type', 'text/event-stream')
            handler.send_header('Content-Length', str(len(body)))
            handler.end_headers()
            try:
                handler.wfile.write(body)
            except BrokenPipeError:
                pass
        self.server.RequestHandlerClass.do_POST = do_POST


class Terminal(Client):
    """A client at the requested size that also answers the background query."""
    def __init__(self, home, cwd):
        env = dict(os.environ, TERM='xterm-256color', COLORTERM='truecolor', DEEPSEEK_API_KEY='offline-fixture-only')
        env.pop('NO_COLOR', None)
        if args.no_color:
            env['NO_COLOR'] = '1'
        self.child = pexpect.spawn(str(args.binary.resolve()), ['--home', str(home), '--cwd', str(cwd)],
                                   cwd=str(cwd), env=env, encoding='utf-8', dimensions=(rows, columns))
        self.screen = Grid(columns, rows, history=20000)
        self.stream = pyte.Stream(self.screen)
        self.chunks = []

    def drain(self, seconds=.2):
        end = time.monotonic()+seconds
        while time.monotonic() < end:
            try:
                text = self.child.read_nonblocking(65536, timeout=.03)
            except pexpect.TIMEOUT:
                continue
            except pexpect.EOF:
                return
            self.chunks.append(text)
            if '\x1b]11;?' in text:
                colour = 'ffff/ffff/ffff' if args.light else '1717/1919/1c1c'
                self.child.send(f'\x1b]11;rgb:{colour}\x07')
            if '\x1b[c' in text:
                self.child.send('\x1b[?62;22c')
            if '\x1b[6n' in text:
                self.child.send(f'\x1b[{self.screen.cursor.y+1};{self.screen.cursor.x+1}R')
            self.stream.feed(text)


provider = Scripted()
client = None
with tempfile.TemporaryDirectory(prefix='loh-contract-offline-') as temporary:
    home = Path(temporary)/'home'
    cwd = Path(temporary)/'workspace'
    home.mkdir(mode=0o700)
    (cwd/'src').mkdir(parents=True)
    (cwd/'tests').mkdir()
    (cwd/'src/storage.rs').write_text(''.join(f'// storage {i}\n' for i in range(48)))
    (cwd/'tests/cli.sh').write_text('echo "running 12 tests"\necho "test result: ok. 12 passed; 0 failed; 0 ignored"\n')
    (cwd/'tests/storage.sh').write_text('echo "test migration_is_idempotent ... FAILED"\necho "assertion failed: expected 3 rows, found 6"\nexit 101\n')
    (cwd/'tests/all.sh').write_text('echo "test result: ok. 38 passed; 0 failed; 0 ignored"\n')
    (home/'config.toml').write_text(f'base_url="http://127.0.0.1:{provider.server.server_port}"\nmax_output=1024\nuser_skills=false\nseconds=3600\n')
    try:
        client = Terminal(home, cwd)
        client.wait(lambda: 'Enter 开始' in client.text())
        client.save(output, '1-startup')
        client.send(f'/goal {ORIGINAL}\r')
        # The rerun is live: its tool line shows in the active region at any width.
        client.wait(lambda: '⋯ 运行  sleep' in client.text(), 40)
        client.drain(1.5)
        client.save(output, '3-engineering')
        client.send(CORRECTION)
        client.send('\r')
        client.wait(lambda: '契约 v2' in client.text() and '已按 v2 调整' in ''.join(client.chunks), 40)
        client.drain(.5)
        client.save(output, '4-revision')
        verdict = '验收未通过' if args.reject else '验收回执'
        client.wait(lambda: verdict in ''.join(client.chunks), 60)
        client.drain(2.5)
        client.save(output, '7-rejected' if args.reject else '6-accepted')
        client.send('/contract\r')
        client.wait(lambda: '条款' in client.text() and 'Esc 返回' in client.text())
        client.save(output, '9-contract')
        client.send('\x1b')
        client.drain(1)
        assert 'Esc 返回' not in client.text(), 'the detail page must close on Esc'
        print('OFFLINE FIXTURE ONLY — scripted model turns. Saved:', output)
        if provider.errors:
            print('runtime rejected scripted calls:', *provider.errors, sep='\n  ')
    except AssertionError:
        if client:
            client.save(output, 'failure')
        print('runtime rejected scripted calls:', *provider.errors, sep='\n  ')
        raise
    finally:
        if client:
            client.close()
        stop(home)
        provider.close()
