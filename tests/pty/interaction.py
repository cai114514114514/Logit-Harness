"""Isolated actual macOS PTY checks with local HTTP responses and durable replay."""
import argparse, copy, hashlib, json, os, socket, sqlite3, subprocess, tempfile, threading, time, uuid
from pathlib import Path
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import pexpect, pyte
ROOT = Path(__file__).resolve().parents[2]
from grid import shot
p = argparse.ArgumentParser()
p.add_argument('--binary', type=Path, default=ROOT/'target/debug/loh')
p.add_argument('--output', type=Path, default=ROOT/'artifacts/interaction')
args = p.parse_args()
OUT = args.output.resolve(); OUT.mkdir(parents=True,exist_ok=True)
requests = []; main_turn = 0

def message(text):
    return {'type':'message','role':'assistant','content':[{'type':'output_text','text':text}]}
def call(name, arguments):
    return {'type':'function_call','call_id':str(uuid.uuid4()),'name':name,'arguments':json.dumps(arguments)}
class Provider(BaseHTTPRequestHandler):
    def do_POST(self):
        global main_turn
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        requests.append(request)
        role = request.get('instructions','').split('\n')[0]
        if role == 'ROLE: main':
            main_turn += 1
            if main_turn == 1:
                output = call('shell', {'command':"printf '# raw **literal**\\n'; sleep 8; printf 'quiet-finished\\n'", 'timeout_seconds':20})
            elif main_turn == 3:
                output = call('shell', {'command':"printf 'cancel-start\\n'; sleep 30", 'timeout_seconds':40})
            else:
                output = call('ask_user', {'question':'Fixture command complete; awaiting your answer.', 'requirements':['r1']})
        else:
            output = message('Fixture conversation response 中文.')
        packets = []
        if output['type'] == 'message':
            packets.append({'type':'response.output_text.delta','delta':'Fixture conversation '})
        packets.append({'type':'response.completed','response':{'status':'completed','model':'fixture','output':[output],'usage':{'input_tokens':100,'output_tokens':20,'input_tokens_details':{'cached_tokens':0}}}})
        body = ''.join('data: '+json.dumps(packet)+'\n\n' for packet in packets).encode()
        self.send_response(200); self.send_header('Content-Type','text/event-stream'); self.send_header('Content-Length',str(len(body))); self.end_headers()
        try: self.wfile.write(body)
        except BrokenPipeError: pass
    def log_message(self,*args): pass
class Screen(pyte.HistoryScreen):
    """pyte lacks xterm alternate-buffer save/restore; model that documented mode."""
    def set_mode(self,*modes,**kwargs):
        if kwargs.get('private') and 1049 in modes:
            self.saved_normal = copy.deepcopy((self.buffer,self.cursor,self.history))
            self.erase_in_display(2); self.cursor_position(1,1)
        super().set_mode(*modes,**kwargs)
    def reset_mode(self,*modes,**kwargs):
        if kwargs.get('private') and 1049 in modes and hasattr(self,'saved_normal'):
            self.buffer,self.cursor,self.history = self.saved_normal
        super().reset_mode(*modes,**kwargs)
