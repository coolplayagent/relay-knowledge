# Embedded in the paths boundary and executed only by Windows PowerShell 5.1.
# DirectoryInfo.Create(DirectorySecurity) applies the DACL at creation time.
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)

function Get-RelayStorageSid {
    $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
    try { return $identity.User.Value } finally { $identity.Dispose() }
}

function Get-RelayStoragePathKind {
    param([string]$Path)
    # File.GetAttributes reports access/I/O failures instead of hiding them as
    # missing. A hung filesystem is confined to the terminable helper process.
    try { $attributes = [System.IO.File]::GetAttributes($Path) }
    catch [System.IO.FileNotFoundException] { return 'missing' }
    catch [System.IO.DirectoryNotFoundException] { return 'missing' }
    if ($attributes -band [System.IO.FileAttributes]::ReparsePoint) { return 'reparse' }
    if ($attributes -band [System.IO.FileAttributes]::Directory) { return 'directory' }
    return 'file'
}

function Get-RelayStorageCreationOwner {
    param([string]$ActorSid, [string]$ExpectedSid)
    # Installed services run as LocalSystem but retain the installing account's
    # directory. Other accounts cannot adopt that account's reserved path.
    if ($ActorSid -ne $ExpectedSid -and $ActorSid -ne 'S-1-5-18') {
        throw 'Storage requires the owning account or LocalSystem'
    }
    return $ActorSid
}

function Assert-RelayDirectorySecurity {
    param([System.IO.DirectoryInfo]$Directory, [string]$Sid, [bool]$Private, [switch]$AllowInheritance)
    $Directory.Refresh()
    if (-not $Directory.Exists -or ($Directory.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
        throw "Storage must use existing directories without reparse points: $($Directory.FullName)"
    }
    $acl = $Directory.GetAccessControl()
    $raw = [System.Security.AccessControl.RawSecurityDescriptor]::new($acl.GetSecurityDescriptorBinaryForm(), 0)
    if ($null -eq $raw.DiscretionaryAcl) { throw "Unsafe null storage DACL: $($Directory.FullName)" }
    $trusted = @($Sid, 'S-1-5-18', 'S-1-5-32-544') # Account, SYSTEM, Administrators.
    if ($trusted -notcontains $acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value) {
        throw "Untrusted storage directory owner: $($Directory.FullName)"
    }
    if ($Private -and -not $AllowInheritance -and -not $acl.AreAccessRulesProtected) {
        throw "Private storage directory inherits permissions: $($Directory.FullName)"
    }
    # Even a protected child can be removed through its parent's DELETE_CHILD.
    # Attribute/ACL/owner writes can also redirect or replace a storage ancestor.
    $dangerous = [int][System.Security.AccessControl.FileSystemRights]'DeleteSubdirectoriesAndFiles,Delete,ChangePermissions,TakeOwnership,WriteAttributes,WriteExtendedAttributes'
    $dangerous = $dangerous -bor 0x10000000 -bor 0x40000000 # GENERIC_ALL / GENERIC_WRITE
    $ownerCanInherit = $false
    foreach ($rule in $acl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier])) {
        if ($rule.AccessControlType -ne 'Allow') { continue }
        if (-not $Private -and ($rule.PropagationFlags -band [System.Security.AccessControl.PropagationFlags]::InheritOnly)) { continue }
        if ($trusted -notcontains $rule.IdentityReference.Value -and ($Private -or ($rule.FileSystemRights -band $dangerous))) {
            throw "Unsafe storage permissions for $($rule.IdentityReference.Value): $($Directory.FullName)"
        }
        if ($rule.IdentityReference.Value -eq $Sid -and
            ($rule.FileSystemRights -band [System.Security.AccessControl.FileSystemRights]::FullControl) -eq [System.Security.AccessControl.FileSystemRights]::FullControl -and
            ($rule.InheritanceFlags -band [System.Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit') -eq [System.Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit' -and
            $rule.PropagationFlags -eq [System.Security.AccessControl.PropagationFlags]::None) {
            $ownerCanInherit = $true
        }
    }
    if ($Private -and -not $ownerCanInherit) {
        throw "Private storage must grant the account inheritable full control: $($Directory.FullName)"
    }
}

function Assert-RelayStoragePayload {
    param([System.IO.FileSystemInfo]$Item, [string]$Sid)
    $Item.Refresh()
    if (-not $Item.Exists -or ($Item.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
        throw "Storage payload must exist without reparse points: $($Item.FullName)"
    }
    if ($Item -is [System.IO.DirectoryInfo]) {
        Assert-RelayDirectorySecurity $Item $Sid $true -AllowInheritance
        return
    }
    $acl = $Item.GetAccessControl()
    $raw = [System.Security.AccessControl.RawSecurityDescriptor]::new($acl.GetSecurityDescriptorBinaryForm(), 0)
    if ($null -eq $raw.DiscretionaryAcl) { throw "Unsafe null payload DACL: $($Item.FullName)" }
    $trusted = @($Sid, 'S-1-5-18', 'S-1-5-32-544')
    if ($trusted -notcontains $acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value) {
        throw "Untrusted storage payload owner: $($Item.FullName)"
    }
    foreach ($rule in $acl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier])) {
        if ($rule.AccessControlType -eq 'Allow' -and $trusted -notcontains $rule.IdentityReference.Value) {
            throw "Unsafe storage payload permissions: $($Item.FullName)"
        }
    }
}

