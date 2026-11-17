"""Check archive usability and portable asset policy without game data."""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import importlib.util
import json
from pathlib import Path, PurePosixPath, PureWindowsPath
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT/'tools'/filename)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


assets = module('package_assets', 'package-assets.py')
validation = module('check_prototype_assets', 'check-prototype-assets.py')


class PackageAssetTests(unittest.TestCase):
    def test_portable_policy_paths_do_not_depend_on_host_separator(self):
        for kind, root in [(PureWindowsPath, 'C:/repository'), (PurePosixPath, '/repository')]:
            prefix = kind(root)
            self.assertEqual(validation.portable_relative(prefix/'sample'/'tile.atx1', prefix), 'sample/tile.atx1')

    def test_binary_archive_gets_trusted_entry_point_and_only_authored_data(self):
        with tempfile.TemporaryDirectory() as temporary:
            stage = Path(temporary)
            assets.package_assets(ROOT, stage)
            self.assertEqual((stage/'tools/provision-prototype.py').read_bytes(), (ROOT/'tools/provision-prototype.py').read_bytes())
            # The sandbox helper loads the prototype helper from beside it.
            self.assertEqual((stage/'tools/provision-sandbox.py').read_bytes(), (ROOT/'tools/provision-sandbox.py').read_bytes())
            self.assertTrue((stage/'samples/prototype-neutral/world.jsonc').is_file())
            self.assertFalse((stage/'samples/prototype-neutral/generate_assets.py').exists())
            self.assertFalse(any(stage.rglob('*.map')))
            self.assertFalse((stage/'recovery').exists())
            self.assertTrue((stage/'asset-provenance.json').is_file())

    def test_every_sample_file_but_the_generator_is_packaged(self):
        # A fixture file left out of the policy ships a sample that does not
        # open; tools/package-linux.sh also opens the staged sample with the
        # packaged binary.
        policy = json.loads((ROOT/'policy/package-assets.json').read_text())
        listed = set(policy['files'])
        sample = ROOT/assets.PREFIX
        present = {path.relative_to(ROOT).as_posix() for path in sample.rglob('*') if path.is_file()}
        unpackaged = sorted(present - listed - {(assets.PREFIX/'generate_assets.py').as_posix()})
        self.assertEqual(unpackaged, [])

    def test_the_shipped_guides_link_only_to_each_other_or_the_design_records(self):
        # A guide a shipped guide links to ships beside it; the design
        # records are the one place a shipped link leaves the archive.
        with tempfile.TemporaryDirectory() as temporary:
            stage = Path(temporary)
            assets.package_assets(ROOT, stage)
            shipped = {path.name for path in stage.glob('*.md')}
            self.assertEqual(shipped, {'OFFLINE-PROTOTYPE.md', 'SANDBOX.md', 'MCP-EDITING.md', 'PROPOSALS.md', 'ENGINE.md', 'MCP.md', 'CLI.md'})
            for name in assets.DOCS:
                self.assertEqual((stage/name).read_bytes(), (ROOT/'docs'/name).read_bytes())
                text = (stage/name).read_text(encoding='utf-8')
                for target in re.findall(r'\]\(([^)\s]+)\)', text):
                    if re.match(r'[a-z][a-z0-9+.-]*:', target) or target.startswith('#'):
                        continue
                    path = target.split('#', 1)[0]
                    if path.startswith('../decisions/'):
                        self.assertTrue((ROOT/'docs'/path).is_file(), f'{name}: {target}')
                    else:
                        self.assertIn(path, shipped, f'{name} links to {target}, which the package does not ship')

    def test_policy_cannot_include_a_parent_or_external_sample(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root/'policy').mkdir()
            stage = root/'stage'
            stage.mkdir()
            (root/'policy/package-assets.json').write_text(json.dumps({'schema_version':1,'sample':'prototype-neutral','files':['../external.map']}))
            with self.assertRaisesRegex(ValueError, 'outside exact allowlist'):
                assets.package_assets(root, stage)
            self.assertEqual(list(stage.iterdir()), [])

    def test_a_sample_catalog_with_project_art_is_never_packaged(self):
        # Format 2 lets a catalog label resources `project`; the packaged
        # sample's catalog must stay MIT only. The labelled catalog exists
        # only in this temporary copy.
        source = ROOT/assets.PREFIX/'preview.jsonc'
        original = source.read_bytes()
        assets.mit_only_catalog(original)
        catalog = json.loads(original)
        self.assertTrue(catalog['resources'])
        catalog['format'] = 'tilewright.editor-preview/2'
        assets.mit_only_catalog(json.dumps(catalog).encode())
        for index in (0, len(catalog['resources']) - 1):
            for value in ('project', 'Project', 'GPL-2.0', None, 7):
                labelled = json.loads(original)
                labelled['format'] = 'tilewright.editor-preview/2'
                labelled['resources'][index]['license'] = value
                with self.assertRaisesRegex(ValueError, 'only MIT resources'):
                    assets.mit_only_catalog(json.dumps(labelled).encode())
        # A license member anywhere else, or a catalog that is not plain
        # JSON, is refused too.
        with self.assertRaisesRegex(ValueError, 'only MIT resources'):
            assets.mit_only_catalog(json.dumps({**catalog, 'editing': {'license': 'project'}}).encode())
        with self.assertRaises(ValueError):
            assets.mit_only_catalog(b'// comment\n' + original)
        # A key spelled with an escape still names the license.
        escaped = json.dumps(catalog).replace('"license"', '"licen\\u0073e"', 1).replace('"MIT"', '"project"', 1)
        self.assertEqual(json.loads(escaped)['resources'][0]['license'], 'project')
        with self.assertRaisesRegex(ValueError, 'only MIT resources'):
            assets.mit_only_catalog(escaped.encode())
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)/'root'
            sample = root/assets.PREFIX
            sample.mkdir(parents=True)
            (root/'policy').mkdir()
            labelled = json.loads(original)
            labelled['resources'][0]['license'] = 'project'
            (sample/'preview.jsonc').write_text(json.dumps(labelled))
            (root/'policy/package-assets.json').write_text(json.dumps({'schema_version':1,'sample':'prototype-neutral','files':[(assets.PREFIX/'preview.jsonc').as_posix()]}))
            stage = Path(temporary)/'stage'
            stage.mkdir()
            with self.assertRaisesRegex(ValueError, 'only MIT resources'):
                assets.package_assets(root, stage)
            self.assertEqual(list(stage.rglob('*.jsonc')), [])

    def test_no_committed_catalog_declares_project_art(self):
        validation.committed_catalogs_are_mit()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaisesRegex(ValueError, 'no committed editor catalog'):
                validation.committed_catalogs_are_mit(root)
            fixture = root/'crates/some-crate/tests/fixtures/world/preview.jsonc'
            fixture.parent.mkdir(parents=True)
            mit = '{"format": "tilewright.editor-preview/2", "resources": [{"license": "MIT"}]}'
            fixture.write_text(mit)
            validation.committed_catalogs_are_mit(root)
            for text in (
                '{"format": "tilewright.editor-preview/2", "resources": [{"license": "MIT"}, {"license": "project"}]}',
                '{"resources": [{"license": "MIT"}], "license": "project"}',
                '{"resources": [{"license": "MIT"}], "editing": {"objects": [{"defaults": {"license": "GPL-2.0"}}]}}',
                '{"resources": [{"license": 7}]}',
                # An escaped key is the same key.
                '{"resources": [{"licen\\u0073e": "project"}]}',
                # A duplicate key: a reader may keep either value.
                '{"resources": [{"license": "MIT", "license": "project"}]}',
                '{"resources": [{"license": "MIT"}], "resources": []}',
                # Comments, which the editor's JSONC reader accepts, are
                # refused outright: they could hide a key from a search.
                '// c\n{"resources": [{"license": "project"}]}',
                '{"resources": [{"license" /* x */ : "project"}]}',
                '{"resources": [{"license" // x\n : "project"}]}',
                '// only a comment\n' + mit,
            ):
                fixture.write_text(text)
                with self.assertRaisesRegex(ValueError, 'plain JSON declaring only MIT'):
                    validation.committed_catalogs_are_mit(root)

    def test_catalogs_and_resources_are_found_in_every_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name, text in (
                ('crates/x/fixtures/w/preview.jsonc', '{"resources": [{"license": "MIT"}]}'),
                ('docs/examples/world/preview.jsonc', '{"resources": [{"license": "project"}]}'),
            ):
                (root/name).parent.mkdir(parents=True, exist_ok=True)
                (root/name).write_text(text)
            # Outside a Git checkout, the whole tree is walked but build
            # output.
            for skipped in ('target/debug',):
                (root/skipped).mkdir(parents=True)
                (root/skipped/'stray.atx1').write_bytes(b'ATX1')
            files = validation.committed_files(root)
            self.assertIn('docs/examples/world/preview.jsonc', files)
            self.assertFalse(any(name.startswith('target/') for name in files))
            with self.assertRaisesRegex(ValueError, 'docs/examples/world/preview.jsonc'):
                validation.committed_catalogs_are_mit(root)
            validation.committed_resources_are_listed(set(), root)
            (root/'docs/examples/world/art.ATX1').write_bytes(b'ATX1')
            with self.assertRaisesRegex(ValueError, 'outside the asset manifest: docs/examples/world/art.ATX1'):
                validation.committed_resources_are_listed(set(), root)
            validation.committed_resources_are_listed({'docs/examples/world/art.ATX1'}, root)
            # In a Git checkout, what Git tracks or would add, not what it ignores.
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            (root/'.gitignore').write_text('ignored/\n')
            (root/'ignored').mkdir()
            (root/'ignored/x.atx1').write_bytes(b'ATX1')
            files = validation.committed_files(root)
            self.assertIn('docs/examples/world/art.ATX1', files)
            self.assertNotIn('ignored/x.atx1', files)
            with self.assertRaisesRegex(ValueError, 'docs/examples/world/preview.jsonc'):
                validation.committed_catalogs_are_mit(root)

    def test_validate_runs_both_repository_checks(self):
        calls = []
        catalogs, resources = validation.committed_catalogs_are_mit, validation.committed_resources_are_listed
        try:
            validation.committed_catalogs_are_mit = lambda *args, **kwargs: calls.append('catalogs')
            validation.committed_resources_are_listed = lambda *args, **kwargs: calls.append('resources')
            validation.validate()
        finally:
            validation.committed_catalogs_are_mit, validation.committed_resources_are_listed = catalogs, resources
        self.assertEqual(sorted(calls), ['catalogs', 'resources'])
        # And the script itself refuses a repository with a labelled catalog.
        script = (ROOT/'tools/check-prototype-assets.py').read_text()
        self.assertIn("raise SystemExit(f'prototype assets refused: {error}')", script)

if __name__ == '__main__':
    unittest.main()