class Client:
    def __init__(self,home,cwd,extra=(),key="fixture-only"):
        environment=dict(os.environ,TERM='xterm-256color',COLORTERM='truecolor');environment.pop('DEEPSEEK_API_KEY',None);environment.pop('NO_COLOR',None)
        if key is not None:environment['DEEPSEEK_API_KEY']=key
        self.child = pexpect.spawn(str(args.binary.resolve()), ['--home',str(home),*extra], cwd=str(cwd), env=environment, encoding='utf-8',dimensions=(32,110))
        self.screen=Screen(110,32,history=10000);self.stream=pyte.Stream(self.screen);self.chunks=[]
    def drain(self,seconds=.2):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            try:
                data=self.child.read_nonblocking(65536,timeout=.05);self.chunks.append(data)
                if '\x1b[6n' in data:self.child.send(f'\x1b[{self.screen.cursor.y+1};{self.screen.cursor.x+1}R')
                self.stream.feed(data)
            except pexpect.TIMEOUT:pass
            except pexpect.EOF:return
    def grid(self):return '\n'.join(''.join(self.screen.buffer[y][x].data for x in range(self.screen.columns)) for y in range(self.screen.lines))
    def wait(self,predicate,seconds=12):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            self.drain()
            if predicate():return
        raise AssertionError('PTY condition timed out:\n'+self.grid())
    def send(self,text):self.child.send(text);self.drain(.25)
    def save(self,name):
        shot(self.screen,name,OUT)
        (OUT/(name+'.txt')).write_text(self.grid()+'\n')
        (OUT/(name+'.ansi')).write_text(''.join(self.chunks))
        history=[''.join(row[x].data for x in sorted(row)) for row in self.screen.history.top]
        (OUT/(name+'-scrollback.txt')).write_text('\n'.join(history)+'\n')
    def close(self):
        if self.child.isalive():self.send('\x04');self.child.expect(pexpect.EOF,timeout=5)
