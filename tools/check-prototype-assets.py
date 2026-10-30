#!/usr/bin/env python3
"""Verify the exact authored fixture/resource package allowlist."""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import hashlib
import json
from pathlib import Path
import struct

ROOT = Path(__file__).resolve().parents[1]
PREFIX = 'crates/tilewright-editor-testkit/fixtures/prototype-neutral/preview-assets/'
REQUIRED = {'path', 'source', 'author', 'license', 'sha256', 'transformation', 'notice', 'width', 'height'}


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
    actual = {str(path.relative_to(ROOT)) for path in (ROOT / PREFIX).glob('*') if path.is_file()}
    if actual != seen or not seen:
        raise ValueError('asset allowlist does not exactly match authored resources')


if __name__ == '__main__':
    try:
        validate()
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise SystemExit(f'prototype assets refused: {error}') from error
