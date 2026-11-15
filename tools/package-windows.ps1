param(
    [string]$Output = "dist",
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$FontPack
)
$ErrorActionPreference = "Stop"
if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+([.+-][0-9A-Za-z.-]+)?$') { throw "invalid release version" }
if (Test-Path $Output) { throw "release output already exists: $Output" }
foreach ($Command in @("cargo-auditable", "syft", "python")) {
    if (-not (Get-Command $Command -ErrorAction SilentlyContinue)) { throw "missing required tool: $Command" }
}
cargo auditable build --locked --release --package tilewright-editor
$Metadata = cargo metadata --locked --offline --format-version 1 --no-deps | ConvertFrom-Json
$StageRoot = Join-Path $env:RUNNER_TEMP ([System.IO.Path]::GetRandomFileName())
$Stage = Join-Path $StageRoot "tilewright-editor-$Version"
try {
    New-Item -ItemType Directory -Path "$Stage/bin", $Output | Out-Null
    Copy-Item (Join-Path $Metadata.target_directory "release/tilewright-editor.exe") "$Stage/bin/"
    Copy-Item LICENSE, PROVENANCE.md, THIRD_PARTY_NOTICES.md $Stage
    python tools/check-prototype-assets.py
    if ($LASTEXITCODE -ne 0) { throw "sample asset verification failed" }
    python tools/package-assets.py --root . --stage $Stage
    if ($LASTEXITCODE -ne 0) { throw "sample package allowlist failed" }
    python tools/package-font-notice.py --root . --stage $Stage | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "font package notice verification failed" }
    python tools/prepare-font-pack.py stage --root . --font-pack $FontPack --stage $Stage | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Unicode font pack staging failed" }
    & "$Stage/bin/tilewright-editor.exe" version | Out-Null
    # The packaged sample must open as a World with the packaged binary.
    & "$Stage/bin/tilewright-editor.exe" open "$Stage/samples/prototype-neutral" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "the packaged sample does not open" }
    # The read-only commands need no editing support: the packaged sample's
    # catalog and art load, and lint finds no error in it.
    & "$Stage/bin/tilewright-editor.exe" overview --json "$Stage/samples/prototype-neutral" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "overview of the packaged sample failed" }
    & "$Stage/bin/tilewright-editor.exe" lint "$Stage/samples/prototype-neutral" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "lint of the packaged sample failed" }
    # render writes one PNG, outside the sample and the staged package: its
    # signature and size must be what its report says.
    $RenderSmoke = Join-Path $StageRoot "render-smoke"
    New-Item -ItemType Directory -Path $RenderSmoke | Out-Null
    $Report = & "$Stage/bin/tilewright-editor.exe" render --json --out "$RenderSmoke/sample.png" "$Stage/samples/prototype-neutral" | Out-String | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw "render of the packaged sample failed" }
    $Png = [System.IO.File]::ReadAllBytes("$RenderSmoke/sample.png")
    $Signature = [byte[]](0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A)
    if ([Convert]::ToHexString([byte[]]$Png[0..7]) -ne [Convert]::ToHexString($Signature)) { throw "render did not write a PNG" }
    $Width = [System.Buffers.Binary.BinaryPrimitives]::ReadUInt32BigEndian([byte[]]$Png[16..19])
    $Height = [System.Buffers.Binary.BinaryPrimitives]::ReadUInt32BigEndian([byte[]]$Png[20..23])
    if ($Width -ne 256 -or $Height -ne 192 -or $Report.image.width -ne $Width -or $Report.image.height -ne $Height -or $Report.png.bytes -ne $Png.Length) {
        throw "the rendered sample does not match its report"
    }
    if ((Get-ChildItem -Force $RenderSmoke).Count -ne 1) { throw "render wrote more than its one file" }
    # The MCP server answers over stdio with the packaged binary: discovery,
    # and render, whose image is the PNG render wrote, byte for byte.
    $McpMeta = '"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}'
    $McpInput = @(
        "{""jsonrpc"":""2.0"",""id"":1,""method"":""server/discover"",""params"":{$McpMeta}}",
        "{""jsonrpc"":""2.0"",""id"":2,""method"":""tools/call"",""params"":{""name"":""render"",""arguments"":{},$McpMeta}}"
    ) -join "`n"
    $McpReplies = @($McpInput | & "$Stage/bin/tilewright-editor.exe" mcp "$Stage/samples/prototype-neutral" 2>$null | ForEach-Object { $_ | ConvertFrom-Json })
    if ($LASTEXITCODE -ne 0) { throw "the MCP server of the packaged binary failed" }
    if ($McpReplies.Count -ne 2 -or $McpReplies[0].id -ne 1 -or $McpReplies[1].id -ne 2) { throw "the MCP server did not answer each request once" }
    if ($McpReplies[0].result.supportedVersions[0] -ne "2026-07-28") { throw "the MCP server does not serve revision 2026-07-28" }
    $McpImage = $McpReplies[1].result.content[1]
    if ($McpReplies[1].result.isError -or $McpImage.type -ne "image" -or $McpImage.mimeType -ne "image/png") { throw "MCP render returned no PNG" }
    if ([Convert]::ToHexString([Convert]::FromBase64String($McpImage.data)) -ne [Convert]::ToHexString($Png)) { throw "the MCP image is not the PNG render writes" }
    syft "$Stage/bin/tilewright-editor.exe" --source-name tilewright-editor --source-version $Version --output "cyclonedx-json=$Stage/sbom.cdx.json"
    $Sbom = Get-Content "$Stage/sbom.cdx.json" -Raw | ConvertFrom-Json
    if ($Sbom.components.Count -lt 10) { throw "binary SBOM is incomplete" }
    $Digest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes("tilewright-editor:windows-amd64:$Version"))).ToLowerInvariant()
    $Sbom.serialNumber = "urn:uuid:$($Digest.Substring(0, 8))-$($Digest.Substring(8, 4))-8$($Digest.Substring(13, 3))-8$($Digest.Substring(17, 3))-$($Digest.Substring(20, 12))"
    $Sbom.metadata.timestamp = "1970-01-01T00:00:00Z"
    $Sbom.metadata.component.'bom-ref' = "tilewright-editor-windows-amd64@$Version"
    foreach ($Component in $Sbom.components) {
        if ($Component.type -eq "file") { $Component.name = "/tilewright-editor.exe" }
    }
    $Sbom | ConvertTo-Json -Depth 20 -Compress | Set-Content "$Stage/sbom.cdx.json" -Encoding utf8NoBOM
    python tools/prepare-font-pack.py sbom --root . --stage $Stage --sbom "$Stage/sbom.cdx.json" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Unicode font pack SBOM verification failed" }
    python tools/package-provenance.py --root . --version $Version --output "$Stage/provenance.json"
    if ($LASTEXITCODE -ne 0) { throw "package dependency provenance failed" }
    $Archive = Join-Path $Output "tilewright-editor-$Version-windows-amd64.zip"
    Compress-Archive -Path $Stage -DestinationPath $Archive
    (Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLowerInvariant() + "  " + (Split-Path $Archive -Leaf) | Set-Content (Join-Path $Output "SHA256SUMS.windows") -Encoding ascii
} finally {
    if (Test-Path $StageRoot) { Remove-Item -Recurse -Force $StageRoot }
}
