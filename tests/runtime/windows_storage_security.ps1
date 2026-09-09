# Native ACL regression tests; run with Windows PowerShell 5.1, without Pester.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/../../src/relay_knowledge/paths/windows_storage.ps1"
. "$PSScriptRoot/windows_storage_grants.ps1"

function Assert-Rejected {
    param([scriptblock]$Action, [string]$Expected)
    try { & $Action } catch {
        if ($_.Exception.Message -notlike "*$Expected*") { throw }
        return
    }
    throw "Expected failure containing: $Expected"
}

$sid = Get-RelayStorageSid
if ((Get-RelayStorageCreationOwner $sid $sid) -ne $sid) { throw 'Account must own newly created storage' }
if ((Get-RelayStorageCreationOwner 'S-1-5-18' $sid) -ne 'S-1-5-18') { throw 'LocalSystem must own service-created storage' }
Assert-Rejected { Get-RelayStorageCreationOwner 'S-1-5-21-1-2-3-9999' $sid } 'owning account'
$root = Join-Path ([System.IO.Path]::GetTempPath()) ("relay-storage-acl-" + [guid]::NewGuid())
$rootSecurity = New-RelaySharedStorageSecurity
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
    Assert-Rejected { Initialize-RelayPrivateStorage $data $sid -ExistingOnly } 'existing directories'
    if ([System.IO.Directory]::Exists($base)) { throw 'Read-only validation provisioned storage' }
    Initialize-RelayPrivateStorage $data $sid
    Initialize-RelayPrivateStorage $data $sid
    Initialize-RelayPrivateStorage $data $sid -ExistingOnly
    Assert-RelayServiceDatabasePath "$data\not-yet-created.sqlite"
    if ((Get-RelayStoragePathKind $data) -ne 'directory') { throw 'Directory probe lost its type' }
    if ((Get-RelayStoragePathKind "$data\absent") -ne 'missing') { throw 'Missing probe must stay missing' }
    Assert-Rejected { Initialize-RelayPrivateStorage $data $sid -DatabasePath "$data\absent.sqlite" -ExistingOnly } 'SQLite database is missing'
    if ([System.IO.File]::Exists("$data\absent.sqlite")) { throw 'Read-only database validation created a file' }
    # Exercise the service identity branch against real persisted ACLs. Only
    # token lookup is stubbed in this test scope; ACL/owner/reparse reads are real.
    $originalSidFunction = ${function:Get-RelayStorageSid}
    try {
        function Get-RelayStorageSid { return 'S-1-5-18' }
        Initialize-RelayPrivateStorage $data $sid -ExistingOnly
    } finally { Set-Item Function:Get-RelayStorageSid $originalSidFunction }
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
    # Both shared ancestors have an owner independent of the first account.
    foreach ($shared in @($base, "$base\users")) {
        $acl = [System.IO.DirectoryInfo]::new($shared).GetAccessControl()
        if ($acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value -ne 'S-1-5-32-544') { throw 'Shared storage must have a stable Administrators owner' }
        Assert-RelayDirectorySecurity ([System.IO.DirectoryInfo]::new($shared)) 'S-1-5-21-1-2-3-1002' $false
    }
    $secondSid = 'S-1-5-21-1-2-3-1002'
    $secondSecurity = [System.Security.AccessControl.DirectorySecurity]::new()
    $secondSecurity.SetAccessRuleProtection($true, $false)
    $secondSecurity.SetOwner([System.Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))
    foreach ($principal in @($secondSid, 'S-1-5-18', 'S-1-5-32-544')) {
        $secondSecurity.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
            [System.Security.Principal.SecurityIdentifier]::new($principal), 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
    }
    $secondProfile = "$base\users\$secondSid"
    [System.IO.DirectoryInfo]::new($secondProfile).Create($secondSecurity)
    [System.IO.DirectoryInfo]::new("$secondProfile\data").Create($secondSecurity)
    try {
        function Get-RelayStorageSid { return $secondSid }
        Initialize-RelayPrivateStorage "$secondProfile\data" $secondSid -ExistingOnly
    } finally { Set-Item Function:Get-RelayStorageSid $originalSidFunction }
    $sharedInfo = [System.IO.DirectoryInfo]::new($base)
    $changedOwner = $sharedInfo.GetAccessControl()
    $changedOwner.SetOwner([System.Security.Principal.SecurityIdentifier]::new($sid))
    $sharedInfo.SetAccessControl($changedOwner)
    try { Assert-Rejected { Initialize-RelayPrivateStorage $data $sid -ExistingOnly } 'Shared storage ancestor requires' }
    finally {
        $restoredOwner = $sharedInfo.GetAccessControl()
        $restoredOwner.SetOwner([System.Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))
        $sharedInfo.SetAccessControl($restoredOwner)
    }
    # Newly created SQLite files inherit only the approved principals.
    $database = Join-Path $data 'test.sqlite'
    [System.IO.File]::WriteAllText($database, 'existing graph')
    Test-RelayRequiredStorageGrants $data $database $sid
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

    # Files moved on the same NTFS volume retain explicit ACLs. A private parent
    # alone must not authorize the database, recovery files, or repository shards.
    foreach ($payload in @($database, "$database-wal", "$database-shm", "$database-journal", "$data\stores\repositories\fixture\code.sqlite")) {
        Initialize-RelayPrivateStorage $data $sid -DatabasePath $payload
        $incoming = "$drive\incoming-payload"
        [System.IO.File]::WriteAllText($incoming, 'moved payload')
        $acl = [System.IO.File]::GetAccessControl($incoming)
        $safeAccess = [System.IO.File]::GetAccessControl($database).GetSecurityDescriptorSddlForm('Access')
        $acl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
            [System.Security.Principal.SecurityIdentifier]::new('S-1-1-0'), 'Read', 'Allow'))
        [System.IO.File]::SetAccessControl($incoming, $acl)
        if ([System.IO.File]::Exists($payload)) { [System.IO.File]::Delete($payload) }
        [System.IO.File]::Move($incoming, $payload)
        if ((Get-RelayStoragePathKind $payload) -ne 'file') { throw 'File probe lost its type' }
        $before = [System.IO.File]::GetAccessControl($payload).GetSecurityDescriptorSddlForm('Access')
        Assert-Rejected { Initialize-RelayPrivateStorage $data $sid } 'payload permissions'
        Assert-Rejected { Initialize-RelayPrivateStorage $data $sid -DatabasePath $payload -ExistingOnly } 'payload permissions'
        try {
            function Get-RelayStorageSid { return 'S-1-5-18' }
            Assert-Rejected { Initialize-RelayPrivateStorage $data $sid -DatabasePath $payload -ExistingOnly } 'payload permissions'
        } finally { Set-Item Function:Get-RelayStorageSid $originalSidFunction }
        if ([System.IO.File]::GetAccessControl($payload).GetSecurityDescriptorSddlForm('Access') -ne $before) { throw 'Payload validation rewrote ACLs' }
        # SetAccessControl persists only modified sections. A descriptor loaded
        # with GetAccessControl alone would leave the injected Everyone ACE intact.
        $restored = [System.Security.AccessControl.FileSecurity]::new()
        $restored.SetSecurityDescriptorSddlForm($safeAccess, 'Access')
        [System.IO.File]::SetAccessControl($payload, $restored)
        Initialize-RelayPrivateStorage $data $sid -DatabasePath $payload -ExistingOnly
    }
    $fileLink = "$data\linked.sqlite"
    New-Item -ItemType SymbolicLink -Path $fileLink -Target $database | Out-Null
    Assert-Rejected { Initialize-RelayPrivateStorage $data $sid } 'reparse points'
    Assert-Rejected { Assert-RelayServiceDatabasePath $fileLink } 'regular file'
    Assert-Rejected { Initialize-RelayPrivateStorage $data $sid -DatabasePath $fileLink } 'regular file'
    [System.IO.File]::Delete($fileLink)
    $removed = "$data\removed.sqlite"
    [System.IO.File]::WriteAllText($removed, 'valid before removal')
    Initialize-RelayPrivateStorage $data $sid -DatabasePath $removed -ExistingOnly
    [System.IO.File]::Delete($removed)
    Assert-Rejected { Initialize-RelayPrivateStorage $data $sid -DatabasePath $removed -ExistingOnly } 'SQLite database is missing'
    $shardLink = "$data\stores\repositories\linked"
    New-Item -ItemType Junction -Path $shardLink -Target "$data\stores\repositories\fixture" | Out-Null
    Assert-Rejected { Initialize-RelayPrivateStorage $data $sid } 'reparse points'
    Assert-Rejected { Initialize-RelayPrivateStorage $data $sid -DatabasePath "$shardLink\code.sqlite" } 'reparse points'
    Assert-Rejected { Assert-RelayServiceDatabasePath "$shardLink\code.sqlite" } 'reparse points'
    [System.IO.Directory]::Delete($shardLink)
    $deep = "$data\deep"
    $leaf = $deep + ('\d' * 32)
    [System.IO.Directory]::CreateDirectory($leaf) | Out-Null
    Assert-Rejected { Initialize-RelayPrivateStorage $data $sid } 'depth limit'
    Remove-Item -LiteralPath $deep -Recurse -Force

    $privateDirectory = [System.IO.DirectoryInfo]::new($data)
    $insecure = $privateDirectory.GetAccessControl()
    $insecure.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
        [System.Security.Principal.SecurityIdentifier]::new('S-1-1-0'), 'Read', 'Allow'))
    $privateDirectory.SetAccessControl($insecure)
    # Windows canonicalizes descriptors when persisting them. Compare two OS
    # reads, not the pre-persistence in-memory descriptor against an OS read.
    $beforeValidation = $privateDirectory.GetAccessControl().GetSecurityDescriptorSddlForm('Access')
    Assert-Rejected { Initialize-RelayPrivateStorage $data $sid } 'Unsafe storage permissions'
    try {
        function Get-RelayStorageSid { return 'S-1-5-18' }
        Assert-Rejected { Initialize-RelayPrivateStorage $data $sid -ExistingOnly } 'Unsafe storage permissions'
    } finally { Set-Item Function:Get-RelayStorageSid $originalSidFunction }
    # Validation must not silently repair a pre-existing ACL.
    if ($privateDirectory.GetAccessControl().GetSecurityDescriptorSddlForm('Access') -ne $beforeValidation) {
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
    try {
        function Get-RelayStorageSid { return 'S-1-5-18' }
        Assert-Rejected { Initialize-RelayPrivateStorage "$junction\users\$sid\data" $sid -ExistingOnly } 'reparse points'
    } finally { Set-Item Function:Get-RelayStorageSid $originalSidFunction }
    [System.IO.Directory]::Delete($junction)
    Assert-Rejected { Initialize-RelayPrivateStorage "$drive\safe\$sid\data" 'S-1-5-18' } 'owning account'
    Write-Host 'Windows account identity and storage ACL regression tests passed.'
} catch {
    Write-Host $_.ScriptStackTrace
    throw
} finally {
    & $subst $drive /D
    Remove-Item -LiteralPath $root -Recurse -Force
}
