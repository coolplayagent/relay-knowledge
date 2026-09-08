# Embedded in the paths boundary and executed only by Windows PowerShell 5.1.
# DirectoryInfo.Create(DirectorySecurity) applies the DACL at creation time.
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)

function Get-RelayStorageSid {
    $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
    try { return $identity.User.Value } finally { $identity.Dispose() }
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
    param([System.IO.DirectoryInfo]$Directory, [string]$Sid, [bool]$Private)
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
    if ($Private -and -not $acl.AreAccessRulesProtected) {
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

function Initialize-RelayPrivateStorage {
    param([string]$DataPath, [string]$ExpectedSid, [switch]$ExistingOnly)
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
}
