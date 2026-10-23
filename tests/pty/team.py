"""Actual three-PTY acceptance, or --serve for explicit manual native-window QA.

This is an OFFLINE test fixture, not model-quality evidence. No native window is
opened by this script. The provider binds loopback and all state is temporary.
"""
import argparse
import hashlib
import json
import shlex
import sqlite3
import tempfile
import threading
import time
from pathlib import Path

from team_fixture import Client, Provider, request, sessions, stop

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, default=ROOT/'target/debug/loh')
parser.add_argument('--output', type=Path, default=ROOT/'artifacts/team-2026-10-23/task-4')
parser.add_argument('--serve', action='store_true', help='Keep isolated OFFLINE provider/daemon alive for manual acceptance; Ctrl-C cleans up only this fixture')
args = parser.parse_args()
binary = args.binary.resolve()
output = args.output.resolve()
output.mkdir(parents=True, exist_ok=True)
provider = Provider()
clients = []
with tempfile.TemporaryDirectory(prefix='loh-team-offline-') as temporary:
    home = Path(temporary)/'home'
    cwd = Path(temporary)/'workspace'
    home.mkdir(mode=0o700)
    cwd.mkdir()
    (home/'config.toml').write_text(f'base_url="http://127.0.0.1:{provider.server.server_port}"\nmax_output=1024\nuser_skills=false\nseconds=3600\n')
    for name in ['Ada', 'Bob']:
        (cwd/(name+'.txt')).write_text(f'{name.upper()}-ONLY-DETAIL 中文\n')
    try:
        main = Client(binary, home, cwd)
        clients.append(main)
        main.wait(lambda: 'Ctrl+D' in main.text())
        main.send('/goal OFFLINE two independent fixture tasks\r')
        def state():
            return sessions(home)[0][1] if sessions(home) else {}
        main.wait(lambda: len(state().get('team', {}).get('members', {})) == 2)
        ident = sessions(home)[0][0]
        main.wait(lambda: all(m['waiting'] == 'main' for m in state()['team']['members'].values()))
        main.drain(2.5)
        main.save(output, 'team-main')
        members = {m['name']: m['id'] for m in state()['team']['members'].values()}
        frozen = len(provider.requests)
        ada = Client(binary, home, cwd, ['attach', ident, '--agent', members['Ada']])
        bob = Client(binary, home, cwd, ['attach', ident, '--agent', members['Bob']])
        clients.extend([ada, bob])
        ada.wait(lambda: 'Ada [' in ada.text())
        bob.wait(lambda: 'Bob [' in bob.text())
        ada.save(output, 'team-ada')
        bob.save(output, 'team-bob')
        assert 'Ada.txt' in ''.join(ada.chunks)
        assert 'Bob.txt' not in ''.join(ada.chunks)
        assert 'Bob.txt' in ''.join(bob.chunks)
        assert 'Ada.txt' not in ''.join(bob.chunks)
        assert 'Read  Ada.txt' not in ''.join(main.chunks)
        main.send('/team\r')
        main.wait(lambda: 'o 新窗口' in main.text())
        main.save(output, 'team-picker')
        main.send('\x1b[B\r')
        main.wait(lambda: '视图 member-' in ''.join(main.chunks))
        main.send('/agent main\r')
        main.wait(lambda: '位成员' in main.text() and '/team' in main.text())
        assert len(provider.requests) == frozen, 'Viewing/attaching must not call provider'
        if args.serve:
            for client in clients:
                client.close()
            print('OFFLINE FIXTURE ONLY — not real model behavior. Native windows were NOT opened.', flush=True)
            print('home:', home, '\nsession:', ident, flush=True)
            for name in ['main', 'Ada', 'Bob']:
                words = [str(binary), '--home', str(home), '--cwd', str(cwd), 'attach', ident, '--agent', members.get(name, name)]
                print(name+': '+shlex.join(words), flush=True)
            print('Ctrl-C stops only this fixture daemon/provider and removes its temporary state.', flush=True)
            try:
                while True:
                    time.sleep(.5)
            except KeyboardInterrupt:
                pass
        else:
            # First-edit binding and independent drafts, with actual CJK paste.
            ada.send('\x1b[200~中文草稿\n第二行\x1b[201~')
            ada.save(output, 'team-cjk-draft')
            assert '中文草稿' in ada.text() and '第二行' in ada.text()
            # Ctrl+T opens team navigation without replacing the current draft.
            # Switching views then restores the two independent real editors.
            ada.send('\x14')
            ada.wait(lambda: 'o 新窗口' in ada.text())
            ada.send('\r')
            ada.wait(lambda: '位成员' in ada.text())
            ada.send('main scoped draft')
            ada.save(output, 'team-draft-main')
            ada.send('\x14')
            ada.wait(lambda: 'o 新窗口' in ada.text())
            position = list(state()['team']['members']).index(members['Ada']) + 1
            ada.send('\x1b[B'*position+'\r')
            ada.wait(lambda: '中文草稿' in ada.text() and 'Ada [' in ada.text())
            assert 'main scoped draft' not in ada.text()
            ada.save(output, 'team-draft-return')
            ada.send('\x15')
            ada.send('\x03')
            ada.wait(lambda: state()['team']['members'][members['Ada']]['paused'])
            assert not state()['team']['members'][members['Bob']]['paused']
            ada.wait(lambda: '已应用' in ''.join(ada.chunks))
            ada.save(output, 'team-local-pause')
            bob.send('/goal must be refused\r')
            bob.wait(lambda: '需要 main' in ''.join(bob.chunks))
            assert 'must be refused' in bob.text()
            bob.send('\x15')
            # A member side question is handled by the local provider and appears
            # only in its target view. The other member remains paused.
            bob.send('/btw literal question 中文\r')
            bob.wait(lambda: 'OFFLINE-SIDE-ANSWER' in ''.join(bob.chunks))
            bob.save(output, 'team-side-answer')
            ada.drain(.3)
            assert 'OFFLINE-SIDE-ANSWER' not in ''.join(ada.chunks)
            assert state()['team']['members'][members['Ada']]['paused']
            before = len(provider.requests)
            bob.send('/history BOB-ONLY-DETAIL\r')
            bob.wait(lambda: 'BOB-ONLY-DETAIL' in bob.text() and '完整历史' in bob.text())
            bob.save(output, 'team-history')
            assert 'ADA-ONLY-DETAIL' not in bob.text()
            bob.send('\x1b')
            assert len(provider.requests) == before, 'History must stay provider-idle'
            # All three PTYs submit independently before any client is drained.
            # Each command must retain its own target and application receipt.
            previous = set(state()['team']['receipts'])
            barrier = threading.Barrier(3)
            sent = []
            def submit(client, text):
                barrier.wait()
                sent.append(time.monotonic())
                client.child.send(text+'\r')
            threads = [threading.Thread(target=submit, args=pair) for pair in
                       [(main, '/team limit 1'), (ada, '/pause'), (bob, '/pause')]]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join()
            def concurrent_receipts():
                return [receipt for key, receipt in state()['team']['receipts'].items() if key not in previous]
            end = time.monotonic()+10
            while time.monotonic() < end:
                for client in [main, ada, bob]:
                    client.drain(.1)
                received = concurrent_receipts()
                if len(received) == 3 and all(receipt['status'] == 'applied' for receipt in received):
                    break
            assert len(received) == 3 and all(receipt['status'] == 'applied' for receipt in received)
            assert len({receipt['request']['client_id'] for receipt in received}) == 3
            targets = {receipt['request']['target'].get('id', 'main') for receipt in received}
            assert targets == {'main', members['Ada'], members['Bob']}
            assert state()['team']['limit'] == 1
            main.wait(lambda: '并发上限 1' in main.text())
            for label, client in [('main', main), ('ada', ada), ('bob', bob)]:
                client.save(output, 'team-concurrent-'+label)
            # Member resume cannot defeat global pause. Global resume must retain
            # Ada's explicit local pause while Bob returns to its prior mail wait.
            main.send('/team pause\r')
            main.wait(lambda: state()['status'] == 'paused')
            bob.send('/resume\r')
            bob.wait(lambda: not state()['team']['members'][members['Bob']]['paused'])
            assert state()['status'] == 'paused'
            main.send('/team resume\r')
            main.wait(lambda: state()['status'] != 'paused')
            assert state()['team']['members'][members['Ada']]['paused']
            assert not state()['team']['members'][members['Bob']]['paused']
            main.send('/team limit 2\r')
            main.wait(lambda: state()['team']['limit'] == 2)
            main.wait(lambda: state()['actors']['main']['phase'] == 'blocked' and all(actor['busy'] == 0 for actor in state()['actors'].values()))
            before = len(provider.requests)
            main.save(output, 'team-global-resume')
            for client in [ada, bob]:
                client.child.setwinsize(24, 70)
                client.screen.resize(lines=24, columns=70)
                client.drain(.4)
                client.child.setwinsize(32, 110)
                client.screen.resize(lines=32, columns=110)
                client.drain(.4)
            bob.save(output, 'team-resize')
            bob.close()
            bob = Client(binary, home, cwd, ['attach', ident, '--agent', members['Bob']])
            clients.append(bob)
            bob.wait(lambda: 'Bob [' in bob.text())
            bob.save(output, 'team-reconnect')
            assert len(provider.requests) == before, 'History/resize/reattach must stay provider-idle'
            bob.send('/new\r')
            bob.wait(lambda: '原任务继续后台' in ''.join(bob.chunks))
            assert state()['team']['members'][members['Ada']]['paused']
            assert state()['team']['members'][members['Bob']]['waiting'] == 'main'
            bob.save(output, 'team-new-background')
            receipt = dict(actual_pty_clients=3, native_windows=False, paid_requests=0,
                provider_requests=len(provider.requests), idle_view_request_count=frozen,
                view_actions_add_no_requests=True, isolated_home=True,
                member_output_attributed=True, local_pause=True, main_only_mutations=True,
                view_history=True, cjk_multiline=True, per_view_drafts=True, new_keeps_team=True,
                concurrent_inputs=3, concurrent_submit_spread_seconds=max(sent)-min(sent),
                concurrent_receipts=received, global_pause_dominates_local_resume=True,
                global_resume_preserves_local_pause=True,
                binary=str(binary), binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
            with sqlite3.connect(home/'sessions.sqlite') as conn:
                events = [dict(sequence=seq, timestamp=stamp, **json.loads(data)) for seq, stamp, data in conn.execute('SELECT sequence,timestamp,data FROM events WHERE session=? ORDER BY sequence', (ident,))]
            (output/'team-events.json').write_text(json.dumps(events, ensure_ascii=False, indent=2))
            (output/'team-state.json').write_text(json.dumps(state(), ensure_ascii=False, indent=2))
            (output/'team-receipt.json').write_text(json.dumps(receipt, indent=2)+'\n')
            print(json.dumps(receipt, indent=2))
    finally:
        for index, client in enumerate(clients):
            client.save(output, f'team-final-{index}')
            if client.child.isalive():
                client.child.terminate(force=True)
        stop(home)
        provider.close()
