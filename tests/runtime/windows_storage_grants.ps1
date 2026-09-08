# Native mutations of disposable private ACLs; validation must never repair them.
function Test-RelayRequiredStorageGrants {
    param([string]$Data, [string]$Database, [string]$Sid)
    foreach ($item in @([System.IO.DirectoryInfo]::new($Data), [System.IO.FileInfo]::new($Database))) {
        $saved = $item.GetAccessControl().GetSecurityDescriptorSddlForm('Access')
        foreach ($principal in @($Sid, 'S-1-5-18', 'S-1-5-32-544', 'S-1-1-0')) {
            foreach ($deny in @($false, $true)) {
                if (-not $deny -and $principal -eq 'S-1-1-0') { continue }
                try {
                    # PurgeAccessRules ignores inherited ACEs. Build explicit
                    # test rules so file inheritance cannot retain a removed grant.
                    $original = $item.GetAccessControl()
                    $acl = if ($item -is [System.IO.DirectoryInfo]) {
                        [System.Security.AccessControl.DirectorySecurity]::new()
                    } else { [System.Security.AccessControl.FileSecurity]::new() }
                    $acl.SetAccessRuleProtection($true, $false)
                    foreach ($rule in $original.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier])) {
                        if (-not $deny -and $rule.IdentityReference.Value -eq $principal) { continue }
                        $acl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
                            $rule.IdentityReference, $rule.FileSystemRights, $rule.InheritanceFlags,
                            $rule.PropagationFlags, $rule.AccessControlType))
                    }
                    $identity = [System.Security.Principal.SecurityIdentifier]::new($principal)
                    if ($deny) {
                        $acl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($identity, 'WriteData', 'Deny'))
                    }
                    $item.SetAccessControl($acl)
                    $readBack = $item.GetAccessControl()
                    $persisted = $readBack.GetSecurityDescriptorSddlForm('Access')
                    $targetRules = @($readBack.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier]) | Where-Object { $_.IdentityReference.Value -eq $principal })
                    if (-not $deny -and $targetRules.Count) { throw "Fixture retained revoked grant for $principal on $($item.FullName)" }
                    if ($deny -and -not ($targetRules | Where-Object { $_.AccessControlType -eq 'Deny' })) { throw "Fixture did not persist denial for $principal on $($item.FullName)" }
                    $expected = if ($deny) { 'deny permissions' } else { 'must grant full control' }
                    Assert-Rejected { Initialize-RelayPrivateStorage $Data $Sid -ExistingOnly } $expected
                    if ($item.GetAccessControl().GetSecurityDescriptorSddlForm('Access') -ne $persisted) { throw 'Grant validation rewrote the ACL' }
                } catch {
                    throw "Grant case $principal deny=$deny on $($item.FullName): $($_.Exception.Message)"
                } finally {
                    $restored = if ($item -is [System.IO.DirectoryInfo]) {
                        [System.Security.AccessControl.DirectorySecurity]::new()
                    } else { [System.Security.AccessControl.FileSecurity]::new() }
                    $restored.SetSecurityDescriptorSddlForm($saved, 'Access')
                    $item.SetAccessControl($restored)
                }
                Initialize-RelayPrivateStorage $Data $Sid -DatabasePath $Database -ExistingOnly
            }
        }
    }
}
