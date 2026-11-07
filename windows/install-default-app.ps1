<#
.SYNOPSIS
    把 nebula-lite 注册为「默认应用」候选，并打开 Windows 默认应用设置页。

.DESCRIPTION
    只写 **HKCU**（当前用户），不需要管理员权限，也不碰 UserChoice。

    Windows 10/11 **不允许应用静默修改已有文件的默认打开方式**：UserChoice 带
    微软的哈希保护，只能由用户在系统设置里确认。所以这里做的是**注册**：

      * `HKCU\Software\NebulaLite\Capabilities` —— 应用名、说明、文件关联清单；
      * `HKCU\Software\RegisteredApplications` —— 让「默认应用」页认识这个应用；
      * `HKCU\Software\Classes\NebulaLite.Document` —— 打开命令与图标；
      * 每个扩展名的 `OpenWithProgids` —— 让 nebula-lite 出现在「打开方式」列表；
      * `HKCU\Software\Classes\Applications\<exe>` —— 帮助资源管理器识别它。

    注册完会打开 `ms-settings:defaultapps?registeredAppUser=NebulaLite`（Windows 11
    直达该应用；Windows 10 会停在默认应用主页），由用户点几下把扩展名指到
    nebula-lite。脚本**不会**、也无法静默改掉现有的 UserChoice。

    必须用 **pwsh（PowerShell 7）** 运行：本文件是 UTF-8 无 BOM，Windows
    PowerShell 5.1 会按 ANSI 读它，中文会变成乱码。同目录的 .cmd 已调好 pwsh。

.EXAMPLE
    pwsh -File .\install-default-app.ps1
    pwsh -File .\install-default-app.ps1 -Extensions 'txt','md','log' -NoSettings
    pwsh -File .\install-default-app.ps1 -ExePath "D:\tools\nebula-lite.exe"
#>
[CmdletBinding()]
param(
    # nebula-lite.exe 的路径。默认取工作区的 release 产物（见 .cargo/config.toml）。
    [string]$ExePath = 'D:\dev-cache\cargo-target\release\nebula-lite.exe',

    # 要注册的扩展名（带不带点都可）。默认是日常文本类；代码文件不自动接管。
    [string[]]$Extensions = @(
        '.txt', '.md', '.markdown', '.log', '.ini', '.cfg', '.conf', '.toml', '.yaml', '.yml'
    ),

    # 只注册，不打开系统设置页（给自动化 / 批量部署用）。
    [switch]$NoSettings
)

$ErrorActionPreference = 'Stop'

# 命令行里 `-Extensions txt,md` 有时会作为一个整串传进来：统一按逗号拆开。
$Extensions = @($Extensions | ForEach-Object { $_ -split ',' } | Where-Object { $_.Trim() -ne '' })

if (-not (Test-Path -LiteralPath $ExePath -PathType Leaf)) {
    Write-Host "找不到 nebula-lite.exe：$ExePath" -ForegroundColor Red
    Write-Host '用 -ExePath 指定实际路径，例如：' -ForegroundColor Yellow
    Write-Host '  pwsh -File .\install-default-app.ps1 -ExePath "D:\somewhere\nebula-lite.exe"'
    exit 1
}
$ExePath = (Resolve-Path -LiteralPath $ExePath).ProviderPath
$exeName = [System.IO.Path]::GetFileName($ExePath)

# 规范化扩展名：小写、补点、去重；拒绝路径分隔符（防止写到别的键里去）。
$normalized = @()
foreach ($ext in $Extensions) {
    $e = $ext.Trim().ToLowerInvariant()
    if ($e -eq '') { continue }
    if (-not $e.StartsWith('.')) { $e = ".$e" }
    if ($e -match '[\\/\*\?\:\<\>\|]' -or $e.Length -lt 2) {
        throw "扩展名不合法：$ext"
    }
    if ($normalized -notcontains $e) { $normalized += $e }
}
if ($normalized.Count -eq 0) {
    throw '没有可注册的扩展名。'
}

