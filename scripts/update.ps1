param(
    [switch]$SkipBuild,
    [switch]$SkipPublish,
    [switch]$NoPause
)

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$root = Split-Path -Parent $PSScriptRoot
$updateFile = Join-Path $root "update.txt"
$envFile = Join-Path $PSScriptRoot ".update-server.env"
$utf8 = New-Object System.Text.UTF8Encoding $false

function Write-Utf8File([string]$Path, [string]$Content) {
    [System.IO.File]::WriteAllText($Path, $Content, $utf8)
}

function Read-UpdateFile([string]$Path) {
    if (-not (Test-Path $Path)) {
        throw "找不到 $Path，请按「版本号 更新日志」逐行记录，脚本只读取最后一行"
    }

    $last = $null
    foreach ($line in [System.IO.File]::ReadAllLines($Path, [System.Text.UTF8Encoding]::new($false))) {
        $trim = $line.TrimStart([char]0xFEFF).Trim()
        if ($trim -eq "" -or $trim.StartsWith("#")) {
            continue
        }
        $last = $trim
    }

    if (-not $last) {
        throw "update.txt 没有有效记录，请追加一行：0.1.1 本次更新说明"
    }
    if ($last -notmatch "^(\d+\.\d+\.\d+)\s+(.+)$") {
        throw "最后一行格式不正确：$last`n请写成：版本号 更新日志    例如 0.1.1 修复分支切换"
    }

    return [pscustomobject]@{
        Version = $Matches[1]
        Notes   = $Matches[2].Trim()
    }
}

function Set-FirstMatch([string]$Path, [string]$Pattern, [string]$Replacement, [string]$Version) {
    $content = [System.IO.File]::ReadAllText($Path)
    $updated = [regex]::Replace($content, $Pattern, $Replacement, 1)
    if ($updated -eq $content -and $content -notmatch [regex]::Escape($Version)) {
        throw "未能更新版本号：$Path"
    }
    Write-Utf8File $Path $updated
}

