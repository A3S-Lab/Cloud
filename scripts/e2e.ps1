param(
    [string]$A3sTestExecutable = $(if ($env:A3S_TEST_BIN) { $env:A3S_TEST_BIN } else { "a3s-test" }),
    [string]$BrowserExecutable = $(if ($env:A3S_TEST_BROWSER) { $env:A3S_TEST_BROWSER } else { "agent-browser" }),
    [string]$Manifest = "tests/e2e/workflow-studio.acl",
    [int]$CommandTimeoutMs = 10000,
    [int]$IdleTimeoutMs = 1000,
    [int]$CleanupTimeoutMs = 20000
)

$ErrorActionPreference = "Stop"

function Invoke-Checked {
    param([string[]]$Arguments)
    & $A3sTestExecutable @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "a3s-test failed with exit code $LASTEXITCODE"
    }
}

Invoke-Checked @(
    "capabilities",
    "--browser-driver", "standalone",
    "--browser-executable", $BrowserExecutable,
    "--json"
)

Invoke-Checked @("check", $Manifest, "--json")

Invoke-Checked @(
    "run", $Manifest,
    "--browser-driver", "standalone",
    "--browser-executable", $BrowserExecutable,
    "--command-timeout-ms", $CommandTimeoutMs.ToString(),
    "--idle-timeout-ms", $IdleTimeoutMs.ToString(),
    "--cleanup-timeout-ms", $CleanupTimeoutMs.ToString(),
    "--infrastructure-retries", "1",
    "--json"
)
