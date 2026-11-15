"""Check the agent sandbox mode: a fresh unique child per start, refusals
before any state, nothing on stdout, the receipt, and descriptor passing."""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('provision-sandbox.py')
spec = importlib.util.spec_from_file_location('sandbox_provision', SCRIPT)
sandbox = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sandbox)
PREFIX = 'tilewright sandbox: '

# A stand-in for the editor: it reports its arguments and the directories
# its descriptors hold, then copies its input to its output unchanged, so
# the test sees exactly what the helper passes and that it adds nothing to
# the runner's output.
FAKE_EDITOR = '''import json, os, sys
arguments = sys.argv[1:]
opened = {}
for name in os.listdir('/proc/self/fd'):
    try:
        opened[name] = os.readlink('/proc/self/fd/' + name)
    except OSError:
        pass
fds = {}
for flag in ('--world-fd', '--recovery-fd', '--proposals-fd'):
    fd = int(arguments[arguments.index(flag) + 1])
    value = os.fstat(fd)
    fds[flag] = {'fd': fd, 'inode': value.st_ino, 'device': value.st_dev}
with open(os.environ['FAKE_EDITOR_REPORT'], 'w') as report:
    json.dump({'arguments': arguments, 'fds': fds, 'opened': opened}, report)
sys.stdout.buffer.write(sys.stdin.buffer.read())
'''


def persistent_root(test):
    """A temporary directory on persistent local storage, or a skip."""
    temporary = tempfile.TemporaryDirectory(dir=os.environ.get('TILEWRIGHT_PERSISTENT_TEST_ROOT'))
    fd = sandbox.provision.directory(Path(temporary.name))
    try:
        sandbox.provision.persistent_mount(fd)
    except ValueError as error:
        temporary.cleanup()
        test.skipTest(f'persistent test root unavailable: {error}')
    finally:
        os.close(fd)
    return temporary


def world(root):
    source = root / 'source'
    source.mkdir()
    (source / 'world.jsonc').write_bytes(b'{"original": true}\n')
    (source / 'nested').mkdir()
    (source / 'nested' / 'data').write_bytes(b'data')
    return source


def tree(root):
    return sorted((str(path.relative_to(root)), path.read_bytes() if path.is_file() else None)
                  for path in root.rglob('*'))


def run(*arguments, stdin=b''):
    return subprocess.run([sys.executable, str(SCRIPT), *map(str, arguments)], input=stdin,
                          capture_output=True, check=False)


def notice(result):
    lines = result.stderr.decode('ascii').splitlines()
    assert len(lines) == 1 and lines[0].startswith(PREFIX), lines
    return json.loads(lines[0][len(PREFIX):])