function Assert-RelayStoragePayloadTree {
    param([System.IO.DirectoryInfo]$Data, [string]$Sid)
    # Enumerate lazily without following links. The process deadline bounds slow
    # ACL reads; explicit limits also bound memory and total work.
    $pending = [System.Collections.Stack]::new()
    $pending.Push(@{ Directory = $Data; Depth = 0 })
    $visited = 0
    while ($pending.Count -gt 0) {
        $entry = $pending.Pop()
        if ($entry.Depth -ge 32) { throw 'Storage payload depth limit exceeded' }
        $iterator = $entry.Directory.EnumerateFileSystemInfos().GetEnumerator()
        try {
            while ($iterator.MoveNext()) {
                $visited++
                if ($visited -gt 65536) { throw 'Storage payload entry limit exceeded' }
                $item = $iterator.Current
                Assert-RelayStoragePayload $item $Sid
                if ($item -is [System.IO.DirectoryInfo]) {
                    $pending.Push(@{ Directory = $item; Depth = $entry.Depth + 1 })
                }
            }
        } finally { $iterator.Dispose() }
    }
}

function Initialize-RelayStorageDatabase {
    param([System.IO.DirectoryInfo]$Data, [string]$DatabasePath, [string]$Sid,
        [System.Security.AccessControl.DirectorySecurity]$Security, [switch]$ExistingOnly)
    $database = [System.IO.FileInfo]::new($DatabasePath)
    $ancestors = [System.Collections.Generic.List[System.IO.DirectoryInfo]]::new()
    for ($cursor = $database.Directory; $cursor.FullName -ne $Data.FullName; $cursor = $cursor.Parent) {
        if ($null -eq $cursor -or $ancestors.Count -ge 32) { throw 'Database must remain below its private data directory' }
        $ancestors.Add($cursor)
    }
    for ($index = $ancestors.Count - 1; $index -ge 0; $index--) {
        if (-not $ExistingOnly) { $ancestors[$index].Create($Security) }
        Assert-RelayStoragePayload $ancestors[$index] $Sid
    }
    foreach ($suffix in @('', '-wal', '-shm', '-journal')) {
        $path = $database.FullName + $suffix
        $kind = Get-RelayStoragePathKind $path
        if ($kind -eq 'missing') {
            if ($ExistingOnly -and $suffix -eq '') { throw "SQLite database is missing: $path" }
            continue
        }
        if ($kind -ne 'file') { throw "SQLite payload must be a regular file: $path" }
        Assert-RelayStoragePayload ([System.IO.FileInfo]::new($path)) $Sid
    }
}

function Initialize-RelayPrivateStorage {
    param([string]$DataPath, [string]$ExpectedSid, [switch]$ExistingOnly, [string]$DatabasePath)
    $creationOwner = Get-RelayStorageCreationOwner (Get-RelayStorageSid) $ExpectedSid
    $data = [System.IO.DirectoryInfo]::new($DataPath)
    $profile = $data.Parent
    if ($data.Name -ne 'data' -or $profile.Name -ne $ExpectedSid) { throw 'Invalid account storage layout' }
    # Check/create one ancestor at a time, from the volume root down. Never
    # rewrite existing ACLs or adopt an untrusted directory and its contents.
    $ancestors = [System.Collections.Generic.List[System.IO.DirectoryInfo]]::new()
    for ($cursor = $profile.Parent; $null -ne $cursor; $cursor = $cursor.Parent) {
        if ($ancestors.Count -ge 32) { throw 'Storage ancestor limit exceeded' }
        $ancestors.Add($cursor)
    }
    for ($index = $ancestors.Count - 1; $index -ge 0; $index--) {
        $directory = $ancestors[$index]
        if (-not $ExistingOnly -and -not $directory.Exists -and $null -ne $directory.Parent) { $directory.Create() }
        Assert-RelayDirectorySecurity $directory $ExpectedSid $false
    }
    $security = [System.Security.AccessControl.DirectorySecurity]::new()
    $security.SetAccessRuleProtection($true, $false)
    $security.SetOwner([System.Security.Principal.SecurityIdentifier]::new($creationOwner))
    foreach ($principal in (@($ExpectedSid, 'S-1-5-18', 'S-1-5-32-544') | Select-Object -Unique)) {
        $rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
            [System.Security.Principal.SecurityIdentifier]::new($principal),
            [System.Security.AccessControl.FileSystemRights]::FullControl,
            [System.Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit',
            [System.Security.AccessControl.PropagationFlags]::None,
            [System.Security.AccessControl.AccessControlType]::Allow)
        $security.AddAccessRule($rule)
    }
    foreach ($directory in @($profile, $data)) {
        # Create is a no-op for existing directories; validation then rejects
        # pre-created permissive directories rather than silently repairing them.
        if (-not $ExistingOnly) { $directory.Create($security) }
        Assert-RelayDirectorySecurity $directory $ExpectedSid $true
    }
    if ($DatabasePath) {
        Initialize-RelayStorageDatabase $data $DatabasePath $ExpectedSid $security -ExistingOnly:$ExistingOnly
    } else {
        Assert-RelayStoragePayloadTree $data $ExpectedSid
    }
}
