[CmdletBinding()]
param(
    [string]$InstallDir = $(if ($env:A3S_WORKFLOW_INSTALL_DIR) { $env:A3S_WORKFLOW_INSTALL_DIR } elseif ($env:CARGO_HOME) { Join-Path $env:CARGO_HOME "bin" } else { Join-Path $env:USERPROFILE ".cargo\bin" }),
    [string]$SkillDir = $(if ($env:A3S_WORKFLOW_SKILL_DIR) { $env:A3S_WORKFLOW_SKILL_DIR } elseif ($env:CODEX_HOME) { Join-Path $env:CODEX_HOME "skills\a3s-workflow" } else { Join-Path $env:USERPROFILE ".codex\skills\a3s-workflow" }),
    [switch]$NoCli,
    [switch]$NoSkill,
    [switch]$NoDeploy,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$RepoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))

if (-not (Test-Path -LiteralPath (Join-Path $RepoRoot "Cargo.toml") -PathType Leaf) -or
    -not (Test-Path -LiteralPath (Join-Path $RepoRoot "compose.yaml") -PathType Leaf)) {
    throw "Installer must run from an A3S Workflow checkout"
}

function Invoke-NativeChecked {
    param(
        [Parameter(Mandatory)][string]$FilePath,
        [Parameter(ValueFromRemainingArguments)][string[]]$Arguments
    )
    Write-Host ("+ " + $FilePath + " " + ($Arguments -join " "))
    if ($DryRun) { return }
    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$FilePath failed with exit code $LASTEXITCODE"
    }
}

function Assert-Command {
    param([Parameter(Mandatory)][string]$Name)
    if (-not $DryRun -and -not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Required command not found: $Name"
    }
}

if (-not $NoCli) {
    Assert-Command "cargo"
    Invoke-NativeChecked -FilePath "cargo" -Arguments @(
        "build",
        "--manifest-path", (Join-Path $RepoRoot "Cargo.toml"),
        "--package", "a3s-workflow-cli",
        "--release",
        "--locked"
    )
    Write-Host "+ New-Item -ItemType Directory -Force $InstallDir"
    if (-not $DryRun) {
        New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
        Copy-Item -LiteralPath (Join-Path $RepoRoot "target\release\a3s-workflow.exe") -Destination (Join-Path $InstallDir "a3s-workflow.exe") -Force
    }
}

if (-not $NoSkill) {
    $SkillSource = Join-Path $RepoRoot "skills\a3s-workflow"
    if (-not (Test-Path -LiteralPath (Join-Path $SkillSource "SKILL.md") -PathType Leaf)) {
        throw "Missing skill source: $SkillSource"
    }
    if (Test-Path -LiteralPath $SkillDir) {
        $Backup = "$SkillDir.backup-$([DateTime]::UtcNow.ToString('yyyyMMddHHmmss'))"
        Write-Host "+ Move-Item $SkillDir $Backup"
        if (-not $DryRun) {
            Move-Item -LiteralPath $SkillDir -Destination $Backup
        }
    }
    $SkillParent = Split-Path -Parent $SkillDir
    Write-Host "+ Copy-Item -Recurse $SkillSource $SkillDir"
    if (-not $DryRun) {
        New-Item -ItemType Directory -Force -Path $SkillParent | Out-Null
        Copy-Item -LiteralPath $SkillSource -Destination $SkillDir -Recurse
    }
}

if (-not $NoDeploy) {
    Assert-Command "docker"
    Invoke-NativeChecked -FilePath "docker" -Arguments @(
        "compose",
        "--project-directory", $RepoRoot,
        "-f", (Join-Path $RepoRoot "compose.yaml"),
        "up",
        "--build",
        "--detach"
    )
    if (-not $DryRun) {
        $Deadline = [DateTime]::UtcNow.AddSeconds(120)
        do {
            try {
                Invoke-RestMethod -Uri "http://127.0.0.1:8080/api/health" -TimeoutSec 5 | Out-Null
                $Healthy = $true
            } catch {
                $Healthy = $false
                Start-Sleep -Seconds 2
            }
        } until ($Healthy -or [DateTime]::UtcNow -ge $Deadline)
        if (-not $Healthy) {
            throw "A3S Workflow API did not become healthy within 120 seconds"
        }
    }
}

Write-Host "A3S Workflow installation completed."
if (-not $NoCli) { Write-Host "CLI: $(Join-Path $InstallDir 'a3s-workflow.exe')" }
if (-not $NoSkill) { Write-Host "Skill: $SkillDir" }
if (-not $NoDeploy) {
    Write-Host "Studio: http://127.0.0.1:3000"
    Write-Host "API: http://127.0.0.1:8080"
}
