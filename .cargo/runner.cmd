@echo off
"C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe" sign /a /fd SHA256 %1 >nul 2>&1
%*
