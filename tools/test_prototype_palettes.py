"""Check the helper's --palettes: checked before anything is created, copied
into a private palettes/ child, recorded by digest only, and the editor's
newest saved file copied back only when that is safe."""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import argparse
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
spec = importlib.util.spec_from_file_location('prototype_provision_palettes', SCRIPT)
provision = importlib.util.module_from_spec(spec)
spec.loader.exec_module(provision)

# Personal-looking content that must never reach the receipt.
PERSONAL = b'{"format": "tilewright.editor-favourites/1", "objects": ["personalprototype"]}\n'
SAVED = b'{\n  "format": "tilewright.editor-favourites/1",\n  "terrain": [],\n  "scatter": [],\n  "objects": []\n}\n'


def persistent_root(case, temporary):
    root = Path(temporary)
    fd = provision.directory(root)
    try:
        try:
            provision.persistent_mount(fd)
        except ValueError as error:
            case.skipTest(f'persistent test root unavailable: {error}')
    finally:
        os.close(fd)
    return root


def run_helper(root, *extra):
    source = root / 'source'
    if not source.exists():
        source.mkdir()
        (source / 'world.jsonc').write_bytes(b'{}\n')
    command = [sys.executable, str(SCRIPT), '--world', str(source), '--destination', str(root / 'session'), *extra]
    return subprocess.run(command, capture_output=True)


