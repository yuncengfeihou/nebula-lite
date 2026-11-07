<#
.SYNOPSIS
    取消 install-default-app.ps1 注册的默认应用候选。

.DESCRIPTION
    只清理 HKCU 下由安装脚本写入的东西：

      * `HKCU\Software\NebulaLite`（能力清单与本次注册的扩展名名单）；
      * `HKCU\Software\RegisteredApplications` 的 NebulaLite 值；
      * 每个扩展名 OpenWithProgids 里的 NebulaLite.Document 值；
      * `HKCU\Software\Classes\NebulaLite.Document` 与
        `HKCU\Software\Classes\Applications\<exe>`。

    **不碰 UserChoice**：如果某个扩展名当前仍以 nebula-lite 为默认，删掉 ProgID
    会让它打不开。遇到这种情况会保留 ProgID / Applications 项，只从「默认应用」
    候选里移除，并提示你先去系统设置里改回别的程序。

    必须用 pwsh（PowerShell 7）运行。卸载前不需要管理员权限。
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$base = [Microsoft.Win32.Registry]::CurrentUser
$appKeyName = 'NebulaLite'
$capabilities = 'Software\NebulaLite\Capabilities'
$defaultProgId = 'NebulaLite.Document'
$defaultExeName = 'nebula-lite.exe'
$defaultExtensions = @(
    '.txt', '.md', '.markdown', '.log', '.ini', '.cfg', '.conf', '.toml', '.yaml', '.yml'
)

# 先读安装时留下的名单；没有就按当前版本的默认名单清。
$progId = $defaultProgId
$exeName = $defaultExeName
$extensions = $defaultExtensions
$info = $base.OpenSubKey('Software\NebulaLite')
if ($null -ne $info) {
    $savedProgId = $info.GetValue('ProgId')
    if ($savedProgId) { $progId = [string]$savedProgId }
    $savedExe = $info.GetValue('ExeName')
    if ($savedExe) { $exeName = [string]$savedExe }
    $savedExtensions = $info.GetValue('RegisteredExtensions')
    if ($savedExtensions) { $extensions = [string[]]$savedExtensions }
    $info.Close()
}

# 某些扩展名可能仍以我们为默认（UserChoice 指向 ProgID 或 Applications\<exe>）。
$inUse = @()
foreach ($ext in $extensions) {
    $choice = $base.OpenSubKey("Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\$ext\UserChoice")
    if ($null -ne $choice) {
        $current = [string]$choice.GetValue('ProgId')
        $choice.Close()
        if ($current -ieq $progId -or $current -ieq "Applications\$exeName") {
            $inUse += $ext
        }
    }
}

# 从每个扩展名的「打开方式」候选里移除我们。
foreach ($ext in $extensions) {
    $openWithPath = "Software\Classes\$ext\OpenWithProgids"
    $openWith = $base.OpenSubKey($openWithPath, $true)
    if ($null -eq $openWith) { continue }

    $names = @($openWith.GetValueNames())
    if ($names -contains $progId) {
        $openWith.DeleteValue($progId, $false)
        Write-Host "已移除打开方式候选：$ext" -ForegroundColor Green
    }
    $empty = ($openWith.ValueCount -eq 0 -and $openWith.SubKeyCount -eq 0)
    $openWith.Close()
    if ($empty) {
        try { $base.DeleteSubKey($openWithPath, $false) } catch [System.ArgumentException] { }
    }
}

# 从「默认应用」注册表里移除应用名。
$registered = $base.OpenSubKey('Software\RegisteredApplications', $true)
if ($null -ne $registered) {
    if ($registered.GetValue($appKeyName) -eq $capabilities) {
        $registered.DeleteValue($appKeyName, $false)
        Write-Host '已移除注册应用：NebulaLite' -ForegroundColor Green
    }
    $registered.Close()
}

# 能力清单整棵删除（这里只放我们自己的东西）。
try {
    $base.DeleteSubKeyTree('Software\NebulaLite', $false)
    Write-Host '已删除 HKCU\Software\NebulaLite' -ForegroundColor Green
}
catch [System.ArgumentException] { }

if ($inUse.Count -gt 0) {
    Write-Host ''
    Write-Host '注意：这些扩展名现在仍以 nebula-lite 为默认，已保留 ProgID 以免打不开：' -ForegroundColor Yellow
    Write-Host ("  {0}" -f ($inUse -join '  ')) -ForegroundColor Yellow
    Write-Host '请先在「设置 → 应用 → 默认应用」里把它们改回别的程序，再重新运行本脚本。' -ForegroundColor Gray
}
else {
    foreach ($sub in @("Software\Classes\$progId", "Software\Classes\Applications\$exeName")) {
        try {
            $base.DeleteSubKeyTree($sub, $false)
            Write-Host "已删除 HKCU\$sub" -ForegroundColor Green
        }
        catch [System.ArgumentException] { }
    }
}

Write-Host ''
Write-Host '完成。资源管理器右键菜单是另一套脚本，不受影响。' -ForegroundColor Gray
