# Mutable path ownership

| Path class | Owner | Mutation contract | Recovery/user visibility |
| --- | --- | --- | --- |
| Authored source roots | world toolkit transaction publisher | explicit semantic command, expected revision, validation, dry-run diff, canonical revalidation, atomic publish | dirty/conflict/diff/save state visible per document |
| Project/editor preferences | editor project service | bounded typed update, atomic editor-owned storage | reset/export visible; never mixed with authored source |
| Prototype recovery | editor recovery service over retained World storage | append fresh exact inverse outside project; bounded records/bytes; no replacement or automatic deletion | record name/digest and applied/durable/cleaned status; nonempty startup refused; explicit inspection |
| Agent sandbox (`world/`, `recovery/`, `proposals/`) | sandbox provisioning helper; then the headless runner over the descriptors it is given | fresh uniquely named private copy per run; Accept publishes into its `world/`, recovery as above, proposal files through the proposal store, same bytes once; never reused or deleted | receipt and paths on standard error; proposal file name and digest in the `submit` reply; a person applies a proposal only through Review and Accept |
| Generated/collected/build output | world toolkit compiler | replace versioned output root, never treated as source | build result and provenance visible |
| Renderer cache | renderer resource provider | digest/revision keyed and bounded | safe eviction; diagnostics only |
| Game build/runtime/state/logs | the consuming game's workspace tooling | that game's profile/topology/state contracts only | the game's own status and log commands |
| Credentials/trust | owning OS/wrapper service | never an editor document or snapshot | never rendered, logged, recovered, or packaged |

No panel, preview adapter, SDL callback or application shell owns an alternate
filesystem writer. Source publication, recovery retention and evidence writes
use World retained-root storage primitives. The trusted prototype provisioning
helper creates fresh private task directories, copies bounded regular World data
and establishes persistent mount/separation evidence before launching the app;
it never writes the selected original source tree. See
[the bounded deployment contract](OFFLINE-PROTOTYPE.md). Its sandbox mode
does the same for an agent's run ([agent sandboxes](SANDBOX.md)).