$base = [Microsoft.Win32.Registry]::CurrentUser
$appKeyName = 'NebulaLite'
$progId = 'NebulaLite.Document'
$classes = 'Software\Classes'
$capabilities = 'Software\NebulaLite\Capabilities'
$command = "`"$ExePath`" `"%1`""
$icon = "`"$ExePath`",0"
$none = [Microsoft.Win32.RegistryValueKind]::None
$empty = [byte[]]::new(0)

# —— 1. ProgID：右键「打开」实际执行的命令 ——
$prog = $base.CreateSubKey("$classes\$progId")
$prog.SetValue('', 'nebula-lite 文档')
$prog.SetValue('FriendlyTypeName', 'nebula-lite 文档')
$prog.SetValue('DefaultIcon', $icon)
$shell = $prog.CreateSubKey('shell\open\command')
$shell.SetValue('', $command)
$shell.Close()
$prog.Close()

# —— 2. 每个扩展名：加入「打开方式」候选，不覆盖默认值 ——
foreach ($ext in $normalized) {
    $openWith = $base.CreateSubKey("$classes\$ext\OpenWithProgids")
    $openWith.SetValue($progId, $empty, $none)
    $openWith.Close()
}

# —— 3. 应用注册信息：让「默认应用」页列出 nebula-lite ——
$cap = $base.CreateSubKey($capabilities)
$cap.SetValue('ApplicationName', 'nebula-lite')
$cap.SetValue('ApplicationDescription', '轻量本地文本编辑器：Markdown / 代码 / 图片预览')
$cap.SetValue('ApplicationIcon', $icon)
$assoc = $cap.CreateSubKey('FileAssociations')
foreach ($ext in $normalized) {
    $assoc.SetValue($ext, $progId)
}
$assoc.Close()
$cap.Close()

# 记住本次注册了哪些扩展名，卸载脚本按这份名单清理。
$root = $base.CreateSubKey('Software\NebulaLite')
$root.SetValue('RegisteredExtensions', [string[]]$normalized, [Microsoft.Win32.RegistryValueKind]::MultiString)
$root.SetValue('ProgId', $progId)
$root.SetValue('ExeName', $exeName)
$root.Close()

$registered = $base.CreateSubKey('Software\RegisteredApplications')
$registered.SetValue($appKeyName, $capabilities)
$registered.Close()

# —— 4. 让「打开方式」对话框认识这个 exe ——
$app = $base.CreateSubKey("$classes\Applications\$exeName")
$app.SetValue('FriendlyAppName', 'nebula-lite')
$app.SetValue('DefaultIcon', $icon)
$appShell = $app.CreateSubKey('shell\open\command')
$appShell.SetValue('', $command)
$appShell.Close()
$supported = $app.CreateSubKey('SupportedTypes')
foreach ($ext in $normalized) {
    $supported.SetValue($ext, $empty, $none)
}
$supported.Close()
$app.Close()

Write-Host '已注册（HKCU，无需管理员）：' -ForegroundColor Green
Write-Host ("  应用：{0}" -f $ExePath) -ForegroundColor Gray
Write-Host ("  类型：{0}" -f ($normalized -join '  ')) -ForegroundColor Gray
Write-Host ''
Write-Host 'Windows 需要你确认默认值（UserChoice 受系统保护，脚本不能静默改）：' -ForegroundColor Yellow
Write-Host '  在「默认应用」里找到 nebula-lite，或按文件类型把上面这些扩展名指给它。' -ForegroundColor Gray
Write-Host ''

if (-not $NoSettings) {
    $uri = 'ms-settings:defaultapps?registeredAppUser=NebulaLite'
    try {
        Start-Process $uri
    }
    catch {
        # Windows 10 不支持带参数的深链：退回到默认应用主页。
        Start-Process 'ms-settings:defaultapps'
    }
    Write-Host '已尝试打开 Windows 默认应用设置页。' -ForegroundColor Gray
}
else {
    Write-Host '已跳过打开系统设置页（-NoSettings）。' -ForegroundColor DarkGray
}
