# Native ACL regression tests; run with Windows PowerShell 5.1, without Pester.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/../../src/relay_knowledge/paths/windows_storage.ps1"

function Assert-Rejected {
    param([scriptblock]$Action, [string]$Expected)
    try { & $Action } catch {
        if ($_.Exception.Message -notlike "*$Expected*") { throw }
        return
    }
    throw "Expected failure containing: $Expected"
}

$sid = Get-RelayStorageSid
$root = Join-Path ([System.IO.Path]::GetTempPath()) ("relay-storage-acl-" + [guid]::NewGuid())
$rootSecurity = [System.Security.AccessControl.DirectorySecurity]::new()
$rootSecurity.SetAccessRuleProtection($true, $false)
$rootSecurity.SetOwner([System.Security.Principal.SecurityIdentifier]::new($sid))
$rootSecurity.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
    [System.Security.Principal.SecurityIdentifier]::new($sid), 'FullControl',
    'ContainerInherit,ObjectInherit', 'None', 'Allow'))
[System.IO.DirectoryInfo]::new($root).Create($rootSecurity)
# A disposable drive alias gives the test a private root without editing any
# real volume's ACL. Production has no drive-alias creation or test bypass.
$drive = @('R:', 'Q:', 'P:', 'O:') | Where-Object { -not [System.IO.Directory]::Exists("$_\") } | Select-Object -First 1
if (-not $drive) { throw 'No free drive letter for isolated storage security test' }
$subst = Join-Path $env:SystemRoot 'System32/subst.exe'
& $subst $drive $root
if ($LASTEXITCODE -ne 0) { throw 'Cannot create isolated drive alias' }
try {
    $base = "$drive\shared"
    $data = "$base\users\$sid\data"
    Initialize-RelayPrivateStorage $data $sid
    Initialize-RelayPrivateStorage $data $sid
    foreach ($path in @((Split-Path $data), $data)) {
        $acl = [System.IO.DirectoryInfo]::new($path).GetAccessControl()
        if (-not $acl.AreAccessRulesProtected) { throw 'Private DACL must be protected' }
        if ($acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value -ne $sid) { throw 'Wrong directory owner' }
        foreach ($rule in $acl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier])) {
            if ($rule.AccessControlType -eq 'Allow' -and @($sid, 'S-1-5-18', 'S-1-5-32-544') -notcontains $rule.IdentityReference.Value) {
                throw 'Private directory permits another account'
            }
        }
    }
    # Newly created SQLite files inherit only the approved principals.
    $database = Join-Path $data 'test.sqlite'
    [System.IO.File]::WriteAllText($database, 'existing graph')
    $fileAcl = [System.IO.File]::GetAccessControl($database)
    foreach ($rule in $fileAcl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier])) {
        if ($rule.AccessControlType -eq 'Allow' -and @($sid, 'S-1-5-18', 'S-1-5-32-544') -notcontains $rule.IdentityReference.Value) {
            throw 'SQLite file inherited unsafe permissions'
        }
    }
    # The same token always identifies the same store, independent of AppData.
    $previousLocal = $env:LOCALAPPDATA
    try {
        $env:LOCALAPPDATA = "$drive\moved-profile"
        if ((Get-RelayStorageSid) -ne $sid) { throw 'Profile relocation changed the account SID' }
        Initialize-RelayPrivateStorage $data $sid
        if ([System.IO.File]::ReadAllText($database) -ne 'existing graph') { throw 'Lost graph during profile relocation' }
    } finally { $env:LOCALAPPDATA = $previousLocal }

    $privateDirectory = [System.IO.DirectoryInfo]::new($data)
    $insecure = $privateDirectory.GetAccessControl()
    $insecure.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
        [System.Security.Principal.SecurityIdentifier]::new('S-1-1-0'), 'Read', 'Allow'))
    $privateDirectory.SetAccessControl($insecure)
    Assert-Rejected { Initialize-RelayPrivateStorage $data $sid } 'Unsafe storage permissions'
    # Validation must not silently repair a pre-existing ACL.
    if ($privateDirectory.GetAccessControl().GetSecurityDescriptorSddlForm('Access') -ne $insecure.GetSecurityDescriptorSddlForm('Access')) {
        throw 'Validation unexpectedly rewrote a directory ACL'
    }

    $parent = [System.IO.DirectoryInfo]::new($base)
    $parentAcl = $parent.GetAccessControl()
    $parentAcl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
        [System.Security.Principal.SecurityIdentifier]::new('S-1-1-0'), 'DeleteSubdirectoriesAndFiles', 'Allow'))
    $parent.SetAccessControl($parentAcl)
    Assert-Rejected { Initialize-RelayPrivateStorage "$base\other\$sid\data" $sid } 'Unsafe storage permissions'
    if ([System.IO.Directory]::Exists("$base\other")) { throw 'Created storage below insecure parent' }

    $junction = "$drive\junction"
    New-Item -ItemType Junction -Path $junction -Target "$drive\shared" | Out-Null
    Assert-Rejected { Initialize-RelayPrivateStorage "$junction\users\$sid\data" $sid } 'reparse points'
    [System.IO.Directory]::Delete($junction)
    Assert-Rejected { Initialize-RelayPrivateStorage "$drive\safe\$sid\data" 'S-1-5-18' } 'account changed'
    Write-Host 'Windows account identity and storage ACL regression tests passed.'
} finally {
    & $subst $drive /D
    Remove-Item -LiteralPath $root -Recurse -Force
}
