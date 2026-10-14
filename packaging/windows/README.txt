TAW Equaliser - system-wide engine for Windows (TEST BUILD)
===========================================================

This folder installs the Equaliser's engine inside Windows' own audio
engine, so it equalises everything an output plays. No virtual cable is
needed. It's an early test build: try it on one device first.

COMING FROM EQ BOX
  EQ Box is this app's old name. Save your EQ as a preset in EQ Box first
  (Save). When you install, setup sees the EQ Box engine and offers to
  remove it: every device goes back as it was, then the TAW Equaliser
  engine goes in its place. Load your preset in TAW Equaliser afterwards.

INSTALL
  1. Either press Install engine in TAW Equaliser's Settings (System:
     "Windows (system-wide)"), or double-click
     "Install system-wide engine.cmd" in this folder.
  2. Allow Windows' permission prompt.
  3. Pick your output from the list. Windows audio restarts (a second of
     silence). Run it again (Add in the app) to put the Equaliser on
     another output.
     If the output has its own effect from its driver (speaker tuning,
     "enhancements"), the Equaliser keeps it and runs it after the EQ.
     Windows protects each device's effect settings, so setup takes
     ownership of that one registry key (as other EQ software does). Its
     original owner and permissions are saved and put back on removal.

USE
  Open taw-equaliser.exe, set System to "Windows (system-wide)" in Settings
  (the gear), and turn on the On switch. Play music in any app on that
  output and move the EQ.
  The EQ stays set after the app closes. Use Bypass to switch it off.

REMOVE
  Press Remove in the app, or double-click "Remove system-wide engine.cmd".
  Every device goes back exactly as it was. It also removes an EQ Box
  engine, if one is still installed.

IF SOMETHING'S WRONG
  Double-click "Diagnose system-wide engine.cmd" and send the developer
  what it shows. It changes nothing.

  The scripts also run from an Administrator PowerShell in this folder:
       powershell -ExecutionPolicy Bypass -File .\install-engine.ps1
  (install-engine.ps1 -NoChain switches a misbehaving driver effect off
  while the Equaliser is installed, instead of keeping it.)

IF A DEVICE GOES SILENT
  Settings > System > Sound > (your device) > Audio enhancements: Off
  bypasses the Equaliser straight away. Then run remove-engine.ps1.

WON'T AFFECT
  Apps using exclusive mode or ASIO, and devices with Audio enhancements
  turned off.

Files: Program Files\Timster's Audio Works\Equaliser (the engine and the
device backups), ProgramData\Timster's Audio Works\Equaliser (settings).
