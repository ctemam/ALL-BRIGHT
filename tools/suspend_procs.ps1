param(
    [Parameter(Mandatory=$true)][int[]]$Pids,
    [switch]$Resume
)
# Suspend/resume arbitrary PIDs via ntdll — reversible, never kills.
$src = @"
using System;
using System.Runtime.InteropServices;
public static class NativeProc {
    [DllImport("ntdll.dll")] public static extern int NtSuspendProcess(IntPtr h);
    [DllImport("ntdll.dll")] public static extern int NtResumeProcess(IntPtr h);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern IntPtr OpenProcess(int access, bool inherit, int pid);
    [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
}
"@
Add-Type -TypeDefinition $src -Language CSharp
$PROCESS_SUSPEND_RESUME = 0x0800
foreach ($pid_ in $Pids) {
    $h = [NativeProc]::OpenProcess($PROCESS_SUSPEND_RESUME, $false, $pid_)
    if ($h -eq [IntPtr]::Zero) { Write-Output "pid ${pid_}: open failed"; continue }
    $rc = if ($Resume) { [NativeProc]::NtResumeProcess($h) } else { [NativeProc]::NtSuspendProcess($h) }
    [NativeProc]::CloseHandle($h) | Out-Null
    $verb = if ($Resume) { "resumed" } else { "suspended" }
    Write-Output "pid ${pid_}: $verb (rc=$rc)"
}
