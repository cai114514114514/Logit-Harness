"""Actual isolated PTY layout/scrollback receipt; PNGs are colored emulator grids."""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

sys.path.insert(0, '/tmp/loh-terminal-inspection')
import pexpect
import pyte
from grid import shot

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/loh')
parser.add_argument('--output', type=Path, default=ROOT / 'artifacts/terminal-layout')
parser.add_argument('--baseline', action='store_true')
args = parser.parse_args()
out = args.output.resolve()
out.mkdir(parents=True, exist_ok=True)
requests = []
main_turn = 0


class Provider(BaseHTTPRequestHandler):
    def do_POST(self):
        global main_turn
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        requests.append(request)
        role = request.get('instructions', '').split('\n')[0]
        if role == 'ROLE: main':
            main_turn += 1
            if main_turn == 1:
                output = {'type': 'function_call', 'call_id': str(uuid.uuid4()), 'name': 'shell',
                          'arguments': json.dumps({'command': "printf 'EARLY-TOOL-PREFIX\\n'; sleep 3; i=0; while [ $i -lt 90 ]; do printf 'tool-line-%03d\\n' $i; i=$((i+1)); done; printf 'TOOL-END\\n'", 'timeout_seconds': 15})}
            else:
                output = {'type': 'function_call', 'call_id': str(uuid.uuid4()), 'name': 'ask_user',
                          'arguments': json.dumps({'question': 'Layout fixture finished. Continue?', 'requirements': ['r1']})}
        else:
            marker = f'REPLY-{len(requests):03d}'
            output = {'type': 'message', 'role': 'assistant', 'content': [
                {'type': 'output_text', 'text': f'{marker} 中文回答，已经收到。'}]}
        packets = []
        if output['type'] == 'message':
            packets.append({'type': 'response.output_text.delta', 'delta': '中文回答'})
        packets.append({'type': 'response.completed', 'response': {
            'status': 'completed', 'model': 'fixture', 'output': [output],
            'usage': {'input_tokens': 100, 'output_tokens': 20,
                      'input_tokens_details': {'cached_tokens': 0}}}})
        body = ''.join('data: ' + json.dumps(packet) + '\n\n' for packet in packets).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


class Screen(pyte.HistoryScreen):
    def set_mode(self, *modes, **kwargs):
        if kwargs.get('private') and 1049 in modes:
            self.saved_normal = copy.deepcopy((self.buffer, self.cursor, self.history))
            self.erase_in_display(2)
            self.cursor_position(1, 1)
        super().set_mode(*modes, **kwargs)

    def reset_mode(self, *modes, **kwargs):
        if kwargs.get('private') and 1049 in modes and hasattr(self, 'saved_normal'):
            self.buffer, self.cursor, self.history = self.saved_normal
        super().reset_mode(*modes, **kwargs)


class Client:
    def __init__(self, home, cwd):
        environment = dict(os.environ, TERM='xterm-256color', COLORTERM='truecolor',
                           DEEPSEEK_API_KEY='fixture-only')
        environment.pop('NO_COLOR', None)
        self.child = pexpect.spawn(str(args.binary.resolve()), ['--home', str(home)],
                                  cwd=str(cwd), env=environment, encoding='utf-8', dimensions=(32, 110))
        self.screen = Screen(110, 32, history=10000)
        self.stream = pyte.Stream(self.screen)
        self.chunks = []
        self.receipts = []

    def drain(self, seconds=.2):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            try:
                data = self.child.read_nonblocking(65536, timeout=.05)
                self.chunks.append(data)
                if '\x1b[6n' in data:
                    self.child.send(f'\x1b[{self.screen.cursor.y + 1};{self.screen.cursor.x + 1}R')
                self.stream.feed(data)
            except pexpect.TIMEOUT:
                pass
            except pexpect.EOF:
                return

    def rows(self):
        return [''.join(self.screen.buffer[y][x].data for x in range(self.screen.columns))
                for y in range(self.screen.lines)]

    def history(self):
        return [''.join(row[x].data for x in sorted(row)) for row in self.screen.history.top]

    def all_text(self):
        return '\n'.join(self.history() + self.rows())

    def wait(self, predicate, seconds=12):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            self.drain()
            if predicate():
                return
        raise AssertionError('PTY timed out:\n' + '\n'.join(self.rows()))

    def send(self, text):
        self.child.send(text)
        self.drain(.25)

    def save(self, name, conversation=True):
        shot(self.screen, name, out)
        (out / (name + '.ansi')).write_text(''.join(self.chunks))
        (out / (name + '-scrollback.txt')).write_text('\n'.join(self.history()))
        rows = self.rows()
        footer = [i + 1 for i, row in enumerate(rows) if 'KV' in row or 'Ctrl+D' in row]
        receipt = dict(name=name, rows=self.screen.lines, columns=self.screen.columns,
                       footer_rows=footer, cursor=[self.screen.cursor.x + 1, self.screen.cursor.y + 1],
                       brands=self.all_text().count('██████▀'))
        self.receipts.append(receipt)
        if conversation and not args.baseline:
            assert footer == [self.screen.lines], receipt
            assert self.screen.cursor.y in range(self.screen.lines - 6, self.screen.lines - 2), receipt
            assert receipt['brands'] == 1, receipt
            assert sum('❯' in row for row in rows[-7:]) == 1, rows


