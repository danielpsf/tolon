# Manual checklist

For what CI cannot run. Sign off per release in `docs/ledger.md` with OS, version and your name.

## macOS
- [ ] S4.1 Interrupt shows over a full-screen app (test with QuickTime full screen) and covers the menu bar and Dock.
- [ ] S4.2 Two monitors: both show the interrupt; Esc on either hides both.
- [ ] S4.3 Interrupt has focus on open; Enter joins; D dismisses; Space marks already there.
- [ ] AC9.4 Start at login switch reflects System Settings, Login Items.
- [ ] AC12.3 Fresh user account: install from the DMG following `docs/install.md` only; reach first run.
- [ ] AC11.2 Install an update through the in-app updater; note whether Gatekeeper asks again.
- [ ] AC3.4 Sleep the Mac across an alarm time with a full-screen app open; after wake the interrupt shows over it within 15 s.

## Windows
- [ ] S4.2 Two monitors.
- [ ] AC12.3 Fresh machine or VM: `.msi` through SmartScreen "More info, Run anyway"; reach first run.
- [ ] AC9.4 Start at login appears in Task Manager, Startup apps.
- [ ] S4.4 When the interrupt cannot take focus, the fallback notification shows Snooze and Join; each acts on that meeting only.

## Linux
- [ ] S4.4 On a Wayland session (GNOME), the fallback notification shows Join and Snooze and the sound plays.
- [ ] S4.1 On X11, the interrupt covers a full-screen browser video.
- [ ] AC12.3 `.AppImage` runs after `chmod +x`; `.deb` installs on Ubuntu 22.04 and 24.04.

## Every OS
- [ ] AC6.2 With the system output muted, an alarm makes no sound; the system volume is unchanged after an alarm.
- [ ] AC6.3 Settings, Alarms, "Play" previews the chosen sound once.
- [ ] AC6.1 The alarm sound repeats about every 30 s while the interrupt is open and stops at once when it closes.
- [ ] S4.4 An old fallback notification's Join or Snooze does nothing once a later meeting is open.
- [ ] AC4.4 With an Arabic or Hebrew keyboard layout active, D dismisses the interrupt.
- [ ] L-039 With the system language set to Arabic or Hebrew, the interrupt and Settings mirror right to left; Hindi, Chinese and Japanese text renders in a system font, not boxes.
