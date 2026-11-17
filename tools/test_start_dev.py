"""Linux pwsh process-contract checks; fake HTTP children, NOT Windows acceptance.
Run: ORIGENCE_TEST_PWSH=/path/to/pwsh python3 -m unittest discover -s tools -p test_start_dev.py
No provider keys, real data, or external services. Skips without opt-in pwsh/on Windows.
"""
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import unittest

FAKE = r'''#!/usr/bin/env python3
import http.server, json, os, pathlib, sys, threading, time
role=pathlib.Path(sys.argv[0]).stem
args=sys.argv[1:]
port=int(next(a.split('=',1)[1] for a in args if a.startswith('local_port='))) if role=='anyseek' else int(args[args.index('--bind')+1].split(':')[-1]) if role=='origence' else int(os.environ['ORIGENCE_UI_PORT'])
if role=='anyseek': assert 'OC_ANYSEEK_API_KEY' not in os.environ
if role=='origence': assert 'MASTER_KEY_SECRET' not in os.environ and 'OC_ANYSEEK_COLLECTION' not in os.environ and 'OC_ANYSEEK_TENANT' not in os.environ
if role=='node': assert 'MASTER_KEY_SECRET' not in os.environ and 'OC_ANYSEEK_API_KEY' not in os.environ
pathlib.Path(os.environ['DEV_TEST_RECORD']+'/'+role).write_text(json.dumps({'pid':os.getpid(),'args':args}))
print(os.environ['DEV_TEST_SECRET'],flush=True)
print(os.environ['DEV_TEST_SECRET'],file=sys.stderr,flush=True)
mode=os.environ['DEV_TEST_MODE']
if mode=='exit' and role=='anyseek': sys.exit(23)
if role=='origence':
 threading.Thread(target=lambda:(time.sleep(3),os._exit(24)),daemon=True).start()
class Handler(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args): pass
 def do_GET(self):
  if role=='anyseek': assert self.path=='/api/v1/live' and self.headers.get('apikey')=='disposable-key'
  code=302 if mode=='redirect' and role=='anyseek' else 403 if mode=='denied' and role=='anyseek' else 503 if mode=='timeout' and role=='anyseek' else 200
  self.send_response(code);self.end_headers()
  body='fixture-1' if role=='anyseek' else {'status':'ready'}
  self.wfile.write(b'<html>fixture</html>' if role=='node' else json.dumps(body).encode())
http.server.HTTPServer(('127.0.0.1',port),Handler).serve_forever()
'''


@unittest.skipUnless(os.environ.get('ORIGENCE_TEST_PWSH') and os.name == 'posix', 'opt-in Linux pwsh fixture only')
class StartDevTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='origence dev fixture ')
        self.root = Path(self.tmp.name)
        (self.root/'tools').mkdir()
        shutil.copy(Path(__file__).with_name('start-dev.ps1'), self.root/'tools/start-dev.ps1')
        (self.root/'ui/dist').mkdir(parents=True)
        (self.root/'ui/dist/server.js').touch()
        (self.root/'api').mkdir(); (self.root/'api/context.db').write_text('fixture')
        (self.root/'index').mkdir(); (self.root/'index/existing').touch()
        (self.root/'records').mkdir()
        self.env = dict(os.environ, MASTER_KEY_SECRET='disposable-master', OC_ANYSEEK_API_KEY='disposable-key', DEV_TEST_SECRET='MUST_NOT_LEAK_471', DEV_TEST_RECORD=str(self.root/'records'))
        for category in ['CACHE','CONFIG','DATA']:
            self.env['XDG_'+category+'_HOME'] = str(self.root/category)
        for role in ['anyseek','origence','node']:
            path=self.root/role; path.write_text(FAKE); path.chmod(0o700)
        sockets=[]
        for _ in range(3):
            sock=socket.socket(); sock.bind(('127.0.0.1',0)); sockets.append(sock)
        self.ports=[s.getsockname()[1] for s in sockets]
        for sock in sockets: sock.close()

    def tearDown(self):
        # Defensive fixture cleanup if an assertion catches a launcher bug.
        for path in (self.root/'records').iterdir():
            pid=json.loads(path.read_text())['pid']
            try:
                command = Path(f'/proc/{pid}/cmdline').read_bytes().split(b'\0')
                if str(self.root/path.name).encode() in command:
                    os.kill(pid, 9)
            except (ProcessLookupError, FileNotFoundError):
                pass
        self.tmp.cleanup()

    def run_launcher(self, mode, ui=False):
        command=[os.environ['ORIGENCE_TEST_PWSH'],'-NoProfile','-File',str(self.root/'tools/start-dev.ps1'),'-AnySeekExe',str(self.root/'anyseek'),'-AnySeekDataDir',str(self.root/'index'),'-OrigenceExe',str(self.root/'origence'),'-DataDir',str(self.root/'api'),'-AnySeekPort',str(self.ports[0]),'-ApiPort',str(self.ports[1]),'-UiPort',str(self.ports[2]),'-StartupTimeoutSeconds','2','-ShutdownTimeoutSeconds','1']
        if ui: command+=['-Ui','-NodeExe',str(self.root/'node')]
        result=subprocess.run(command,env=dict(self.env,DEV_TEST_MODE=mode),capture_output=True,text=True,timeout=20)
        output=result.stdout+result.stderr
        self.assertNotIn(self.env['DEV_TEST_SECRET'],output)
        self.assertNotIn('disposable-master',output)
        self.assertNotIn('disposable-key',output)
        self.assertNotEqual(result.returncode,0) # every fixture eventually exits/fails
        for path in (self.root/'records').iterdir():
            record=json.loads(path.read_text())
            self.assertNotIn('disposable',str(record['args']))
            with self.assertRaises(ProcessLookupError): os.kill(record['pid'],0)
        return output

    def test_ready_ui_then_child_exit_cleans_only_owned_processes(self):
        output=self.run_launcher('ready',True)
        self.assertIn('Dev services ready',output)
        self.assertIn('UI ready',output)
        self.assertIn('Origence exited (code 24)',output)
        self.assertEqual(len(list((self.root/'records').iterdir())),3)

    def test_early_exit(self):
        self.assertIn('AnySeek exited (code 23)',self.run_launcher('exit'))
        self.assertFalse((self.root/'records/origence').exists())

    def test_http_not_ready_times_out_without_starting_origence(self):
        self.assertIn('readiness timed out',self.run_launcher('timeout'))
        self.assertFalse((self.root/'records/origence').exists())

    def test_auth_failure_is_not_ready(self):
        self.assertIn('HTTP 403',self.run_launcher('denied'))
        self.assertFalse((self.root/'records/origence').exists())

    def test_redirect_is_rejected_without_forwarding_credentials(self):
        self.assertIn('HTTP 302',self.run_launcher('redirect'))
        self.assertFalse((self.root/'records/origence').exists())

    def test_missing_data_is_not_created(self):
        shutil.rmtree(self.root/'api')
        self.assertIn('must already exist',self.run_launcher('ready'))
        self.assertFalse((self.root/'api').exists())
        self.assertFalse(list((self.root/'records').iterdir()))

    def test_occupied_port_does_not_start_or_stop_services(self):
        with socket.socket() as occupied:
            occupied.bind(('127.0.0.1',self.ports[1]));occupied.listen()
            self.assertIn('unavailable',self.run_launcher('ready'))
            self.assertEqual(occupied.getsockname()[1],self.ports[1])
            self.assertFalse(list((self.root/'records').iterdir()))


if __name__ == '__main__':
    unittest.main()
