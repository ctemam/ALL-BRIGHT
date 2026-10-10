@echo off
title AllBright - Autonomous Wealth Engine
cd /d D:\Allbright
echo.
echo   AllBright - starting fleet via PM2...
echo.
pm2 start ecosystem.config.js
if errorlevel 1 ( pm2 resurrect )
pm2 status
echo.
echo   Dashboard: http://localhost:3001
echo   (PM2 keeps services alive in background - safe to close this window)
echo.
timeout /t 4 /nobreak >nul
start "" http://localhost:3001
