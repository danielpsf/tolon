# Install Tolon

Tolon is a beta. Download the file for your system from the Releases page, then follow the steps for your OS. Tolon never writes to your calendars and sends nothing anywhere.

## macOS (Apple silicon and Intel, one universal build)

Tolon is signed by its author, not by Apple, because the project has no paid Apple Developer membership. macOS will say it cannot verify the developer. You approve it once.

1. Open the `.dmg` and drag Tolon into Applications.
2. Double-click Tolon. macOS says it cannot verify it. Click Done.
3. Open System Settings, then Privacy & Security, scroll to Security, and click Open Anyway next to Tolon. That button shows for about an hour after step 2.
4. Confirm with Open and your password. From now on Tolon opens normally.

Prefer the Terminal? After checking the download against `SHA256SUMS`:

```
xattr -dr com.apple.quarantine /Applications/Tolon.app
```

Updates arrive through Tolon itself and are verified with the project's signing key, so you should not have to repeat these steps.

## Windows

1. Run the `.msi` (or the `.exe` installer).
2. SmartScreen may say "Windows protected your PC" because the installer is not yet signed by a certificate authority. Click More info, then Run anyway.
3. Tolon starts in the system tray (bottom right). Click the leaf icon.

## Linux

AppImage, any distribution:

```
chmod +x Tolon_*.AppImage
./Tolon_*.AppImage
```

Debian and Ubuntu:

```
sudo apt install ./tolon_*_amd64.deb
```

The tray icon needs an AppIndicator-capable panel. On GNOME, install the AppIndicator extension; on KDE it works out of the box.

## Verify a download

```
sha256sum -c SHA256SUMS --ignore-missing
```

## Known limits

- macOS: the first launch needs the Open Anyway step above.
- macOS Spaces: the interrupt joins every Space and sits above full-screen apps, so it stays up when you switch Space. Join, snooze or dismiss it to close it on all screens.
- Windows focus: Windows stops background apps from taking keyboard focus. Tolon always puts the interrupt above every other window, but Windows may refuse it focus. Then Enter, Esc and D do nothing until you click the interrupt or press Alt+Tab to it; the buttons still work. An app in exclusive full-screen mode, such as some games, may still draw over it.
- Linux on Wayland: the compositor may not let Tolon cover the whole screen. Tolon then shows a notification with Join and Snooze and plays the alarm sound instead. X11 sessions get the full takeover.
- Linux without a display (a console login or an SSH session): Tolon cannot open a window and uses the same notification fallback.
- Google Calendar sign-in (beta 2) uses a test client; refresh tokens expire after 7 days unless you paste your own client ID from a Google Workspace Internal app.
