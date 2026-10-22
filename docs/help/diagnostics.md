# Diagnostics: help us fix a problem

When Tolon does something wrong, a diagnostics file helps us find out why. Tolon writes it on your computer only. It sends nothing. You decide if you send the file to us.

## How to save the file

1. Open Tolon's settings and go to About.
2. Under "Help us fix a problem", click Save diagnostics.
3. Choose where to save the file. Tolon suggests your Downloads folder and a name like `Tolon-diagnostics-0.1.0-beta.1-20261022-1405.zip`.
4. Tolon shows where it saved the file. Click Show in Finder (macOS), Show in Explorer (Windows) or Show in Files (Linux) to find it.

## How to send it

Open an issue at https://github.com/danielpsf/tolon/issues and attach the file. Say what happened, when it happened, and what you expected. The time helps us find the right lines in the logs.

You can open the file before you send it. It is a normal zip file with text files inside.

## What is in the file

- `README.txt`: this page.
- `environment.json`: the version of Tolon, your operating system and its version, the processor type, your language, the time zone, the number of screens with their sizes and positions, and the version of the system web view.
- `settings.json`: your Tolon settings, such as the lead time, the sound, quiet hours and the appearance. For the ignore words, only how many there are, not the words.
- `calendars.json`: for each calendar, Tolon's own id for it, the kind (iCal link or Google), for an iCal link only the server name (for example `calendar.google.com`), if it is on, if the last sync worked, the last error as a code (for example `http` and `404`), and the times of the last sync.
- `sync-log.json`: the recent sync attempts: when, which calendar (by Tolon's id), and the result as a code.
- `alarms.json`: the alarms of the last 7 days and the coming days: when each one rings, when its meeting starts, its kind (before the meeting or at the start), and what happened (snoozed, dismissed, joined, expired). Each alarm has a random-looking code instead of its meeting.
- `logs/`: Tolon's log files of the last 7 days, at most 10 MB. They are at debug level for Tolon's own code, so they show each step the scheduler takes, with times in UTC. Libraries that Tolon uses log at info level only.

## What is never in the file

- Your calendar addresses. An iCal link can work like a password, so only its server name is kept.
- Sign-in tokens, such as your Google sign-in.
- Your Google account address, or any other email address.
- Meeting titles, descriptions, locations and links. When a call link does not open, a log line names only its server, such as `zoom.us`.
- The people invited to your meetings.
- The names of your calendars.
- Your ignore words.

## Where the log files are

Tolon keeps its log files in a `logs` folder in its data folder:

- macOS: `~/Library/Application Support/dev.tolon.desktop/logs`
- Windows: `%APPDATA%\dev.tolon.desktop\logs`
- Linux: `~/.local/share/dev.tolon.desktop/logs`

Tolon deletes files older than 7 days, and old files go first when the folder reaches 10 MB.
