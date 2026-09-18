param(
    [switch]$SkipBuild,
    [switch]$SkipPublish
)

& (Join-Path $PSScriptRoot "update.ps1") -SkipBuild:$SkipBuild -SkipPublish:$SkipPublish