@unittest.skipUnless(sys.platform == 'linux', 'Linux descriptor provisioning')
class SandboxProvisionTests(unittest.TestCase):
    def test_each_start_makes_a_fresh_private_child_and_prints_nothing_on_stdout(self):
        with persistent_root(self) as temporary:
            root = Path(temporary)
            source = world(root)
            parent = root / 'sandboxes'
            parent.mkdir()
            original = tree(source)
            label = 'client \x1b[31m‮ "label"'
            arguments = ('--world', source, '--destination-parent', parent, '--actor', 'agent:tester',
                         '--client-label', label)
            first, second = run(*arguments), run(*arguments)
            for result in (first, second):
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, b'')
            self.assertEqual(tree(source), original)
            sandboxes = sorted(parent.iterdir())
            self.assertEqual(len(sandboxes), 2)
            self.assertEqual({notice(first)['sandbox'], notice(second)['sandbox']},
                             {str(path) for path in sandboxes})
            for result in (first, second):
                reported = notice(result)
                path = Path(reported['sandbox'])
                self.assertRegex(path.name, r'^sandbox-[0-9]{8}T[0-9]{6}Z-[0-9a-f]{16}$')
                self.assertEqual(sorted(child.name for child in path.iterdir()),
                                 ['proposals', 'provision.json', 'recovery', 'world'])
                for directory in (path, path / 'world', path / 'recovery', path / 'proposals', path / 'world/nested'):
                    self.assertEqual(stat.S_IMODE(directory.stat().st_mode), 0o700, directory)
                self.assertEqual(list((path / 'recovery').iterdir()), [])
                self.assertEqual(list((path / 'proposals').iterdir()), [])
                self.assertEqual((path / 'world/world.jsonc').read_bytes(), b'{"original": true}\n')
                receipt_path = path / 'provision.json'
                self.assertEqual(stat.S_IMODE(receipt_path.stat().st_mode), 0o600)
                self.assertEqual(reported['receipt'], str(receipt_path))
                data = receipt_path.read_bytes()
                self.assertEqual(reported['receipt_sha256'], hashlib.sha256(data).hexdigest())
                receipt = json.loads(data)
                self.assertEqual(receipt['format'], 'tilewright.sandbox-provision/1')
                self.assertEqual(receipt['actor'], 'agent:tester')
                self.assertEqual(receipt['client_label'], label)
                self.assertEqual(receipt['copy'], {'files': 2, 'bytes': 23})
                self.assertEqual(set(receipt['roots']), {'session', 'world', 'recovery', 'proposals'})
                for name, root_identity in receipt['roots'].items():
                    directory = path if name == 'session' else path / name
                    self.assertEqual(root_identity['inode'], directory.stat().st_ino, name)
                # The receipt and the notice are ASCII: the label's escape
                # and bidi characters are written as JSON escapes.
                self.assertTrue(data.isascii())
                self.assertNotIn(b'\x1b', result.stderr)
            self.assertNotEqual(notice(first)['receipt_sha256'], notice(second)['receipt_sha256'])

    def test_refusals_create_nothing_and_print_nothing_on_stdout(self):
        with persistent_root(self) as temporary:
            root = Path(temporary)
            source = world(root)
            parent = root / 'sandboxes'
            parent.mkdir()
            base = ('--world', source, '--destination-parent', parent)
            cases = {
                'no prefix': ('--actor', 'tester'),
                'prefix only': ('--actor', 'agent:'),
                'human actor': ('--actor', 'human:tester'),
                'trailing space': ('--actor', 'agent:tester '),
                'not ASCII': ('--actor', 'agent:téster'),
                'control': ('--actor', 'agent:te\x1bster'),
                'too long': ('--actor', 'agent:' + 'x' * 251),
                'long label': ('--actor', 'agent:tester', '--client-label', 'é' * 129),
                'empty label': ('--actor', 'agent:tester', '--client-label', ''),
                'no editor': ('--actor', 'agent:tester', '--editor', root / 'missing-editor'),
                'old destination': ('--actor', 'agent:tester', '--destination', root / 'session'),
                'font pack': ('--actor', 'agent:tester', '--font-pack', root),
            }
            for case, extra in cases.items():
                result = run(*base, *extra)
                self.assertNotEqual(result.returncode, 0, case)
                self.assertEqual(result.stdout, b'', case)
                self.assertEqual(list(parent.iterdir()), [], case)
                self.assertFalse((root / 'session').exists(), case)
            self.assertIsNotNone(sandbox.sandbox_actor('agent:' + 'x' * 250))
            self.assertEqual(sandbox.client_label('x' * 256), 'x' * 256)
            self.assertEqual(sandbox.client_label('\u00e9' * 128), '\u00e9' * 128)
            for label in ('x' * 257, '\u00e9' * 128 + 'x', '\udcff'):
                with self.assertRaises(ValueError):
                    sandbox.client_label(label)
            inside = source / 'nested'
            result = run('--world', source, '--destination-parent', inside, '--actor', 'agent:tester')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'inside it', result.stderr)
            self.assertEqual(sorted(path.name for path in inside.iterdir()), ['data'])
            result = run('--world', source, '--destination-parent', source, '--actor', 'agent:tester')
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(sorted(path.name for path in source.iterdir()), ['nested', 'world.jsonc'])
            missing = run('--world', root / 'absent', '--destination-parent', parent, '--actor', 'agent:tester')
            self.assertNotEqual(missing.returncode, 0)
            self.assertEqual(missing.stdout, b'')
            self.assertEqual(list(parent.iterdir()), [])
            # A copy refused part-way, here for a link, is found only after
            # the sandbox was made: it is kept for inspection, never deleted.
            linked = root / 'linked'
            linked.mkdir()
            (linked / 'a-link').symlink_to(source / 'world.jsonc')
            refused = run('--world', linked, '--destination-parent', parent, '--actor', 'agent:tester')
            self.assertNotEqual(refused.returncode, 0)
            self.assertEqual(refused.stdout, b'')
            self.assertIn(b'preserve any created state', refused.stderr)
            self.assertIn(b'link or special file', refused.stderr)
            self.assertEqual(len(list(parent.iterdir())), 1)
            self.assertEqual((source / 'world.jsonc').read_bytes(), b'{"original": true}\n')

    def test_volatile_tmpfs_is_refused_before_creating_a_sandbox(self):
        volatile = '/dev/shm' if Path('/dev/shm').is_dir() and os.access('/dev/shm', os.W_OK) else '/tmp'
        with tempfile.TemporaryDirectory(dir=volatile) as temporary:
            root = Path(temporary)
            fd = sandbox.provision.directory(root)
            try:
                try:
                    sandbox.provision.persistent_mount(fd)
                except ValueError:
                    pass
                else:
                    self.skipTest('no writable volatile test mount available')
            finally:
                os.close(fd)
            source = world(root)
            parent = root / 'sandboxes'
            parent.mkdir()
            result = run('--world', source, '--destination-parent', parent, '--actor', 'agent:tester')
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, b'')
            self.assertEqual(list(parent.iterdir()), [])

    def test_the_runner_gets_exactly_the_three_directories_and_the_protocol_streams(self):
        with persistent_root(self) as temporary:
            root = Path(temporary)
            source = world(root)
            parent = root / 'sandboxes'
            parent.mkdir()
            editor = root / 'fake-editor'
            editor.write_text(f'#!{sys.executable} -I\n{FAKE_EDITOR}')
            editor.chmod(0o700)
            report = root / 'report.json'
            requests = b'{"request":"status"}\n{"request":"quit"}\n'
            result = subprocess.run(
                [sys.executable, str(SCRIPT), '--world', str(source), '--destination-parent', str(parent),
                 '--actor', 'agent:tester', '--client-label', 'a label', '--editor', str(editor)],
                input=requests, capture_output=True, check=False,
                env={**os.environ, 'FAKE_EDITOR_REPORT': str(report)})
            self.assertEqual(result.returncode, 0, result.stderr)
            # Standard input reaches the runner, and its output is all
            # there is on standard output.
            self.assertEqual(result.stdout, requests)
            reported = notice(result)
            path = Path(reported['sandbox'])
            seen = json.loads(report.read_text())
            arguments = seen['arguments']
            self.assertEqual(arguments[0], 'sandbox')
            self.assertEqual(arguments[arguments.index('--actor') + 1], 'agent:tester')
            self.assertEqual(arguments[arguments.index('--client-label') + 1], 'a label')
            self.assertEqual(arguments[arguments.index('--receipt-sha256') + 1], reported['receipt_sha256'])
            self.assertEqual(len(arguments), 13)
            receipt = json.loads((path / 'provision.json').read_text())
            fds = set()
            for flag, child in (('--world-fd', 'world'), ('--recovery-fd', 'recovery'), ('--proposals-fd', 'proposals')):
                self.assertEqual(seen['fds'][flag]['inode'], receipt['roots'][child]['inode'], child)
                self.assertEqual(seen['fds'][flag]['device'], receipt['roots'][child]['device'], child)
                fds.add(seen['fds'][flag]['fd'])
            self.assertEqual(len(fds), 3)
            self.assertTrue(all(fd >= 3 for fd in fds))
            # The sandbox itself and its parent are not held open in the
            # runner: only its three children are.
            targets = set(seen['opened'].values())
            self.assertNotIn(str(path), targets)
            self.assertNotIn(str(parent), targets)
            for child in ('world', 'recovery', 'proposals'):
                self.assertIn(str(path / child), targets)


if __name__ == '__main__':
    unittest.main()
