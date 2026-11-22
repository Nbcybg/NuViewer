@echo off
rem %1 = the file to read, %2 = the PNG to write. [0] = the first frame only. The exit code is magick's: 0 = success.
magick "%~1[0]" "PNG32:%~2"
