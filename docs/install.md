# Install Tolon

Tolon is a beta. Download the file for your system from the Assets list of the release, then follow the steps for your system below. It takes about two minutes, including your first calendar. Tolon only reads your calendars. It has no analytics and sends no data about you. It contacts your calendar links and GitHub, to check for updates.

| System | File to download |
| --- | --- |
| macOS, Apple silicon and Intel | `Tolon_<version>_universal.dmg` |
| Windows | `Tolon_<version>_x64-setup.exe` (or the `.msi`, if your IT team asks for it) |
| Linux | `Tolon_<version>_amd64.AppImage` (any distribution) or `Tolon_<version>_amd64.deb` (Debian, Ubuntu) |

## macOS

Tolon is signed by its author, not by Apple, because the project has no paid Apple Developer membership. macOS therefore says it cannot check Tolon for malware. You approve it once.

1. Open the `.dmg` and drag Tolon into Applications.
2. Open Tolon from Applications. macOS shows a warning that it could not verify Tolon. Click Done. Do not click Move to Trash.
3. Open System Settings, click Privacy & Security, and scroll down to Security. Click Open Anyway next to Tolon. The button shows for about an hour after step 2. If it is gone, repeat step 2.
4. Type your Mac password and click OK. Tolon opens its first-run window. From now on Tolon opens like any other app.

If you prefer the Terminal, check the download against `SHA256SUMS` first (see "Check your download" below). Then run this line, which removes the quarantine mark that causes the warning:

```
xattr -dr com.apple.quarantine /Applications/Tolon.app
```

## Windows

1. Double-click the `.exe` you downloaded.
2. Windows may show "Windows protected your PC". Tolon is not yet signed by a certificate authority, so Windows does not know the publisher. Click More info, then click Run anyway.
3. Follow the installer. Tolon opens its first-run window.
4. After that, Tolon lives in the system tray at the bottom right. If you do not see its icon, click the small arrow to show hidden icons.

## Linux

AppImage, any distribution:

1. Open a terminal in the folder with the download.
2. Make the file runnable, then start it:

```
chmod +x Tolon_*.AppImage
./Tolon_*.AppImage
```

If it does not start and mentions FUSE, install it once with `sudo apt install libfuse2` (Debian and Ubuntu) and try again.

Debian and Ubuntu, with the `.deb`:

```
sudo apt install ./Tolon_*_amd64.deb
```

Then start Tolon from your applications menu. It opens its first-run window.

The tray icon needs a panel that supports AppIndicator icons. KDE works out of the box. On GNOME, install the AppIndicator extension.

## Check your download

This step is optional. It proves the file was not changed on the way to you. Download `SHA256SUMS` from the same release into the same folder as your installer, then run one of these:

```
shasum -a 256 -c SHA256SUMS --ignore-missing      # macOS
sha256sum -c SHA256SUMS --ignore-missing          # Linux
```

On Windows, run `Get-FileHash <file>` in PowerShell and compare the result with the line for your file in `SHA256SUMS`.

## Updates

Tolon checks GitHub for a new version and asks before it installs one. Every update is checked against the project's signing key first.

## Known limits

- macOS: the first launch needs the Open Anyway step above.
- macOS Spaces: the interrupt joins every Space and sits above full-screen apps, so it stays up when you switch Space. Join, snooze or dismiss it to close it on all screens.
- Windows focus: Windows stops background apps from taking keyboard focus. Tolon always puts the interrupt above every other window, but Windows may refuse it focus. Then Enter, Esc, D and Space do nothing until you click the interrupt or press Alt+Tab to it; the buttons still work. An app in exclusive full-screen mode, such as some games, may still draw over it.
- Linux on Wayland: the compositor may not let Tolon cover the whole screen. Tolon then shows a notification with Join and Snooze and plays the alarm sound instead. X11 sessions get the full takeover.
- Linux: Tolon needs a graphical session.
- Google Calendar sign-in (beta 2) uses a test client; refresh tokens expire after 7 days unless you paste your own client ID from a Google Workspace Internal app.
