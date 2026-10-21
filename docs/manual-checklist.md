# Manual checklist

For what CI cannot run. Sign off per release in `docs/ledger.md` with OS, version and your name.

## macOS
- [ ] S4.1 Interrupt shows over a full-screen app (test with QuickTime full screen) and covers the menu bar and Dock.
- [ ] S4.1 Interrupt shows over a native full-screen app on each display (two displays, one app full screen on each in turn), and no Space switch happens with Settings open on another Space.
- [ ] In the settings window, Cmd+V pastes into the iCal URL field and Cmd+C copies (Tolon has no menu bar, so the Edit shortcuts must still work).
- [ ] S4.2 Two monitors: both show the interrupt; Esc on either hides both.
- [ ] S4.3 Interrupt has focus on open; Enter joins; D dismisses; Space marks already there.
- [ ] AC9.4 Start at login switch reflects System Settings, Login Items. Turning it off in System Settings and reopening the page shows the switch off with the hint to allow Tolon in Login Items; switching off twice shows no error.
- [ ] AC9.4 A development build (`cargo run`) shows the switch off, and turning it on shows an error without a crash.
- [ ] AC12.3 Fresh user account: install from the DMG following `docs/install.md` only; reach first run.
- [ ] AC11.2 Install an update through the in-app updater; note whether Gatekeeper asks again.
- [ ] AC3.4 Sleep the Mac across an alarm time with a full-screen app open; after wake the interrupt shows over it within 15 s.

## Windows
- [ ] S4.2 Two monitors.
- [ ] AC12.3 Fresh machine or VM: `.msi` through SmartScreen "More info, Run anyway"; reach first run.
- [ ] AC9.4 Start at login appears in Task Manager, Startup apps. Disable it there, then turn the switch on in Tolon, and note what the switch shows (Windows keeps the Task Manager choice).
- [ ] S4.4 When the interrupt cannot take focus, the fallback notification shows Snooze and Join; each acts on that meeting only.

## Linux
- [ ] S4.4 On a Wayland session (GNOME), the fallback notification shows Join and Snooze and the sound plays.
- [ ] S4.1 On X11, the interrupt covers a full-screen browser video.
- [ ] AC12.3 `.AppImage` runs after `chmod +x`; `.deb` installs on Ubuntu 22.04 and 24.04.
- [ ] AC9.4 Turning start at login on writes `~/.config/autostart/dev.tolon.desktop` with `Exec=` pointing at the AppImage path, and Tolon starts after logging out and in.

## Every OS
- [ ] AC6.2 With the system output muted, an alarm makes no sound; the system volume is unchanged after an alarm.
- [ ] AC6.3 Settings, Alarms, "Play" previews the chosen sound once.
- [ ] AC6.1 The alarm sound repeats about every 30 s while the interrupt is open and stops at once when it closes.
- [ ] S4.4 An old fallback notification's Join or Snooze does nothing once a later meeting is open.
- [ ] AC4.5 L-038 With the language set to German, then Arabic, then Hebrew (Settings, Language), a fallback notification shows the title in that language (the meeting title isolated in ar and he), the start time in that locale's clock, and Snooze and Join in that language; after a quit and restart it still does before the settings window is opened.
- [ ] AC4.4 With an Arabic or Hebrew keyboard layout active, D dismisses the interrupt.
- [ ] L-039 With the system language set to Arabic or Hebrew, the interrupt and Settings mirror right to left; Hindi, Chinese and Japanese text renders in a system font, not boxes.
- [ ] AC7.1 The tray icon reads in a light and a dark menu bar or taskbar; its tooltip shows the next meeting and the minutes until it.
- [ ] AC7.2 Clicking the tray icon opens the panel under or above it on each display; a second click or clicking away hides it; Esc hides it and focus starts on the first action.
- [ ] S7.1 Pause for 1 hour shows the paused icon and "Paused until HH:MM"; a meeting in 20 minutes does not fire; the pause holds across a quit and restart (icon included); Resume and the automatic end restore the normal icon.
- [ ] S7.2 Closing the settings window keeps the tray icon and an alarm still fires; Settings in the panel brings the window back (no Dock icon on macOS).
- [ ] AC7.4 Quit in the panel removes the tray icon and ends the process; Cmd+Q in the settings window also quits cleanly; logging out is not blocked by Tolon.
