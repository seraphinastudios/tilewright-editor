# Security policy

Report vulnerabilities privately through GitHub's security advisory interface.
Do not include project source, credentials, private assets, or recovery data in
a public issue.

Supported releases are the latest GitHub release. Path traversal, symlink or
canonical-root escape, stale/external overwrite, partial transaction, project
code execution, dependency/source substitution, unbounded authored input, and
server control from the editor are security defects. So is any write, lock,
editing session or provisioning reached through the read-only commands or the
MCP server (`docs/MCP.md`), and any world text that leaves them unescaped or
unmarked. See `docs/THREAT_MODEL.md`.
