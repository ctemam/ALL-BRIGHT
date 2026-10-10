$ws = New-Object -ComObject WScript.Shell
$sc = $ws.CreateShortcut('C:\Users\op\Desktop\AllBright.lnk')
$sc.TargetPath = 'D:\Allbright\start_allbright.bat'
$sc.WorkingDirectory = 'D:\Allbright'
$sc.IconLocation = 'D:\Allbright\crates\allbright-dashboard\static\logo.ico'
$sc.Description = 'AllBright — Autonomous Wealth Engine (starts PM2 fleet + opens dashboard)'
$sc.Save()
Write-Output 'shortcut created'
