#!/usr/bin/env python3
"""Copy only exact allowlisted original sample data into a fresh package stage."""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import argparse
import json
from pathlib import Path
import re
import shutil

PREFIX = Path('crates/tilewright-editor-testkit/fixtures/prototype-neutral')
# The guides a package ships, at its top level: the prototype and sandbox
# guides and every page they link to, so that no link between shipped
# guides leaves the archive. Links to the design records (`../decisions/`)
# still do; those records are not shipped.
DOCS = ('OFFLINE-PROTOTYPE.md', 'SANDBOX.md', 'MCP-EDITING.md', 'PROPOSALS.md', 'ENGINE.md', 'MCP.md', 'CLI.md')


LICENSE = re.compile(rb'"license"\s*:\s*("(?:[^"\\]|\\.)*"|[^\s,}\]]+)')


def mit_only_catalog(data):
    """Refuse a sample catalog that declares any resource license but MIT.

    A catalog of format 2 may label resources `project`: art a project
    supplies under its own license. Such art never ships, so the packaged
    sample's catalog must be plain JSON whose every resource is MIT, and
    every `license` member anywhere in its text must be `"MIT"`.
    """
    catalog = json.loads(data)
    resources = catalog.get('resources') if isinstance(catalog, dict) else None
    if not isinstance(resources, list) or not all(isinstance(resource, dict) and resource.get('license') == 'MIT' for resource in resources):
        raise ValueError('packaged sample catalog must declare only MIT resources')
    if any(value != b'"MIT"' for value in LICENSE.findall(data)):
        raise ValueError('packaged sample catalog must declare only MIT resources')


def package_assets(root, stage):
    manifest = json.loads((root/'policy/package-assets.json').read_text())
    if set(manifest) != {'schema_version', 'sample', 'files'} or manifest['schema_version'] != 1 or manifest['sample'] != 'prototype-neutral':
        raise ValueError('unsupported sample package policy')
    seen = set()
    for value in manifest['files']:
        path = Path(value)
        if not isinstance(value, str) or value in seen or path.is_absolute() or '..' in path.parts or not path.is_relative_to(PREFIX):
            raise ValueError('sample package path outside exact allowlist')
        if path.suffix not in ('.jsonc', '.atx1') and path.name not in ('COPYING', 'README.md'):
            raise ValueError('package policy includes executable or unsupported data')
        seen.add(value)
        source = root/path
        if source.is_symlink() or not source.is_file() or source.resolve() != source:
            raise ValueError('package sample contains a link or nonregular file')
        if path.name == 'preview.jsonc':
            mit_only_catalog(source.read_bytes())
        output = stage/'samples/prototype-neutral'/path.relative_to(PREFIX)
        output.parent.mkdir(parents=True, exist_ok=True)
        with source.open('rb') as incoming, output.open('xb') as outgoing:
            shutil.copyfileobj(incoming, outgoing)
    if not seen:
        raise ValueError('sample package allowlist is empty')
    for source, target in [('provenance/assets.json','asset-provenance.json'), *((f'docs/{name}', name) for name in DOCS), ('tools/provision-prototype.py','tools/provision-prototype.py'), ('tools/provision-sandbox.py','tools/provision-sandbox.py'), ('tools/font_pack.py','tools/font_pack.py')]:
        (stage/target).parent.mkdir(parents=True, exist_ok=True)
        with (root/source).open('rb') as incoming, (stage/target).open('xb') as outgoing:
            shutil.copyfileobj(incoming, outgoing)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path('.'))
    parser.add_argument('--stage', type=Path, required=True)
    args = parser.parse_args()
    try:
        package_assets(args.root.resolve(strict=True), args.stage.resolve(strict=True))
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise SystemExit(f'package sample refused: {error}') from error
