"""Offline team provider and isolated PTY helpers. Never starts a native window."""
import copy
import json
import os
import socket
import sqlite3
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pexpect
import pyte
from grid import shot


def call(name, **arguments):
    return dict(type="function_call", call_id=str(uuid.uuid4()), name=name,
                arguments=json.dumps(arguments))


def message(text):
    return dict(type="message", role="assistant", content=[dict(type="output_text", text=text)])


class Provider:
    def __init__(self):
        self.requests = []
        self.main_turn = 0
        fixture = self

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                fixture.requests.append(request)
                role = request.get('instructions', '').split('\n')[0]
                if role == 'ROLE: main':
                    fixture.main_turn += 1
                    turn = fixture.main_turn
                    if turn == 1:
                        output = call('set_plan', steps=[dict(id=name.lower(), description=name,
                            requirements=['r1'], dependencies=[], files=[name+'.txt']) for name in ['Ada', 'Bob']])
                    elif turn in (2, 3):
                        name = ['Ada', 'Bob'][turn-2]
                        output = call('delegate', step=name.lower(), task='FIXTURE-'+name, member=name)
                    else:
                        output = call('wait_workers')
                elif role == 'ROLE: worker':
                    # The original user task is the worker's stable fixture identity.
                    user = next((item for item in request['input'] if item.get('role') == 'user'), {})
                    name = 'Ada' if 'FIXTURE-Ada' in json.dumps(user) else 'Bob'
                    if not any(item.get('type') == 'function_call' and item.get('name') == 'read_file' for item in request['input']):
                        output = call('read_file', path=name+'.txt')
                    else:
                        output = call('wait_message', **{'from': 'main'})
                elif role == 'ROLE: steering':
                    output = call('classify_input', intent='question', updates={}, additions=[],
                                  answer='OFFLINE-DIRECTED-ANSWER 中文', reason='offline fixture', needs_tools=False)
                else:
                    output = message('OFFLINE-SIDE-ANSWER 中文')
                body = ('data: '+json.dumps(dict(type='response.completed', response=dict(
                    status='completed', model='offline-fixture', output=[output],
                    usage=dict(input_tokens=100, output_tokens=20, input_tokens_details=dict(cached_tokens=40)))))+'\n\n').encode()
                self.send_response(200)
                self.send_header('Content-Type', 'text/event-stream')
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                try:
                    self.wfile.write(body)
                except BrokenPipeError:
                    pass

            def log_message(self, *_):
                pass

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()


class Grid(pyte.HistoryScreen):
    def set_mode(self, *modes, **kwargs):
        if kwargs.get('private') and 1049 in modes:
            self.saved = copy.deepcopy((self.buffer, self.cursor, self.history))
            self.erase_in_display(2)
            self.cursor_position(1, 1)
        super().set_mode(*modes, **kwargs)

    def reset_mode(self, *modes, **kwargs):
        if kwargs.get('private') and 1049 in modes and hasattr(self, 'saved'):
            self.buffer, self.cursor, self.history = self.saved
        super().reset_mode(*modes, **kwargs)


class Client:
    def __init__(self, binary, home, cwd, extra=()):
        env = dict(os.environ, TERM='xterm-256color', COLORTERM='truecolor', DEEPSEEK_API_KEY='offline-fixture-only')
        env.pop('NO_COLOR', None)
        self.child = pexpect.spawn(str(binary), ['--home', str(home), '--cwd', str(cwd), *extra],
            cwd=str(cwd), env=env, encoding='utf-8', dimensions=(32, 110))
        self.screen = Grid(110, 32, history=20000)
        self.stream = pyte.Stream(self.screen)
        self.chunks = []

    def drain(self, seconds=.2):
        end = time.monotonic()+seconds
        while time.monotonic() < end:
            try:
                text = self.child.read_nonblocking(65536, timeout=.03)
                self.chunks.append(text)
                if '\x1b[6n' in text:
                    self.child.send(f'\x1b[{self.screen.cursor.y+1};{self.screen.cursor.x+1}R')
                self.stream.feed(text)
            except pexpect.TIMEOUT:
                pass
            except pexpect.EOF:
                return

    def text(self):
        return '\n'.join(''.join(self.screen.buffer[y][x].data for x in range(self.screen.columns)) for y in range(self.screen.lines))

    def wait(self, predicate, seconds=15):
        end = time.monotonic()+seconds
        while time.monotonic() < end:
            self.drain()
            if predicate():
                return
        raise AssertionError('PTY timeout:\n'+self.text())

    def send(self, text):
        self.child.send(text)
        self.drain(.3)

    def save(self, output, name):
        shot(self.screen, name, output)
        (output/(name+'.txt')).write_text(self.text()+'\n')
        (output/(name+'.ansi')).write_text(''.join(self.chunks))
        (output/(name+'-scrollback.txt')).write_text('\n'.join(''.join(row[x].data for x in sorted(row)) for row in self.screen.history.top))

    def close(self):
        if self.child.isalive():
            self.send('\x04')
            self.child.expect(pexpect.EOF, timeout=5)


def sessions(home):
    try:
        with sqlite3.connect(home/'sessions.sqlite') as conn:
            return [(ident, json.loads(state)) for ident, state in conn.execute('SELECT id,state FROM sessions')]
    except sqlite3.OperationalError:
        return []


def request(home, ident, message):
    endpoint = home/'run'/f'{ident}.sock'
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(3)
        connection.connect(str(endpoint))
        with connection.makefile('rb') as replies:
            connection.sendall(b'{"type":"hello","protocol":1}\n')
            assert json.loads(replies.readline())['type'] == 'hello'
            connection.sendall((json.dumps(message)+'\n').encode())
            return json.loads(replies.readline())


def stop(home):
    for ident, _ in sessions(home):
        endpoint = home/'run'/f'{ident}.sock'
        if endpoint.exists():
            assert request(home, ident, dict(type='command', command=dict(type='stop')))['type'] == 'ack'
            end = time.monotonic()+5
            while endpoint.exists() and time.monotonic() < end:
                time.sleep(.05)
            assert not endpoint.exists(), f'Own fixture daemon did not settle: {endpoint}'
