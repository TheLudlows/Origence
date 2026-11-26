@echo off
setlocal
for %%I in (node.exe) do set "ORIGENCE_SETUP_NODE=%%~$PATH:I"
if not defined ORIGENCE_SETUP_NODE (
    >&2 echo Origence Deploy requires Node.js 24 or later on PATH.
    exit /b 1
)
"%ORIGENCE_SETUP_NODE%" "%~dp0..\tools\deploy.mjs" %*
exit /b %ERRORLEVEL%
