# Diagnostics: help us fix a problem

When Tolon does something wrong, a diagnostics file helps us find out why. Tolon writes it on your computer only. It sends nothing. You decide if you send the file to us.

## How to save the file

1. Open Tolon's settings and go to About.
2. Under "Help us fix a problem", click Save diagnostics.
3. Choose where to save the file. Tolon suggests your Downloads folder and a name like `Tolon-diagnostics-0.1.0-beta.1-20261022-1405.zip`.
4. Tolon shows where it saved the file. Click Show in Finder (macOS), Show in Explorer (Windows) or Show in Files (Linux) to find it.

## How to send it

Send the file by email to tolon@danielpsf.net. In About, click the address: your mail app opens a new email with the subject and a short template. Say what happened, when it happened (with the time zone), and what you expected instead. Then attach the file yourself. The time helps us find the right lines in the logs.

If you prefer a public report, open an issue at https://github.com/danielpsf/tolon/issues and attach the file there. Anyone can read a GitHub issue.

You can open the file before you send it. It is a normal zip file with text files inside.

## What is in the file

- `README.txt`: this page.
- `environment.json`:
  - the version of Tolon (`tolon_version`), the kind of build (`build`: release or debug) and the version of Tauri, the framework Tolon is built on (`tauri_version`);
  - your operating system and its version (`os`, `os_version`) and the processor type (`arch`);
  - the language Tolon shows (`locale`) and the languages of your system (`system_languages`);
  - the time zone (`time_zone`), its offset from UTC (`utc_offset`) and when you saved the file (`saved_at`);
  - the number of screens with their sizes and positions (`screen_count`, `screens`);
  - the version of the system web view (`webview`) and the version of Tolon's database (`database_schema`).
- `settings.json`: your Tolon settings, such as the lead time, the sound, quiet hours and the appearance, and until when alarms are paused (`paused_until`). For the ignore words, only how many there are (`keyword_count`), not the words.
- `calendars.json`: for each calendar, Tolon's own id for it, the kind (iCal link or Google), for an iCal link only the server name (for example `calendar.google.com`), if it is on, if the last sync worked, the last error as a code (for example `http` and `404`), when the last sync ran, and when you added the calendar (`added_at`).
- `sync-log.json`: the recent sync attempts: when, which calendar (by Tolon's id), and the result as a code.
- `alarms.json`: the alarms of the last 7 days and the coming days: when each one rings, when its meeting starts, its kind (before the meeting or at the start), and what happened (snoozed, dismissed, joined, expired). Each alarm has a code instead of its meeting. The code comes from a secret that Tolon makes on your computer and never puts in the file, so nobody can work out the meeting from the code.
- `logs/`: Tolon's log files of the last 7 days, at most 10 MB. They are at debug level for Tolon's own code, so they show each step the scheduler takes, with times in UTC. Libraries that Tolon uses log at info level only. Your home folder's path is written as `~`.

## What is never in the file

- Your calendar addresses. An iCal link can work like a password, so only its server name is kept.
- Sign-in tokens, such as your Google sign-in.
- Your Google account address, or any other email address.
- Meeting titles, descriptions, locations and links. When a call link does not open, a log line names only its server, such as `zoom.us`.
- The people invited to your meetings.
- The names of your calendars.
- Your ignore words.
- The path of your home folder, which often contains your name.

## Where the log files are

Tolon keeps its log files in a `logs` folder in its data folder. On macOS and Linux only your user account can open that folder.

- macOS: `~/Library/Application Support/dev.tolon.desktop/logs`
- Windows: `%APPDATA%\dev.tolon.desktop\logs`
- Linux: `~/.local/share/dev.tolon.desktop/logs`

Tolon deletes files older than 7 days, and old files go first when the folder reaches 10 MB. If one day's log alone reaches 10 MB, Tolon writes one line saying so and stops writing until the next day.
