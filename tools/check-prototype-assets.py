#!/usr/bin/env python3
"""Verify the exact authored fixture/resource package allowlist."""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PREFIX = 'crates/tilewright-editor-testkit/fixtures/prototype-neutral/preview-assets/'
REQUIRED = {'path', 'source', 'author', 'license', 'sha256', 'transformation', 'notice', 'width', 'height'}


def portable_relative(path, root):
    return path.relative_to(root).as_posix()


# Directories a tree walk skips when the root is not a Git checkout: Git's
# own data and build output, which hold no committed files.
SKIPPED = {'.git', 'target'}


def committed_files(root=ROOT):
    """Every file of the repository at `root`, as portable relative paths.

    In a Git checkout, the files Git tracks and the untracked ones it does
    not ignore (so a new file is checked before it is committed). Elsewhere,
    such as an unpacked source archive, every file under `root` but `.git`
    and `target` directories.
    """
    if (root/'.git').exists():
        listed = subprocess.run(
            ['git', '-C', str(root), 'ls-files', '-z', '--cached', '--others', '--exclude-standard'],
            check=True, capture_output=True).stdout
        return sorted({name for name in listed.decode('utf-8').split('\0') if name})
    files = []
    for directory, subdirectories, names in os.walk(root):
        subdirectories[:] = sorted(name for name in subdirectories if name not in SKIPPED)
        files.extend(portable_relative(Path(directory)/name, root) for name in names)
    return sorted(files)


def mit_only_pairs(pairs):
    """`json.loads` object hook: refuse a duplicate key, and a `license`
    member whose value is not "MIT", in any object at any depth."""
    seen = set()
    for key, value in pairs:
        if key in seen:
            raise ValueError(f'duplicate key {key!r}')
        seen.add(key)
        if key == 'license' and value != 'MIT':
            raise ValueError('catalog declares a resource license other than MIT')
    return dict(pairs)


def committed_catalogs_are_mit(root=ROOT, files=None):
    """Every editor catalog in the repository declares only MIT resources.

    Catalog format 2 lets a world label resources `project`: art a project
    supplies under its own license. Project art never enters this
    repository, so every committed `preview.jsonc`, in any directory, must
    be plain JSON (no comments, which the editor's JSONC reader would also
    accept: they could hide a key from a text search) whose every `license`
    member, at any depth and however its key is escaped, is "MIT", with no
    duplicate keys. Tests label the MIT sample `project` only in temporary
    copies.
    """
    files = committed_files(root) if files is None else files
    # Any case: a file system that ignores case loads `Preview.jsonc` too.
    catalogs = [name for name in files if PurePosixPath(name).name.casefold() == 'preview.jsonc']
    if not catalogs:
        raise ValueError('no committed editor catalog found')
    for name in catalogs:
        try:
            json.loads((root/name).read_bytes(), object_pairs_hook=mit_only_pairs)
        except ValueError as error:
            raise ValueError(f'{name}: committed catalogs must be plain JSON declaring only MIT resources ({error})') from error


def committed_resources_are_listed(listed, root=ROOT, files=None):
    """Every committed ATX1 resource, in any directory, is one the asset
    manifest lists: the neutral sample's authored art."""
    files = committed_files(root) if files is None else files
    stray = sorted(name for name in files if name.lower().endswith('.atx1') and name not in listed)
    if stray:
        raise ValueError(f'ATX1 resource outside the asset manifest: {stray[0]}')


def validate():
    manifest = json.loads((ROOT / 'provenance/assets.json').read_text())
    if manifest.get('schema_version') != 1 or set(manifest) != {'schema_version', 'assets'}:
        raise ValueError('unsupported asset manifest')
    seen = set()
    for record in manifest['assets']:
        if not REQUIRED <= record.keys():
            raise ValueError('incomplete asset provenance')
        path = record['path']
        if not isinstance(path, str) or not path.startswith(PREFIX) or path in seen:
            raise ValueError('asset outside exact authored sample allowlist')
        relative = Path(path)
        if relative.is_absolute() or '..' in relative.parts or relative.suffix != '.atx1':
            raise ValueError('invalid resource path')
        seen.add(path)
        if record['license'] != 'MIT' or not all(isinstance(record[key], str) and record[key] for key in ('source', 'author', 'transformation', 'notice')):
            raise ValueError('authored fixture provenance is incomplete or incompatible')
        target = ROOT / relative
        if target.is_symlink() or not target.is_file() or target.resolve() != target:
            raise ValueError('asset must be a regular repository file without links')
        data = target.read_bytes()
        if len(data) < 12 or data[:4] != b'ATX1' or hashlib.sha256(data).hexdigest() != record['sha256']:
            raise ValueError('asset format/digest mismatch')
        width, height = struct.unpack('<II', data[4:12])
        if width != record['width'] or height != record['height'] or not 1 <= width <= 512 or not 1 <= height <= 512 or len(data) != 12 + width * height * 4:
            raise ValueError('asset dimensions/payload mismatch')
    actual = {portable_relative(path, ROOT) for path in (ROOT / PREFIX).glob('*') if path.is_file()}
    if actual != seen or not seen:
        raise ValueError('asset allowlist does not exactly match authored resources')
    files = committed_files()
    committed_resources_are_listed(seen, files=files)
    committed_catalogs_are_mit(files=files)


if __name__ == '__main__':
    try:
        validate()
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        raise SystemExit(f'prototype assets refused: {error}') from error
