param(
    [ValidateSet("smoke-10", "verified-first-100", "verified-full")]
    [string]$Suite = "smoke-10",
    [ValidateRange(1, 8)]
    [int]$Concurrency = 1,
    [switch]$Resume,
    [switch]$ParallelConditions,
    [switch]$RequireSkillUse
)

$ErrorActionPreference = "Stop"
$evalKey = Read-Host -MaskInput "DeepSeek API key"
if ([string]::IsNullOrWhiteSpace($evalKey)) {
    throw "A DeepSeek API key is required."
}

$previousKey = $env:DEEPSEEK_API_KEY
try {
    $env:DEEPSEEK_API_KEY = $evalKey
    $arguments = @(
        "run",
        "relay-knowledge-skill-eval",
        "run",
        "--suite", $Suite,
        "--concurrency", $Concurrency
    )
    if ($Resume) {
        $arguments += "--resume"
    }
    if ($ParallelConditions) {
        $arguments += "--parallel-conditions"
    }
    if ($RequireSkillUse) {
        $arguments += "--require-skill-use"
    }
    & uv @arguments
    if ($LASTEXITCODE -ne 0) {
        throw "relay-knowledge skill evaluation exited with $LASTEXITCODE"
    }
}
finally {
    $evalKey = $null
    if ($null -eq $previousKey) {
        Remove-Item Env:DEEPSEEK_API_KEY -ErrorAction SilentlyContinue
    }
    else {
        $env:DEEPSEEK_API_KEY = $previousKey
    }
}
