<#
.SYNOPSIS
  Protocol handler for the "Update now" toast button.

.DESCRIPTION
  Launched by the shell when the user clicks "Update now" on the update toast
  (via the custom cveelumeupdate:// URL scheme registered by
  test-update-notify.ps1). Any URI argument the shell appends is ignored; we
  just run the proven in-place update.
#>
param()  # no CmdletBinding, so the trailing "%1" URI lands in $args and is ignored

& "$PSScriptRoot\test-update-replace.ps1" -Apply