function Sync-ProjectVersion([string]$Version) {
    Set-FirstMatch (Join-Path $root "src-tauri\Cargo.toml") '(?m)^version\s*=\s*"[^"]+"' "version = `"$Version`"" $Version
    Set-FirstMatch (Join-Path $root "src-tauri\tauri.conf.json") '("version"\s*:\s*")[^"]+"' "`${1}$Version`"" $Version
    Set-FirstMatch (Join-Path $root "package.json") '("version"\s*:\s*")[^"]+"' "`${1}$Version`"" $Version
}

function Get-ReleaseDir {
    $dir = Join-Path $root "release"
    if (-not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir | Out-Null
    }
    return $dir
}

function Copy-LocalInstaller([System.IO.FileInfo]$Installer) {
    $dir = Get-ReleaseDir
    $dest = Join-Path $dir $Installer.Name
    Copy-Item -Path $Installer.FullName -Destination $dest -Force
    Write-Host "  已保留本地安装包：$dest"
    return Get-Item $dest
}

function Find-Installer([string]$Version) {
    $nsis = Join-Path $root "target\release\bundle\nsis"
    $exact = Join-Path $nsis "DBX_${Version}_x64-setup.exe"
    if (Test-Path $exact) {
        return (Get-Item $exact)
    }
    if (Test-Path $nsis) {
        $found = Get-ChildItem $nsis -Filter "*${Version}*setup.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if ($found) {
            return $found
        }
    }
    return $null
}

function Import-UpdateEnv {
    if (Test-Path $envFile) {
        Get-Content $envFile | ForEach-Object {
            if ($_ -match "^\s*#" -or $_ -notmatch "=") { return }
            $pair = $_ -split "=", 2
            Set-Item -Path "Env:$($pair[0].Trim())" -Value $pair[1].Trim()
        }
    }
}

function Publish-Installer([string]$Installer, [string]$Version, [string]$Notes) {
    Import-UpdateEnv

    $hostName = if ($env:UPDATE_HOST) { $env:UPDATE_HOST } else { "111.230.247.111" }
    $user = if ($env:UPDATE_USER) { $env:UPDATE_USER } else { "root" }
    $pass = $env:UPDATE_PASS
    $remoteDir = if ($env:UPDATE_DIR) { $env:UPDATE_DIR } else { "/usr/share/nginx/html/dbx" }

    if (-not $pass) {
        throw "请先在 scripts/.update-server.env 中填写 UPDATE_PASS（该文件已加入 gitignore）"
    }
    if (-not (Test-Path $Installer)) {
        throw "找不到安装包：$Installer"
    }

    $exe = Get-Item $Installer
    $hash = (Get-FileHash -Algorithm SHA256 $exe.FullName).Hash.ToLower()
    $fileName = $exe.Name
    $feedUrl = "http://$hostName/dbx/$fileName"
    $manifestPath = Join-Path $env:TEMP "dbx-latest.json"
    $manifestObj = [ordered]@{
        version    = $Version
        notes      = $Notes
        url        = $feedUrl
        sha256     = $hash
        silentArgs = "/S /UPDATE /R"
    }
    $json = ($manifestObj | ConvertTo-Json -Compress)
    $utf8NoBom = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::WriteAllText($manifestPath, $json, $utf8NoBom)

    Write-Host "上传 $fileName 和 latest.json 到 ${user}@${hostName}:$remoteDir"

    $env:UPDATE_HOST = $hostName
    $env:UPDATE_USER = $user
    $env:UPDATE_PASS = $pass
    $env:UPDATE_DIR = $remoteDir
    $env:UPDATE_VERSION = $Version
    $env:UPDATE_NOTES = $Notes
    $env:UPDATE_URL = $feedUrl
    $env:UPDATE_SHA256 = $hash
    $uploader = Join-Path $PSScriptRoot "upload-update.py"
    python $uploader $exe.FullName $manifestPath $fileName
    if ($LASTEXITCODE -ne 0) {
        throw "上传失败，退出码 $LASTEXITCODE"
    }

    $checkUrl = "http://$hostName/dbx/$fileName"
    try {
        $head = Invoke-WebRequest -Uri $checkUrl -Method Head -UseBasicParsing -TimeoutSec 20
        if ([int]$head.StatusCode -ge 400) {
            throw "HTTP $($head.StatusCode)"
        }
        Write-Host "安装包地址可访问：$checkUrl"
    } catch {
        throw "安装包已上传，但 HTTP 无法下载 $checkUrl ：$($_.Exception.Message)"
    }
    Write-Host "清单地址：http://$hostName/dbx/latest.json"
}

function Wait-ScriptWindow {
    if ($NoPause) { return }
    if (-not [Environment]::UserInteractive) { return }
    try {
        if ([Console]::IsInputRedirected) { return }
    } catch {
        # 控制台不可用时仍尽量停住，避免窗口一闪而过
    }
    Write-Host ""
    Write-Host "执行结束。请查看上方输出，按 Enter 后关闭窗口。"
    try {
        [void][Console]::ReadLine()
    } catch {
        cmd /c pause
    }
}

$script:exitCode = 0
try {
    $info = Read-UpdateFile $updateFile
    Write-Host "读取 update.txt：版本 $($info.Version)"
    Write-Host "更新日志："
    Write-Host $info.Notes
    Write-Host ""

    Write-Host "1/3 同步项目版本号"
    Sync-ProjectVersion $info.Version
    Write-Host "  已写入 Cargo.toml / tauri.conf.json / package.json -> $($info.Version)"

    if (-not $SkipBuild) {
        Write-Host ""
        Write-Host "2/3 打包 NSIS 安装包（pnpm tauri build --bundles nsis）"
        Push-Location $root
        try {
            pnpm tauri build --bundles nsis
            if ($LASTEXITCODE -ne 0) {
                throw "打包失败，退出码 $LASTEXITCODE"
            }
        } finally {
            Pop-Location
        }
    } else {
        Write-Host ""
        Write-Host "2/3 已跳过打包"
    }

    $installer = Find-Installer $info.Version
    if ($installer) {
        Write-Host "  编译产物：$($installer.FullName)"
        $installer = Copy-LocalInstaller $installer
    } elseif ($SkipPublish) {
        Write-Host "  尚未生成安装包，已跳过查找"
    } else {
        throw "未找到版本 $($info.Version) 的安装包，请确认打包成功：target\release\bundle\nsis\"
    }

    if (-not $SkipPublish) {
        Write-Host ""
        Write-Host "3/3 发布到更新服务器"
        Publish-Installer $installer.FullName $info.Version $info.Notes
    } else {
        Write-Host ""
        Write-Host "3/3 已跳过发布"
    }

    Write-Host ""
    Write-Host "完成。本地安装包在 release\ 目录，不会随上传删除。"
    Write-Host "用户将看到版本 $($info.Version) 和 update.txt 中的更新日志。"
} catch {
    $script:exitCode = 1
    Write-Host ""
    Write-Host "失败：$($_.Exception.Message)" -ForegroundColor Red
    if ($_.ScriptStackTrace) {
        Write-Host $_.ScriptStackTrace -ForegroundColor DarkRed
    }
} finally {
    Wait-ScriptWindow
}

exit $script:exitCode