server = ThreadingHTTPServer(('127.0.0.1', 0), Provider)
threading.Thread(target=server.serve_forever, daemon=True).start()
with tempfile.TemporaryDirectory(prefix='loh-layout-') as tmp:
    home = Path(tmp) / 'home'
    home.mkdir(mode=0o700)
    cwd = Path(tmp) / 'workspace'
    cwd.mkdir()
    (home / 'config.toml').write_text(
        f'base_url="http://127.0.0.1:{server.server_port}"\nmax_output=1024\nuser_skills=false\n')
    client = Client(home, cwd)

    def states():
        try:
            with sqlite3.connect(home / 'sessions.sqlite') as connection:
                return [json.loads(data) for data, in connection.execute('SELECT state FROM sessions')]
        except sqlite3.OperationalError:
            return []

    try:
        client.wait(lambda: 'Ctrl+D' in '\n'.join(client.rows()))
        client.save('01-welcome', False)
        client.send('hello\r')
        client.wait(lambda: 'REPLY-001' in client.all_text())
        client.drain(.5)
        client.save('02-conversation')
        for index in range(2, 8):
            client.send(f'hello {index}\r')
            client.wait(lambda: f'REPLY-{index:03d}' in client.all_text())
            client.drain(.15)
        client.save('03-replies')
        # Verify real history, including blank rows; merely deduplicating markers
        # would miss the old layout's full-screen gaps between settled messages.
        history = client.all_text()
        for index in range(1, 8):
            assert history.count(f'REPLY-{index:03d}') == 1, history
        reply_rows = [i for i, row in enumerate(history.splitlines()) if 'REPLY-' in row]
        gaps = [b - a for a, b in zip(reply_rows, reply_rows[1:])]
        if not args.baseline:
            assert max(gaps) <= 10, gaps
        client.send('/')
        client.save('04-menu')
        client.send('\x1b\x15')
        client.send('\x1b[200~你好\n保留中文草稿\x1b[201~')
        client.save('05-multiline')
        client.send('\x15')
        client.send('/goal Run the isolated long command\r')
        client.wait(lambda: any(state['mode'] == 'engineering' for state in states()))
        client.wait(lambda: 'EARLY-TOOL-PREFIX' in client.all_text())
        client.save('06-quiet-tool')
        client.wait(lambda: 'Layout fixture finished' in client.all_text(), seconds=18)
        client.wait(lambda: '/answer ' in '\n'.join(client.rows()))
        question_id = next(question for state in states() for question in state.get('questions', {}))
        client.save('07-tool-complete')
        client.send('/history EARLY-TOOL-PREFIX\r')
        client.wait(lambda: '完整历史' in '\n'.join(client.rows()))
        client.send('\r')
        client.save('08-full-tool', False)
        assert 'EARLY-TOOL-PREFIX' in '\n'.join(client.rows())
        client.send('\x1b')
        client.save('09-detail-return')
        for rows, columns in [(18, 40), (50, 120), (32, 110)]:
            client.child.setwinsize(rows, columns)
            client.screen.resize(lines=rows, columns=columns)
            client.drain(.4)
            client.save(f'10-resize-{rows}-{columns}')
            if not args.baseline:
                assert sum('补充要求、纠正方向' in row for row in client.rows()) == 1, client.rows()
                assert question_id in ''.join(row.strip() for row in client.rows()), client.rows()
        client.send('/new\r')
        client.drain(.4)
        client.save('11-new-empty')
        if not args.baseline:
            assert '从一段对话开始' not in '\n'.join(client.rows())
        client.send('/plan\r')
        client.wait(lambda: any(state['mode'] == 'plan' for state in states()))
        client.drain(.4)
        client.save('11-plan')
        raw = ''.join(client.chunks)
        assert raw.count('\x1b[6n') == 1
        if not args.baseline:
            assert raw.count('██████▀') == 1, 'startup brand emitted more than once'
        doctor = subprocess.run([str(args.binary.resolve()), '--home', str(home), 'doctor', '--json'],
                                cwd=cwd, capture_output=True, text=True, check=True)
        result = dict(source_hash=json.loads(doctor.stdout)['source_hash'],
                      binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),
                      native_window_screenshot=False, actual_macos_pty=True,
                      local_requests=len(requests), paid_requests=0,
                      startup_brand_emissions=raw.count('██████▀'),
                      reply_row_gaps=gaps, captures=client.receipts)
        (out / 'receipt.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result, indent=2))
    finally:
        client.save('last-capture', False)
        if client.child.isalive():
            client.child.terminate(force=True)
        for endpoint in (home / 'run').glob('*.sock'):
            try:
                with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
                    connection.settimeout(1)
                    connection.connect(str(endpoint))
                    connection.sendall(b'{"type":"command","command":{"type":"stop"}}\n')
                    connection.recv(4096)
            except OSError:
                pass
server.shutdown()
