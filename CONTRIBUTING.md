# Contributing

Use MIT code and synthetic or separately licensed, manifested fixtures.
Independent implementation is the default. Reusing outside material requires an
MIT-compatible license and an exact record in [PROVENANCE.md](PROVENANCE.md);
GPL/AGPL source must not be copied, adapted, or depended on, and observing
another program's behavior does not authorize reusing its source.

Direct human-written code contributions are welcome. They follow the same
provenance, maintainer review, tests, and repository validation as code
developed through the project's primary agent-driven workflows; using an agent
is not a contribution requirement.

Keep authority in the owning released dependency. New editor code must not add
a parser, writer, renderer, server/client/protocol edge, direct filesystem write
from UI code, or manifest-local sibling override. Run `tools/validate.sh` and
use a Conventional Commit message and PR title.
