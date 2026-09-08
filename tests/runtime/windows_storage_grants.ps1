# Native mutations of disposable private ACLs; validation must never repair them.
function Test-RelayRequiredStorageGrants {
    param([string]$Data, [string]$Database, [string]$Sid)
    foreach ($item in @([System.IO.DirectoryInfo]::new($Data), [System.IO.FileInfo]::new($Database))) {
        $saved = $item.GetAccessControl().GetSecurityDescriptorSddlForm('Access')
        foreach ($principal in @($Sid, 'S-1-5-18', 'S-1-5-32-544', 'S-1-1-0')) {
            foreach ($deny in @($false, $true)) {
                if (-not $deny -and $principal -eq 'S-1-1-0') { continue }
                try {
                    $acl = $item.GetAccessControl()
                    $acl.SetAccessRuleProtection($true, $true)
                    $identity = [System.Security.Principal.SecurityIdentifier]::new($principal)
                    if ($deny) {
                        $acl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($identity, 'WriteData', 'Deny'))
                    } else { $acl.PurgeAccessRules($identity) }
                    $item.SetAccessControl($acl)
                    $persisted = $item.GetAccessControl().GetSecurityDescriptorSddlForm('Access')
                    $expected = if ($deny) { 'deny permissions' } else { 'must grant full control' }
                    Assert-Rejected { Initialize-RelayPrivateStorage $Data $Sid -ExistingOnly } $expected
                    if ($item.GetAccessControl().GetSecurityDescriptorSddlForm('Access') -ne $persisted) { throw 'Grant validation rewrote the ACL' }
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
