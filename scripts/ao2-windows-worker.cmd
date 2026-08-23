@echo off
setlocal
where py >nul 2>nul
if not errorlevel 1 (
  py -3.11 -c "import sys; raise SystemExit(0 if sys.version_info >= (3, 11) else 1)" >nul 2>nul
  if not errorlevel 1 (
    py -3.11 "%~dp0ao2-windows-outbound-worker.py" %*
    exit /b %errorlevel%
  )
)
where python >nul 2>nul
if not errorlevel 1 (
  python -c "import sys; raise SystemExit(0 if sys.version_info >= (3, 11) else 1)" >nul 2>nul
  if not errorlevel 1 (
    python "%~dp0ao2-windows-outbound-worker.py" %*
    exit /b %errorlevel%
  )
)
>&2 echo AO2 Windows outbound worker requires Python 3.11 or newer.
exit /b 1
