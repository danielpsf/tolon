# Manual checklist

For what CI cannot run. Sign off per release in `docs/ledger.md` with OS, version and your name.

## macOS
- [ ] S4.1 Interrupt shows over a full-screen app (test with QuickTime full screen) and covers the menu bar and Dock.
- [ ] S4.2 Two monitors: both show the interrupt; Esc on either hides both.
- [ ] S4.3 Interrupt has focus on open; Enter joins; D dismisses; Space marks already there.
- [ ] AC9.4 Start at login switch reflects System Settings, Login Items.
- [ ] AC12.3 Fresh user account: install from the DMG following `docs/install.md` only; reach first run.
- [ ] AC11.2 Install an update through the in-app updater; note whether Gatekeeper asks again.

## Windows
- [ ] S4.2 Two monitors.
- [ ] AC12.3 Fresh machine or VM: `.msi` through SmartScreen "More info, Run anyway"; reach first run.
- [ ] AC9.4 Start at login appears in Task Manager, Startup apps.

## Linux
- [ ] S4.4 On a Wayland session (GNOME), the fallback notification shows Join and Snooze and the sound plays.
- [ ] S4.1 On X11, the interrupt covers a full-screen browser video.
- [ ] AC12.3 `.AppImage` runs after `chmod +x`; `.deb` installs on Ubuntu 22.04 and 24.04.

## Every OS
- [ ] AC6.2 With the system output muted, an alarm makes no sound; the system volume is unchanged after an alarm.
- [ ] AC6.3 Settings, Alarms, "Play" previews the chosen sound once.
