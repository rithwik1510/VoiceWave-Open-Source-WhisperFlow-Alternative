# VoiceWave privacy notice

This notice covers the VoiceWave app for Windows, as of version 0.7.7.

VoiceWave turns your speech into text on your PC. Your audio and the text you dictate never leave
it. There is no VoiceWave account and no VoiceWave server.

## What can be sent

Only while **Anonymous usage and problem reports** is on, and only to
[PostHog](https://posthog.com), on its EU Cloud, in a project set to discard IP addresses. Every
event also tells PostHog not to look up a location.

Apart from two random IDs, each value is picked from a fixed list written into the app, or is a
number, a yes or no, or a version number. Nothing you type, say or open is ever a value.

Every report carries a random install ID and a random ID for each launch, the app version, Windows
10 or 11, your graphics card maker, whether speech runs on the graphics card or the processor and
which model family, how you installed VoiceWave (winget, a direct download, or unknown), which
version you first installed and how many days ago (estimated for installs older than 0.7.7), and
whether the PC is a virtual machine. On top of that, VoiceWave sends only:

- when the app starts: whether it started with Windows, and whether the last run ended normally,
  crashed, hit an internal error, or was cut off by a restart or an update, what it was doing then
  and how long it had run;
- setup steps, and whether setup was finished or skipped;
- once for each day you dictate: how many dictations were started, worked, came back empty, failed,
  heard no speech, were retried or were hands free; how fast text appeared; how long you spoke; how
  many words were inserted; how often speech ran on the graphics card; how the text got in (typed,
  pasted, left on the clipboard, kept in History only, or held back from an app running as
  administrator); how many went into each kind of app (browser, code editor, terminal, chat, office,
  notes, AI app, or other); and your formatting style;
- whether your first dictation ran on the graphics card or the processor;
- failed dictations by type, with the kind of app when the text couldn't be inserted;
- speech helper crashes by cause, internal errors by area, and graphics card to processor fallbacks;
- model downloads: which model, how they ended, where a failed one stopped, retries, how far they
  got and how long they took;
- failed updates: the step, the kind of error and the version;
- shortcut setup problems;
- that you chose to share, at the end of setup or on "A quick privacy check" (choosing not to
  share sends nothing);
- your answer to "Where did you hear about VoiceWave?", asked once at the end of setup on a new
  install;
- the reason you pick in "Something wrong?", and where you opened it.

The same list is under **What's sent**, below the switch in the app.

## What is never sent

Audio, transcripts, dictionary or snippet content, window titles, app names, file paths, device
names, error messages, account details, or your IP address.

When text goes into an app, the app's name is turned into a kind of app (such as browser or code
editor) on your PC and dropped there. Errors are turned into a type on your PC in the same way, and
their text is never sent.

## When it starts

- **New installs:** on by default. The first setup screen shows the switch. Reports made during
  setup wait on your PC until you finish or skip setup, and are deleted if you turn the switch off.
- **If you shared the older usage count before 0.7.7:** "A quick privacy check" asks once whether
  to keep sharing. Until you answer, reports wait on your PC and nothing is sent. Keep sharing sends
  them; Turn off deletes them. The question comes back each time you open VoiceWave until you
  answer, and since 0.7.8 the pill also asks once per launch after a dictation works.
- **If you had it off:** nothing changes. You aren't asked, and nothing is sent.

Reports made while you're offline are kept in a file on your PC and sent later, with the time they
happened. At most 500 wait at once, and any queued more than 14 days ago are dropped.

## Your choice

One switch: **Settings → Diagnostics → Anonymous usage and problem reports**. Turning it off stops
all of it right away and deletes anything waiting to be sent. You can turn it back on anytime.

## Identifiers

The install ID is a random number made on your PC. It is not linked to your name, email, account,
hardware or PC name. It is kept in the Windows registry under
`HKEY_CURRENT_USER\Software\VoiceWave\LocalCore`, so it survives updates and reinstalls.
Uninstalling VoiceWave with the option to delete the application data ticked removes it. The
launch ID is new each time VoiceWave starts.

## Reporting a problem yourself

**Help → Report a problem** starts with "Something wrong?". You pick what happened, and if the
switch is on, the reason you picked is sent as above. With the switch off, nothing is sent and the
app says so.

**Tell us more on GitHub** opens GitHub's issue form, filled in with the reason you picked and the
diagnostics report: app and Windows versions, speech engine, microphone name and recent warnings
from the log. Opening the form sends that text to GitHub so it can fill in the form. Nothing is
published until you submit the issue.

## Support file

**Save support file** (in Settings → Diagnostics, after "Something wrong?", and on Home after
VoiceWave closed unexpectedly) writes `voicewave-support-YYYYMMDD-HHMM.zip` to your Downloads
folder. VoiceWave never uploads it. It stays on your PC unless you attach it to an issue yourself.

It contains:

- `README.txt`: what's inside;
- `report.txt`: the same text as Copy diagnostics (app and Windows versions, speech engine,
  microphone name and recent warnings);
- `diagnostics.json`: speech engine checks and recent dictation timings;
- `logs/`: the newest 3 daily logs, the last 2 MB of each;
- `panics.log`: where internal errors happened, if any were noted;
- `session.json`: how the last run ended, only if it closed unexpectedly.

Your Windows user folder appears as `%USERPROFILE%` in the reports and logs. The contents are
capped at 8 MB. It never contains audio, anything you dictated, your history, dictionary or
snippets, settings, models or saved sign ins. It does contain your microphone's name and Windows
version, so look through it before you share it.

## TelemetryDeck in 0.7.7

0.7.7 also still sends the older anonymous usage count through
[TelemetryDeck](https://telemetrydeck.com); that stops in the next release. It follows the same
switch and uses its own random ID, and what it counts is part of the list above.

## Contact

Open an issue on
[GitHub](https://github.com/rithwik1510/VoiceWave-Open-Source-WhisperFlow-Alternative/issues).
