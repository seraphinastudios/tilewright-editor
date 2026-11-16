"""Check private fresh-root provisioning and refusal without source mutation."""
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

SCRIPT = Path(__file__).with_name('provision-prototype.py')
spec = importlib.util.spec_from_file_location('prototype_provision', SCRIPT)
provision = importlib.util.module_from_spec(spec)
spec.loader.exec_module(provision)


@unittest.skipUnless(sys.platform == 'linux', 'Linux descriptor provisioning')
class ProvisionTests(unittest.TestCase):
    def test_private_fresh_copy_preserves_original_and_refuses_existing(self):
        with tempfile.TemporaryDirectory(dir=os.environ.get('TILEWRIGHT_PERSISTENT_TEST_ROOT')) as temporary:
            root = Path(temporary)
            fd = provision.directory(root)
            try:
                try:
                    provision.persistent_mount(fd)
                except ValueError as error:
                    self.skipTest(f'persistent test root unavailable: {error}')
            finally:
                os.close(fd)
            source = root / 'source'
            source.mkdir()
            (source / 'world.jsonc').write_bytes(b'{"original": true}\n')
            (source / 'nested').mkdir()
            (source / 'nested' / 'data').write_bytes(b'data')
            destination = root / 'session'
            command = [sys.executable, str(SCRIPT), '--world', str(source), '--destination', str(destination)]
            subprocess.run(command, check=True, capture_output=True)
            self.assertEqual((source / 'world.jsonc').read_bytes(), b'{"original": true}\n')
            self.assertEqual((destination / 'world/world.jsonc').read_bytes(), b'{"original": true}\n')
            for path in (destination, destination/'world', destination/'recovery', destination/'evidence', destination/'world/nested'):
                self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o700)
            self.assertEqual(stat.S_IMODE((destination/'world/world.jsonc').stat().st_mode), 0o600)
            self.assertNotEqual((destination/'world').stat().st_ino, (destination/'recovery').stat().st_ino)
            before = (destination/'provision.json').read_bytes()
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
            self.assertEqual((destination/'provision.json').read_bytes(), before)

    def test_volatile_tmpfs_is_refused_before_creating_a_session(self):
        with tempfile.TemporaryDirectory(dir='/dev/shm' if Path('/dev/shm').is_dir() and os.access('/dev/shm', os.W_OK) else '/tmp') as temporary:
            root = Path(temporary)
            fd = provision.directory(root)
            try:
                try:
                    provision.persistent_mount(fd)
                except ValueError:
                    pass
                else:
                    self.skipTest('no writable volatile test mount available')
            finally:
                os.close(fd)
            source = root/'source'
            source.mkdir()
            destination = root/'session'
            result = subprocess.run([sys.executable,str(SCRIPT),'--world',str(source),'--destination',str(destination)],capture_output=True)
            self.assertNotEqual(result.returncode,0)
            self.assertFalse(destination.exists())

    def test_editor_launch_requires_explicit_font_pack_before_creating_state(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'source'
            source.mkdir()
            result = subprocess.run([sys.executable, str(SCRIPT), '--world', str(source),
                '--destination', str(root / 'session'), '--editor', str(root / 'editor')], capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'explicit --font-pack', result.stderr)
            self.assertFalse((root / 'session').exists())

    def test_copy_refuses_links_and_retains_any_partial_data(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root/'source'
            source.mkdir()
            outside = root/'outside'
            outside.write_bytes(b'outside')
            (source/'a-link').symlink_to(outside)
            destination = root/'destination'
            destination.mkdir(mode=0o700)
            fd = provision.directory(destination)
            try:
                with self.assertRaisesRegex(ValueError, 'link or special'):
                    provision.copy_tree(source, fd)
            finally:
                os.close(fd)
            self.assertEqual(outside.read_bytes(), b'outside')
            self.assertEqual(list(destination.iterdir()), [])


NAME = 'proposal-' + '0123456789abcdef' * 4 + '.json'


@unittest.skipUnless(sys.platform == 'linux', 'Linux descriptor provisioning')
class ProposalOptionTests(unittest.TestCase):
    """--proposal and --actor: checked before anything is created, the file
    copied into a private proposal/ child and recorded in the receipt."""

    def persistent_root(self, temporary):
        root = Path(temporary)
        fd = provision.directory(root)
        try:
            try:
                provision.persistent_mount(fd)
            except ValueError as error:
                self.skipTest(f'persistent test root unavailable: {error}')
        finally:
            os.close(fd)
        return root

    def run_helper(self, root, *extra):
        source = root / 'source'
        if not source.exists():
            source.mkdir()
            (source / 'world.jsonc').write_bytes(b'{}\n')
        command = [sys.executable, str(SCRIPT), '--world', str(source), '--destination', str(root / 'session'), *extra]
        return subprocess.run(command, capture_output=True)

    def test_the_proposal_is_copied_into_a_private_child_and_recorded(self):
        with tempfile.TemporaryDirectory(dir=os.environ.get('TILEWRIGHT_PERSISTENT_TEST_ROOT')) as temporary:
            root = self.persistent_root(temporary)
            data = b'{"proposal": "bytes as given"}' * 1000
            source = root / 'agent' / NAME
            source.parent.mkdir()
            source.write_bytes(data)
            os.chmod(source, 0o444)
            before = source.stat()
            result = self.run_helper(root, '--proposal', str(source), '--actor', 'human:tester')
            self.assertEqual(result.returncode, 0, result.stderr)
            session = root / 'session'
            copied = session / 'proposal' / NAME
            self.assertEqual(copied.read_bytes(), data)
            self.assertEqual(sorted(path.name for path in (session / 'proposal').iterdir()), [NAME])
            self.assertEqual(stat.S_IMODE((session / 'proposal').stat().st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(copied.stat().st_mode), 0o600)
            self.assertNotEqual(copied.stat().st_ino, source.stat().st_ino)
            # The source is neither changed nor touched.
            self.assertEqual(source.read_bytes(), data)
            after = source.stat()
            self.assertEqual((before.st_mode, before.st_mtime_ns, before.st_size), (after.st_mode, after.st_mtime_ns, after.st_size))
            receipt = json.loads((session / 'provision.json').read_text())
            self.assertEqual(receipt['proposal'], {'name': NAME, 'bytes': len(data),
                                                   'sha256': hashlib.sha256(data).hexdigest()})
            self.assertEqual(receipt['actor'], 'human:tester')
            self.assertEqual(set(receipt['roots']), {'session', 'world', 'recovery', 'evidence', 'proposal'})
            printed = json.loads(result.stdout)
            self.assertEqual(printed['provision'], receipt)

    def test_without_the_options_the_session_and_receipt_are_unchanged(self):
        with tempfile.TemporaryDirectory(dir=os.environ.get('TILEWRIGHT_PERSISTENT_TEST_ROOT')) as temporary:
            root = self.persistent_root(temporary)
            result = self.run_helper(root)
            self.assertEqual(result.returncode, 0, result.stderr)
            session = root / 'session'
            self.assertEqual(sorted(path.name for path in session.iterdir()),
                             ['evidence', 'provision.json', 'recovery', 'world'])
            receipt = json.loads((session / 'provision.json').read_text())
            self.assertEqual(sorted(receipt), ['copy', 'font_pack', 'format', 'platform', 'roots', 'scope'])
            self.assertEqual(sorted(receipt['roots']), ['evidence', 'recovery', 'session', 'world'])

    def test_bad_proposals_and_actors_are_refused_before_anything_is_created(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            agent = root / 'agent'
            agent.mkdir()
            good = agent / NAME
            good.write_bytes(b'{}')
            link = agent / ('proposal-' + 'a' * 64 + '.json')
            link.symlink_to(good)
            directory = agent / ('proposal-' + 'b' * 64 + '.json')
            directory.mkdir()
            oversize = agent / ('proposal-' + 'c' * 64 + '.json')
            with open(oversize, 'wb') as stream:
                stream.truncate((16 << 20) + 1)
            badly_named = agent / 'proposal.json'
            badly_named.write_bytes(b'{}')
            upper = agent / ('proposal-' + 'A' * 64 + '.json')
            upper.write_bytes(b'{}')
            fifo = agent / ('proposal-' + 'd' * 64 + '.json')
            os.mkfifo(fifo)
            cases = [
                (['--proposal', str(link)], b'not a link'),
                (['--proposal', str(directory)], b'regular file'),
                (['--proposal', str(oversize)], b'16 MiB'),
                (['--proposal', str(badly_named)], b'proposal-<64'),
                (['--proposal', str(upper)], b'proposal-<64'),
                (['--proposal', str(agent / ('proposal-' + 'e' * 64 + '.json'))], b'existing readable'),
                (['--proposal', str(fifo)], b'regular file'),
                (['--proposal', str(good), '--actor', 'agent:tester'], b'human:<name>'),
                (['--proposal', str(good), '--actor', 'human:'], b'human:<name>'),
                (['--actor', 'human:t\u00e9ster'], b'printable ASCII'),
                (['--actor', 'human:tester '], b'end with a space'),
                (['--actor', 'human:' + 'x' * 251], b'256'),
            ]
            for extra, message in cases:
                with self.subTest(extra=extra):
                    result = self.run_helper(root, *extra)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(message, result.stderr)
                    self.assertFalse((root / 'session').exists())
            # The agent's files are as they were.
            self.assertEqual(good.read_bytes(), b'{}')
            self.assertEqual(os.stat(oversize).st_size, (16 << 20) + 1)

    def test_a_valid_actor_alone_is_passed_and_recorded(self):
        self.assertEqual(provision.person_actor('human:A. Person (desk 2)'), 'human:A. Person (desk 2)')
        self.assertEqual(provision.person_actor('human:' + 'x' * 250), 'human:' + 'x' * 250)


if __name__ == '__main__':
    unittest.main()