@unittest.skipUnless(sys.platform == 'linux', 'Linux descriptor provisioning')
class PalettesOptionTests(unittest.TestCase):
    def test_the_file_is_copied_into_a_private_child_and_recorded_by_digest_only(self):
        with tempfile.TemporaryDirectory(dir=os.environ.get('TILEWRIGHT_PERSISTENT_TEST_ROOT')) as temporary:
            root = persistent_root(self, temporary)
            source = root / 'config' / 'my-favourites.json'
            source.parent.mkdir()
            source.write_bytes(PERSONAL)
            os.chmod(source, 0o640)
            before = source.stat()
            result = run_helper(root, '--palettes', str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            session = root / 'session'
            copied = session / 'palettes' / 'favourites.json'
            self.assertEqual(copied.read_bytes(), PERSONAL)
            self.assertEqual([path.name for path in (session / 'palettes').iterdir()], ['favourites.json'])
            self.assertEqual(stat.S_IMODE((session / 'palettes').stat().st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(copied.stat().st_mode), 0o600)
            self.assertNotEqual(copied.stat().st_ino, source.stat().st_ino)
            after = source.stat()
            self.assertEqual((before.st_mode, before.st_mtime_ns, before.st_ino),
                             (after.st_mode, after.st_mtime_ns, after.st_ino))
            text = (session / 'provision.json').read_text()
            receipt = json.loads(text)
            self.assertEqual(receipt['palettes'], {'bytes': len(PERSONAL),
                                                   'sha256': hashlib.sha256(PERSONAL).hexdigest()})
            self.assertEqual(set(receipt['roots']), {'session', 'world', 'recovery', 'evidence', 'palettes'})
            # Neither the person's path, the file's name nor its contents.
            for personal in ('personalprototype', 'my-favourites', str(source.parent)):
                self.assertNotIn(personal, text)
                self.assertNotIn(personal, result.stdout.decode())
            self.assertEqual(json.loads(result.stdout)['provision'], receipt)

    def test_bad_files_and_the_scripted_demonstration_are_refused_before_anything_is_created(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config = root / 'config'
            config.mkdir()
            good = config / 'good.json'
            good.write_bytes(PERSONAL)
            link = config / 'link.json'
            link.symlink_to(good)
            directory = config / 'directory.json'
            directory.mkdir()
            oversize = config / 'oversize.json'
            with open(oversize, 'wb') as stream:
                stream.truncate((64 << 10) + 1)
            fifo = config / 'fifo.json'
            os.mkfifo(fifo)
            cases = [
                (['--palettes', str(link)], b'not a link'),
                (['--palettes', str(directory)], b'regular file'),
                (['--palettes', str(fifo)], b'regular file'),
                (['--palettes', str(oversize)], b'64 KiB'),
                (['--palettes', str(config / 'missing.json')], b'existing readable'),
                (['--palettes', str(good), '--demo-script'], b'never reach evidence'),
                (['--palettes', str(good), '--reference', '--demo-script'], b'never reach evidence'),
            ]
            for extra, message in cases:
                with self.subTest(extra=extra):
                    result = run_helper(root, *extra)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(message, result.stderr)
                    self.assertFalse((root / 'session').exists())
            self.assertEqual(good.read_bytes(), PERSONAL)

    def test_the_editor_is_given_the_directory_exactly_when_the_file_is(self):
        children = {'world': 3, 'recovery': 4, 'evidence': 5, 'palettes': 6}
        args = argparse.Namespace(font_pack=Path('/fonts'), demo_script=False, reference=False,
                                  shared_string_field=False)
        without = provision.editor_command(Path('/editor'), children, args, None, None)
        self.assertNotIn('--palettes-fd', without)
        given = provision.editor_command(Path('/editor'), children, args, None, None, True)
        self.assertEqual(given, without + ['--palettes-fd', '6'])


@unittest.skipUnless(sys.platform == 'linux', 'Linux descriptor provisioning')
class ReturnTests(unittest.TestCase):
    """return_palettes: the newest saved file back to the person's file, only
    when that loses nothing; otherwise where it is."""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.session = self.root / 'session'
        self.palettes = self.session / 'palettes'
        self.palettes.mkdir(parents=True, mode=0o700)
        (self.palettes / 'favourites.json').write_bytes(PERSONAL)
        self.target = self.root / 'config' / 'favourites.json'
        self.target.parent.mkdir()
        self.target.write_bytes(PERSONAL)
        os.chmod(self.target, 0o640)
        value = self.target.stat()
        self.original = {'sha256': hashlib.sha256(PERSONAL).hexdigest(), 'identity': (value.st_dev, value.st_ino)}

    def tearDown(self):
        self.temporary.cleanup()

    def save(self, name, data=SAVED, mode=0o600):
        path = self.palettes / name
        path.write_bytes(data)
        os.chmod(path, mode)
        return path

    def run_return(self):
        fd = provision.directory(self.palettes)
        try:
            return provision.return_palettes(fd, self.target, self.original, self.session)
        finally:
            os.close(fd)

    def assert_kept(self, outcome, name, reason):
        self.assertFalse(outcome['returned'])
        self.assertEqual(outcome['kept'], str(self.palettes / name))
        self.assertIn(reason, outcome['reason'])
        self.assertEqual(self.target.read_bytes(), PERSONAL)
        self.assertEqual(sorted(path.name for path in self.target.parent.iterdir()), ['favourites.json'])

    def test_nothing_saved_leaves_the_file_alone(self):
        self.assertEqual(self.run_return(), {'returned': False, 'reason': 'the editor saved no change'})
        self.assertEqual(self.target.read_bytes(), PERSONAL)

    def test_the_newest_saved_file_replaces_the_unchanged_file_keeping_its_mode(self):
        self.save('favourites-0001.json', SAVED.replace(b'"objects": []', b'"objects": ["a"]'))
        self.save('favourites-0010.json')
        self.save('favourites-0002.json', SAVED.replace(b'"objects": []', b'"objects": ["b"]'))
        # Not the editor's names: ignored.
        self.save('favourites-99999.json', b'other')
        self.save('favourites.json.bak', b'other')
        outcome = self.run_return()
        self.assertEqual(outcome, {'returned': True, 'from': 'favourites-0010.json',
                                   'sha256': hashlib.sha256(SAVED).hexdigest()})
        self.assertEqual(self.target.read_bytes(), SAVED)
        self.assertEqual(stat.S_IMODE(self.target.stat().st_mode), 0o640)
        self.assertNotEqual(self.target.stat().st_ino, self.original['identity'][1])
        # No stage is left beside it, and the session keeps its files.
        self.assertEqual(sorted(path.name for path in self.target.parent.iterdir()), ['favourites.json'])
        self.assertEqual((self.palettes / 'favourites-0010.json').read_bytes(), SAVED)

    def test_saved_bytes_equal_to_the_file_given_change_nothing(self):
        self.save('favourites-0001.json', PERSONAL)
        before = self.target.stat()
        outcome = self.run_return()
        self.assertFalse(outcome['returned'])
        self.assertEqual(outcome['reason'], 'the saved favourites equal the file given')
        self.assertEqual(self.target.stat().st_ino, before.st_ino)

    def test_a_file_changed_or_replaced_during_the_session_is_never_overwritten(self):
        self.save('favourites-0001.json')
        self.target.write_bytes(PERSONAL + b' ')
        outcome = self.run_return()
        self.assertFalse(outcome['returned'])
        self.assertEqual(self.target.read_bytes(), PERSONAL + b' ')
        self.assertIn('changed during the session', outcome['reason'])
        # The same bytes in another file (an editor that saves by rename).
        other = self.target.with_name('other.json')
        other.write_bytes(PERSONAL)
        os.replace(other, self.target)
        outcome = self.run_return()
        self.assert_kept(outcome, 'favourites-0001.json', 'changed during the session')
        # Replaced by a link to a copy.
        copy = self.root / 'copy.json'
        copy.write_bytes(PERSONAL)
        self.target.unlink()
        self.target.symlink_to(copy)
        outcome = self.run_return()
        self.assertFalse(outcome['returned'])
        self.assertIn('not a link', outcome['reason'])
        self.assertTrue(self.target.is_symlink())
        self.assertEqual(copy.read_bytes(), PERSONAL)

    def test_a_saved_file_that_is_not_the_editors_is_kept_where_it_is(self):
        cases = [
            ({'mode': 0o644}, 'not a private file'),
            ({'data': b'[]'}, 'not a favourites file'),
            ({'data': b'{"format": "other/1"}'}, 'not a favourites file'),
            ({'data': b'\xff'}, 'decode'),
            ({'data': b'{' + b' ' * (64 << 10)}, '64 KiB'),
        ]
        for options, reason in cases:
            with self.subTest(options=options):
                path = self.save('favourites-0001.json', **options)
                self.assert_kept(self.run_return(), 'favourites-0001.json', reason)
                path.unlink()
        link = self.palettes / 'favourites-0001.json'
        link.symlink_to(self.palettes / 'favourites.json')
        self.assert_kept(self.run_return(), 'favourites-0001.json', 'not a link')

    def test_an_entry_at_the_stage_name_is_neither_replaced_nor_removed(self):
        self.save('favourites-0001.json')
        stage = self.target.parent / f'.{self.target.name}.tilewright-{os.getpid()}'
        stage.write_bytes(b'someone else')
        outcome = self.run_return()
        self.assertFalse(outcome['returned'])
        self.assertEqual(stage.read_bytes(), b'someone else')
        self.assertEqual(self.target.read_bytes(), PERSONAL)


if __name__ == '__main__':
    unittest.main()
