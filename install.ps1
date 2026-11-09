# Installs Tolon on Windows:  irm https://github.com/danielpsf/tolon/releases/latest/download/install.ps1 | iex
# What it does, and how to read it first: docs/install.md, "Install from PowerShell".
# The whole script is one function, called on the last line, so a download that is cut off runs nothing.
# The copy attached to a release is stamped with that release's version (release_assets.py stamp-install).
# The copy in the repository is not: it asks GitHub for the latest version instead.
# Works in Windows PowerShell 5.1 and PowerShell 7. Never asks for administrator rights: the installer is per user.

function Install-Tolon {
  $ErrorActionPreference = 'Stop'
  $ProgressPreference = 'SilentlyContinue'
  $repo = 'danielpsf/tolon'
  $version = '@TOLON_VERSION@'

  function Say($text) { Write-Host $text }
  function Fail($text) { throw "Tolon was not installed. $text" }
  function Fetch($url, $out) {
    if ($out) { Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $out }
    else { (Invoke-WebRequest -UseBasicParsing -Uri $url).Content }
  }

  if (-not $IsWindows -and $PSVersionTable.PSVersion.Major -ge 6) {
    Fail "This installer is for Windows. Download the installer for your system from https://github.com/$repo/releases."
  }
  if (-not [Environment]::Is64BitOperatingSystem) {
    Fail "Tolon has Windows builds for 64-bit systems only. See https://github.com/$repo/releases."
  }
  # Windows PowerShell 5.1 does not use TLS 1.2 by default on older systems.
  [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

  if ($version -like '@*' -or -not $version) {
    Say 'Looking up the latest version of Tolon.'
    $version = (Fetch "https://github.com/$repo/releases/latest/download/latest.json" | ConvertFrom-Json).version
  }
  if ($version -notmatch '^\d+\.\d+\.\d+([-+][0-9A-Za-z.-]+)?$') {
    Fail "Could not find out which version to install. Download it from https://github.com/$repo/releases."
  }

  $file = "Tolon_${version}_x64-setup.exe"
  $base = "https://github.com/$repo/releases/download/v$version"
  $tmp = Join-Path ([IO.Path]::GetTempPath()) ('tolon-install-' + [IO.Path]::GetRandomFileName())
  New-Item -ItemType Directory -Path $tmp | Out-Null
  try {
    Say "Downloading Tolon $version."
    try { Fetch "$base/$file" (Join-Path $tmp $file) } catch { Fail "The download of $file failed. Check your connection and try again." }
    try { Fetch "$base/SHA256SUMS" (Join-Path $tmp 'SHA256SUMS') } catch { Fail 'The download of SHA256SUMS failed. Check your connection and try again.' }

    $pattern = '^([0-9A-Fa-f]{64})\s+\*?' + [regex]::Escape($file) + '$'
    $expected = @(Get-Content (Join-Path $tmp 'SHA256SUMS') | ForEach-Object { if ($_ -match $pattern) { $Matches[1] } })
    if ($expected.Count -ne 1) { Fail "SHA256SUMS has no line for $file, so the download cannot be checked." }
    $actual = (Get-FileHash -Algorithm SHA256 -Path (Join-Path $tmp $file)).Hash
    if ($actual -ne $expected[0]) {
      Fail "The checksum of $file does not match SHA256SUMS. The download was deleted. Try again, and report it if it keeps happening."
    }
    Say 'The checksum matches.'
    # The checksum stands in for the mark of the web, as on macOS: SmartScreen then does not ask about the download.
    Unblock-File -Path (Join-Path $tmp $file) -ErrorAction SilentlyContinue

    $running = @(Get-Process -Name 'Tolon', 'tolon-app' -ErrorAction SilentlyContinue)
    if ($running.Count -gt 0) {
      Say 'Quitting Tolon so it can be replaced.'
      $running | Stop-Process -Force -ErrorAction SilentlyContinue
      $running | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
    }

    Say "Installing Tolon $version."
    # /S: the installer runs without windows or questions. It installs for the current user only, so no administrator prompt.
    $setup = Start-Process -FilePath (Join-Path $tmp $file) -ArgumentList '/S' -Wait -PassThru
    if ($setup.ExitCode -ne 0) { Fail "The installer stopped with code $($setup.ExitCode). Run $file by hand from your Downloads folder to see why." }

    $app = Get-ChildItem -Path (Join-Path $env:LOCALAPPDATA 'Tolon') -Filter '*.exe' -ErrorAction SilentlyContinue |
      Where-Object { $_.Name -ne 'uninstall.exe' } | Select-Object -First 1
    if (-not $app) { Fail "The installer finished, but Tolon was not found in $env:LOCALAPPDATA\Tolon. Start it from the Start menu." }
    Say "Installed Tolon $version in $($app.DirectoryName)."
    Start-Process -FilePath $app.FullName
  }
  finally {
    Remove-Item -Path $tmp -Recurse -Force -ErrorAction SilentlyContinue
  }
}

Install-Tolon
