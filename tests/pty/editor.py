"""Exercise the real editor without a model, credentials, or user sessions."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

import pexpect
import pyte
from grid import shot

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/loh')
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)

with tempfile.TemporaryDirectory(prefix='loh-editor-') as directory:
    env = dict(os.environ, TERM='xterm-256color')
    env.pop('DEEPSEEK_API_KEY', None)
    child = pexpect.spawn(str(args.binary.resolve()), ['--home', directory],
                          cwd=directory, env=env, encoding='utf-8', dimensions=(22, 40))
    screen = pyte.HistoryScreen(40, 22, history=2000)
    stream = pyte.Stream(screen)
    chunks = []

    def rows():
        # pyte's display helper indexes empty continuation cells for wide emoji.
        return [''.join(screen.buffer[y][x].data for x in range(screen.columns))
                for y in range(screen.lines)]

    def text():
        return '\n'.join(rows())

    def drain(seconds=.12):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            try:
                data = child.read_nonblocking(65536, timeout=.02)
                chunks.append(data)
                if '\x1b[6n' in data:
                    child.send(f'\x1b[{screen.cursor.y + 1};{screen.cursor.x + 1}R')
                stream.feed(data)
            except pexpect.TIMEOUT:
                pass
            except pexpect.EOF:
                return

    def wait(needle):
        end = time.monotonic() + 8
        while time.monotonic() < end:
            drain()
            if needle in text():
                return
        raise AssertionError(f'Editor did not show {needle!r}:\n{text()}')

    def send(value):
        child.send(value)
        drain()

    def save(name):
        shot(screen, name, args.output)
        (args.output / f'{name}.ansi').write_text(''.join(chunks))

    try:
        wait('Ctrl+D')
        send('/help\r')
        wait('/setup')
        # Text wraps at 34 cells. Up must move to the previous visual row,
        # never replace the current draft with the remembered /help command.
        send('abcdefghijklmnopqrstuvwx中文0123456789')
        send('\x1b[A')
        send('X')
        save('wrapped-up')
        assert 'abcdXefgh' in text(), text()
        send('\x15' + 'a' * 33 + '中def')
        send('\x01\x1b[BX')
        save('wide-row-start')
        assert 'a' * 33 + 'X' in text(), text()
        send('\x15/goal alpha beta')
        save('arguments')
        assert '↑↓ 选择' not in text(), text()
        send('\x17')  # Ctrl+W
        send('\x1bbZ')  # Option+B, then insert
        save('word-motion')
        assert '/goal Zalpha' in text(), text()
        send('\x01X\x0b')  # Ctrl+A, insert X, Ctrl+K
        save('line-edit')
        assert any(row.strip() == '❯ X' for row in rows()), text()
        send('\x15\x1b[200~第一行\nalpha beta 👩‍💻\x1b[201~')
        send('\x17\x01X\x0b')
        save('unicode-lines')
        assert '第一行' in text() and any(row.strip() == 'X' for row in rows()), text()
        send('\x15')
        child.send('\x04')
        child.expect(pexpect.EOF, timeout=5)
        doctor = json.loads(subprocess.check_output(
            [str(args.binary.resolve()), '--home', directory, 'doctor', '--json'], text=True))
        result = dict(source_hash=doctor['source_hash'], actual_macos_pty=True,
                      native_window_screenshot=False, model_requests=0,
                      wrapped_arrows=True, argument_menu_hidden=True,
                      word_and_line_editing=True, unicode_preserved=True,
                      cursor_queries=''.join(chunks).count('\x1b[6n'))
        assert result['cursor_queries'] == 1
        (args.output / 'receipt.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result, indent=2))
    finally:
        save('last-capture')
        if child.isalive():
            child.terminate(force=True)