server=ThreadingHTTPServer(('127.0.0.1',0),Provider);threading.Thread(target=server.serve_forever,daemon=True).start()
clients=[]; result={}
with tempfile.TemporaryDirectory(prefix='loh-task3-') as tmp:
    home=Path(tmp)/'home';home.mkdir(mode=0o700);homes=[home];cwd=Path(tmp)/'workspace';cwd.mkdir()
    (home/'config.toml').write_text(f'base_url="http://127.0.0.1:{server.server_port}"\nmax_output=1024\nuser_skills=false\n')
    doctor=subprocess.run([str(args.binary.resolve()),'--home',str(home),'doctor','--json'],cwd=str(cwd),capture_output=True,text=True,check=True)
    result.update(binary=str(args.binary.resolve()),source_hash=json.loads(doctor.stdout)['source_hash'],binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),native_window_screenshot=False)
    def db():return sqlite3.connect(home/'sessions.sqlite')
    def sessions():
        try:
            with db() as conn:return [(i,json.loads(s)) for i,s in conn.execute('SELECT id,state FROM sessions')]
        except sqlite3.OperationalError:return []
    def events(ident):
        with db() as conn:return [json.loads(s) for (s,) in conn.execute('SELECT data FROM events WHERE session=? ORDER BY sequence',(ident,))]
    try:
        c=Client(home,cwd);clients.append(c);c.wait(lambda:'Ctrl+D' in c.grid())
        c.send('/goal Run the isolated quiet fixture command\r')
        c.wait(lambda:any(e['type']=='process_started' for i,s in sessions() for e in events(i)))
        ident=sessions()[0][0]
        c.drain(1.2);c.save('task3-quiet')
        assert 'sleep 8' in c.grid() and '距输出' in c.grid()
        before=len(requests);c.send('/tools\r');c.wait(lambda:'工具能力' in ''.join(c.chunks));assert len(requests)==before
        c.send('/btw Explain the quiet command while it runs\r');c.wait(lambda:'Fixture conversation response' in c.grid());c.save('task3-interleaved')
        c.send('/new\r');c.wait(lambda:'新对话' in c.grid() or '原任务继续后台' in ''.join(c.chunks))
        c.send('\x1b[200~你好\n保留中文草稿\x1b[201~');c.drain(.2)
        assert len(sessions())==1, 'paste must not submit'
        c.send('\r');c.wait(lambda:len(sessions())==2)
        c.wait(lambda:any(st['mode']=='conversation' and st['status']=='waiting' for i,st in sessions() if i!=ident));c.wait(lambda:'Fixture conversation response' in c.grid());c.save('task3-new')
        c.wait(lambda:any(e['type']=='action_finished' and e.get('success') for e in events(ident)),seconds=15)
        assert not any(e['type']=='status' and e.get('status')=='stopped' for e in events(ident))
        result['new_keeps_background_command']=True
        c.send('/');c.drain(.4);c.save('task3-menu');c.send('\x15');c.send('/sessions\r');c.wait(lambda:'↑↓ 选择' in c.grid());c.save('task3-sessions')
        c.send('\x1b');c.send('/help\r');c.drain(.5);c.save('task3-scrollback')
        history=(OUT/'task3-scrollback-scrollback.txt').read_text()
        assert 'Fixture conversation response' in history or '工具能力' in history
        result['normal_scrollback_contains_settled_blocks']=True
        assert ''.join(c.chunks).count('\x1b[6n')==1, 'cursor queries must never recur'
        c.close()
        # Durable replay includes genuine process start/delta/result plus >300
        # additional immutable entries. No mutation of any real user session.
        stamp='2026-10-22T10:00:00Z'
        with db() as conn:
            for i in range(365):conn.execute('INSERT INTO events(session,timestamp,data) VALUES(?,?,?)',(ident,stamp,json.dumps({'type':'message','actor':'archive','text':f'OLD-HISTORY-{i:03d} '+('long Chinese 中文 '*8)})))
        replay=[]
        with db() as conn:
            for seq,ts,data in conn.execute('SELECT sequence,timestamp,data FROM events WHERE session=? ORDER BY sequence',(ident,)):
                replay.append(dict(sequence=seq,session=ident,timestamp=ts,**json.loads(data)))
        (OUT/'task3-replay.json').write_text(json.dumps(replay,ensure_ascii=False,indent=2))
        c=Client(home,cwd,['attach',ident]);clients.append(c);c.wait(lambda:'Ctrl+O' in c.grid());c.save('task3-recent')
        assert 'OLD-HISTORY-000' not in ''.join(c.chunks)
        recent_raw=''.join(c.chunks);assert recent_raw.count('OLD-HISTORY-')<=20
        c.send('/history OLD-HISTORY-000\r');c.wait(lambda:'OLD-HISTORY-000' in c.grid());c.save('task3-history-search')
        assert '1 条' in c.grid();c.send('\r');c.save('task3-history-expanded');c.send('\x1b')
        c.wait(lambda:'Ctrl+O' in c.grid());c.save('task3-history-return')
        count=''.join(c.chunks).count('OLD-HISTORY-364')
        c.child.setwinsize(26,70);c.screen.resize(lines=26,columns=70);c.drain(.5)
        c.child.setwinsize(32,110);c.screen.resize(lines=32,columns=110);c.drain(.5);c.save('task3-resize')
        assert ''.join(c.chunks).count('OLD-HISTORY-364')==count,'resize replayed settled output'
        assert ''.join(c.chunks).count('\x1b[6n')==1
        c.send('\x0f');c.wait(lambda:'完整历史' in c.grid());c.send('\x1b[6~');c.save('task3-history-page')
        assert '第 2/' in c.grid();c.send('\x1b');c.close()
        failure_home=Path(tmp)/'failure';failure_home.mkdir(mode=0o700);homes.append(failure_home)
        (failure_home/'config.toml').write_text(f'base_url="http://127.0.0.1:{server.server_port}"\nmax_output=1024\nuser_skills=false\n')
        def failed_sessions():
            try:
                with sqlite3.connect(failure_home/'sessions.sqlite') as conn:return [(i,json.loads(data)) for i,data in conn.execute('SELECT id,state FROM sessions')]
            except sqlite3.OperationalError:return []
        f=Client(failure_home,cwd,key=None);clients.append(f);f.wait(lambda:'Ctrl+D' in f.grid())
        before=len(requests);f.send('recoverable hello\r');f.wait(lambda:'后台启动失败' in ''.join(f.chunks))
        assert f.child.isalive() and len(requests)==before and len(failed_sessions())==1
        f.save('task3-startup-failure');assert 'recoverable hello' in f.grid()
        # The saved initial message is resumed once; it is not re-submitted from
        # the recovered draft. The separate secret field never enters chat history.
        f.send('\x15/setup\r');f.wait(lambda:'API' in f.grid());f.send('\tfixture-secret-mask-check');f.save('task3-setup-mask')
        assert 'fixture-secret-mask-check' not in ''.join(f.chunks)
        f.send('\r');f.drain(.3);f.send('/resume\r');f.wait(lambda:'Fixture conversation response' in f.grid())
        assert len(requests)==before+1
        first_id=failed_sessions()[0][0]
        f.send('/rename Restored fixture\r');f.wait(lambda:any(i==first_id and st['title']=='Restored fixture' for i,st in failed_sessions()))
        f.send('/config tokens 123456\r');f.wait(lambda:'默认值已保存' in ''.join(f.chunks))
        f.send('/new\r');f.drain(.3);f.send('new defaults hello\r');f.wait(lambda:len(failed_sessions())==2)
        f.wait(lambda:any(st['budget']['limit']==123456 for i,st in failed_sessions()))
        # SQLite creation precedes the asynchronous client attachment. Local
        # export needs that rendered session context, not merely a database row.
        f.wait(lambda:'new defaults hello' in f.grid() and 'Ctrl+O' in f.grid())
        f.send('/export fixture-export.json\r');f.wait(lambda:(cwd/'fixture-export.json').exists())
        before=len(requests);f.send('/new\r');f.drain(.2);f.send('/plan\r');f.wait(lambda:any(st['mode']=='plan' for i,st in failed_sessions()))
        f.drain(.4);assert len(requests)==before, 'bare /plan must remain model-idle'
        f.save('task3-plan-idle')
        f.send('/new\r');f.drain(.2);f.send('/goal Cancel isolated command\r')
        def failed_events():
            with sqlite3.connect(failure_home/'sessions.sqlite') as conn:return [json.loads(data) for (data,) in conn.execute('SELECT data FROM events ORDER BY sequence')]
        f.wait(lambda:any(e['type']=='process_started' for e in failed_events()));f.send('/pause\r')
        f.wait(lambda:any(e['type']=='action_finished' and not e['success'] for e in failed_events()))
        f.wait(lambda:'已暂停' in f.grid());f.save('task3-cancelled');f.close()
        result['actual_command_cancel_settled']=True
        result['side_answer_interleaved_with_live_tool']=True
        result.update(startup_failure_preserves_session_and_draft=True,setup_masked=True,resume_one_saved_request=True,config_applies_to_new_conversation=True,rename_and_export=True,bare_plan_model_idle=True)
        result.update(actual_macos_pty=True,paid_requests=0,local_requests=len(requests),history_entries=len(replay),history_search_older_than_300=True,resize_no_replay=True,cursor_queries_per_client=1)
        (OUT/'task3-pty.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    finally:
        for c in clients:
            c.save('task3-last-capture')
            if c.child.isalive():c.child.terminate(force=True)
        for endpoint in [endpoint for state_home in homes for endpoint in (state_home/'run').glob('*.sock')]:
            try:
                with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as conn:
                    conn.settimeout(1)
                    conn.connect(str(endpoint))
                    with conn.makefile("rb") as replies:
                        conn.sendall(b'{"type":"hello","protocol":1}\n')
                        assert json.loads(replies.readline())["type"] == "hello"
                        conn.sendall(b'{"type":"command","command":{"type":"stop"}}\n')
                        assert json.loads(replies.readline())["type"] == "ack"
                deadline = time.monotonic() + 5
                while endpoint.exists() and time.monotonic() < deadline:
                    time.sleep(.05)
                assert not endpoint.exists(), f"fixture daemon did not settle: {endpoint}"
            except OSError:pass
        time.sleep(.2)
server.shutdown()
