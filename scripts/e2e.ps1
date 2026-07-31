param(
    [string]$A3sTestExecutable = $(if ($env:A3S_TEST_BIN) { $env:A3S_TEST_BIN } else { "a3s-test" }),
    [string]$BrowserExecutable = $(if ($env:A3S_TEST_BROWSER) { $env:A3S_TEST_BROWSER } else { "agent-browser" }),
    [string]$Manifest = "tests/e2e/workflow-studio.acl"
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
    "--command-timeout-ms", "10000",
    "--idle-timeout-ms", "1000",
    "--cleanup-timeout-ms", "10000",
    "--infrastructure-retries", "1",
    "--json"
)
