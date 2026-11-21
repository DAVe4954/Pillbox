# Pillbox: developer reference

The long technical notes behind Pillbox: how each feature works, the settings file, the code
layout, measurements and the fragile spots. (The top-level [README](../README.md) is the
tutorial and the overview.) These notes were written while building it on the author's own PC,
so "this PC" and "this laptop" mean that machine (Windows 11, no Wi-Fi adapter, a Windows
Insider build). Pillbox was called "Island" until version 0.1.0; the old name still shows up
in a few history notes.

Status: **M1 (shell), M2 (look and layout), M3 (music, lyrics, lock-key notices), M4 (daily
tools), M5 (fullscreen games, stats strip with GPU, play time, chat drawer), M6 (phone
transfer over the local network) and M7 (script API, `pillboxctl`, coding agents) done.** M8
(notification mirror) is optional and asked about item by item: the notification list, the
stack on the closed pill, real app icons, the duration setting, flipping with the wheel, per-app
rules and opening the source app are all built (off by default).

Words used for the M7 and M8 parts are in [GLOSSARY.md](../GLOSSARY.md): **flash**, **status**, **agent**,
**session**, **notification**.

## Build and run

Needs Rust stable (MSVC toolchain) and the Visual Studio 2022 C++ Build Tools with a Windows 11 SDK.

```
cargo run --release
cargo build --release -p pillboxctl     # the command line for scripts and agent hooks (M7)
```

`pillboxctl.exe` ends up next to `pillbox.exe` in `target\release`. Run `pillboxctl help` for its
commands.

Other useful commands:

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Logs are rewritten on each start: `%LOCALAPPDATA%\Pillbox\pillbox.log` (Pillbox) and
`settings.log` (the Settings window).

## What works

**Shell (M1)**
- Pill at the top centre; rest the pointer on it and it springs open into a card, move away and it closes.
- Never takes focus, never activates, no taskbar button, always on top.
- Clicks outside the pill shape (including rounded-corner cut-outs) go to the app underneath.
- Tray icon: Settings, Hide/Show Pillbox, Start with Windows, Quit.
- **App icon:** a violet-to-pink rounded square with the black pill on it and the opened card
  under it. It is drawn in code (`pillbox_core::appicon`), so there is no picture file: the same
  drawing is the icon of `pillbox.exe` in Explorer (embedded by `crates/Pillbox/build.rs` as a
  resource, six sizes from 16 to 256 px), the tray icon (drawn at the size the notification area
  asks for), and the taskbar/Alt+Tab icon of the Settings window (set on the window itself, so it
  shows even before Windows has read the exe's resource). `cargo run -p pillbox-core --example
  appicon -- <folder>` writes `pillbox.ico` and PNGs of it.
- **Settings and quit buttons:** at the right end of the tab row of the open card, a gear (opens
  the Settings window) and a small X. The first click on the X
  turns it red, the second quits the whole app (the Claude hooks go quiet until it is started
  again). Closing the card or clicking anything else takes the first click back.
- Single instance. Zero rendering and zero polling while idle.

**Look and layout (M2)**
- Shape: floating pill or a notch attached to the edge; one Size setting (80–140%) that scales
  the whole Pillbox, closed and open, text included; distance from the edge; top, left or right
  side; choice of monitor.
- Surfaces: Black, Graphite, Pure black, Acrylic, Liquid Glass, Candy glass, and the soft themes
  Rosé (hearts progress bar), Lavender and Peach (sparkles), Sakura (falling petals).
  Accent colour and glass blur.
- Glass surfaces show the live, blurred screen behind Pillbox; Liquid and Candy glass also
  bend it near the rim. Sampling stops when the screen behind is static.
- Hide from screenshots and recordings (always on for glass surfaces).
- **App themes** (`appearance.app_theme`, Settings > Appearance): Midnight, Ocean, Forest,
  Sunset, Violet, Ember, Mono recolour the card: its fill and rim, the text, the accent, the
  highlight behind the active tab (`Theme::chip`) and the wash the tiles, rows and bars of the
  list tabs are drawn with (`Theme::wash`, so Home, Clipboard, Reminders... all follow).
  `Theme::with_app_theme`: a glass surface keeps its material (blur, bend, gloss) and takes the
  theme's colour as its tint; the soft surfaces lose their decoration (hearts, petals) under an
  app theme. "Surface colours" (the default) changes nothing.
- **Fade themes** (M9): Aurora, Golden hour, Deep sea, Midnight neon, Forest mist, Lava, Mono
  fade and Cotton candy are app themes of three colours (`theme::Fade`: stops, an angle and a
  softness; `fade_palette` in `theme.rs` holds each one's stops, text, accent, wash and rim).
  `scene::paint_body` draws every body (pill, card, widget windows): the fill (or the blurred
  screen on glass), the fade over it at `Theme::fade_alpha` (on glass it replaces the tint, at
  `GLASS_FADE_ALPHA` × strength), a shimmer's light, then the sheen (`Theme::sheen`, 4.5% white
  over the top half of dark and coloured bodies) and the rim, lit from above (`paint_rim`:
  135% of its alpha at the top, 55% at the bottom). The fade line works like a CSS linear
  gradient (`Fade::line`): through the centre at the angle, long enough that the corners get
  the end colours. Softness narrows each colour change to a band around the midpoint of two
  stops, eased by two extra stops on a smoothstep curve (`Fade::brush_stops`).
- **Your fade** (`app_theme: custom`, `appearance.fade`): 2 or 3 colours with positions, angle,
  softness and strength (opacity). `theme::readable_fade` picks light or dark text, whichever
  needs the colours changed least, and pushes any stop too close to the text colour away from
  it (darker under light text, lighter under dark text) until body text has 4.5:1 and dim text
  3:1 (WCAG contrast, `theme::contrast`) on every part of the fade as drawn over the surface.
  The accent is moved towards the text colour until it has 3:1 (`stand_out`). Glass is
  checked against a middle grey (`GLASS_BACKDROP`), since what is behind it is unknown: over a
  very bright or very dark window the contrast on glass can be lower than that.
- **Motion** (`appearance.animate`, on by default): Aurora (36 s) and Lava (28 s) drift: each
  stop walks round the ring of the theme's colours and the angle sways ±12°, redrawn 5 times a
  second (`DRIFT_FPS`). Midnight neon shimmers: a soft light crosses the body in 2.4 s every
  12 s at 30 fps, and nothing is drawn in between (`Motion::next_frame_in`). Motion runs only
  while the body is in sight: not with Pillbox hidden, in strip or hidden mode (a fullscreen
  app or an app rule), slid away by hide-until-hover, or with Windows' animation effects off
  (`system::animations_enabled`, read when the theme is built). Widget windows show the fade
  still. Measured on this PC (10 s, cycle counts): a still fade costs what Black does (about
  0.01% of one core); Aurora drifting 0.53%, Midnight neon 0.27%; paused, 0.01% again.
- **Settings previews** are pictures drawn from `Theme::body_painter` (the same layers as
  `paint_body`, pixel by pixel) and uploaded as egui textures, one per slot, redrawn only when
  the theme or size changes. The Preview card moves with a moving theme.
- **Polish** (M9): the active tab's highlight slides between tabs and the new tab's content
  eases in (springs `CHIP_SPRING`, `TAB_IN_SPRING`, same stiffness as the card); closing runs
  1.45× faster than opening (`CLOSE_SPEED`); tab icons, the settings gear and the quit button
  have a hover look; every button, row and tile shows a pressed layer while the button is held
  (`scene::paint_pressed`); scrolling lists fade out at an edge with more beyond it
  (`Scroller::clip`, a Direct2D layer with an opacity brush).
- **Closed pill text** (`appearance.clock`): size (80–130%, the pill widens to fit), bold,
  seconds (a 250 ms timer, redrawing only when the second changes), leading zero, and colour
  (the surface's, the accent, or your own). It applies to the clock and the other text faces.
- Resting face: nothing, time, date, weather, battery, CPU, next event, pixel pet. The time face
  can add the weekday ("Fri 14:05") or the date ("20 Nov 14:05"): `behavior.pill_clock`,
  `face::format_pill_clock`.
- Hidden until hover: Pillbox slides off screen until the pointer touches the screen edge.
  **What brings a hidden Pillbox into view** (`layout.reveal`, Settings > General > "Show a hidden
  Pillbox when"): music (never / while a song plays / for `song_seconds` when a song starts or
  changes), a Windows notification, an agent or script flash, a call, a ringing reminder, a short
  notice. Each is a switch and all are on by default; it hides again when the reason ends
  (`App::revealed`, checked on every retarget). A new song uses a one-shot timer
  (`TIMER_REVEAL`).
- Per-app rules: hide, shrink to the stats strip, or always show, for whichever app is in front.
  A rule always beats fullscreen detection (M5).
- **Settings window** (tray > Settings, or the gear on the card): a frameless dark window with
  a sidebar of ten pages (General, Appearance, Music, Widgets, Clipboard & Shelf, Notifications,
  Games & Apps, Agents, Shortcuts, About) and a search box that searches every page (each row's
  title, grey line and a few extra words; results are headed "Page · Section"). Every setting is
  a row in a card: icon tile, title, a grey line, the control on the right (switch, segmented
  buttons, slider with its value, drop-down, text box). Surfaces, visualizer styles and
  progress bar styles are picture cards. Shortcuts are recorded by pressing them. Fonts and
  icons are Windows' own (Segoe UI, Segoe Fluent Icons, read from `C:\Windows\Fonts`, with
  egui's fonts as the fallback). Windows 11 rounds its corners and gives it a dark frame
  (`platform::settings_window`).
  - **No Save button:** a change is written 300 ms after the last edit, once no slider is held
    and no text box is being typed in (text is written when its box is left or Enter is
    pressed), so a dragged slider writes once. Closing the window writes anything waiting.
  - **It never undoes Pillbox's own writes:** Pillbox writes `widgets.system_window`
    itself (its ✕, the tray). The window checks the file once a second and takes changes made
    elsewhere; unless the System window switch was changed in the window, a save keeps the
    file's value.
  - **Weather location by name:** a city search (Open-Meteo's geocoding, the same provider;
    only the typed name is sent, only when Search is pressed) fills latitude, longitude and the
    place name (`weather.place`); the coordinates can still be typed.
  - Pillbox picks up changes to the settings file immediately, including hand edits.
- The card's Home tab is a placeholder until M4: clock, date and a day-progress bar.

**Music (M3)**
- Now playing from any app that reports playback to Windows (Spotify, browsers, Apple Music,
  Media Player, VLC, foobar2000 with its SMTC component, ...). Event-driven, no polling.
- Resting pill while music plays: cover, optional song title, and a visualizer (pixel bars or
  rounded bars) driven by the real audio output.
- **The resting pill sizes itself to what it shows** (top edge; on a side edge it stands upright
  and keeps its size). Logic and tests: `Placement::resting` in `pillbox-core/src/shape.rs`.
  - *Nothing on it:* as small as the clock (or the chosen face) allows: text + one pill height
    of air, at least 88 px.
  - *A notification, flash, notice or call:* a fixed width, `shape::NOTICE_W` (200 at size
    100%).
  - *A song playing:* cover, title and bars, as wide as the title needs, the title cut at 30
    characters with "…" (`music::pill_title`); at least 230 px, at most 480 px, the same height
    as always (a 4 px taller song pill was tried and dropped), title at 40% of the height (the
    clock is 44%). Without a title (`music.title_in_pill` off) just cover and bars. With the
    visualizer off, no room is kept for the bars (`music_pill_metrics(.., bars: false)`).
  - Paused music shows the clock again. Changes glide with the same springs as opening.
  - Sizing and drawing share `shape::music_pill_metrics`, and the text is measured with the
    renderer's DirectWrite (`Renderer::measure`, cached) before the frame, so the title the
    pill widened for fits exactly.
- **The song's colours on the resting pill** (`music.pill_colors`, on by default): while a song
  with a cover plays on it, the pill takes the cover's colours. `music::palette` finds up to three distinct ones (hue groups of
  30°, each at least 60° from the others and at least 2.5% as present as the main one, so a
  small accent on a cover that is 90% one colour still shows; specks under about 0.15% colour
  weight per pixel do not count); a cover with fewer gets softer shades of its main colour
  (`music::mute`, same hue, less saturation; never a made-up hue, which turned red covers
  pink), a grey cover stays grey. Pale colours are made richer (`richer`: saturation 0.15 →
  0.46, 0.6 → 0.8, near-grey left alone). Each is darkened to a relative luminance of at most
  0.2 (`music::pill_tint`; white
  text keeps about 4:1 contrast). They flow slowly along the pill as a left-to-right gradient
  (`music::pill_stops`, one loop every 24 s; the pill redraws anyway while music plays, so
  this costs nothing extra), with a darker bottom for depth, a soft white sheen on the upper
  half and a rim in the same flowing colours. It fades in and out and blends from one song's
  colours to the next (ten extra springs: amount and red, green, blue of each colour), and
  only shows while music plays on the resting pill: not when paused, not behind a
  notification, not on the open card (which has its own cover background).
- **Visualizer styles** (`behavior.visualizer`, Settings > Music): `off` (no bars, and the audio
  capture does not run), `pixel_bars` (default,
  cells stacked like a level meter), `rounded_bars` (grow from the middle), `equalizer`
  (rounded bars standing on the bottom), `burst` (lowest band in the middle, higher ones
  outwards, so it swells from the centre), `wave` (a smooth mirrored sound wave through the
  bands, tapered at the ends) and `dots` (six dots that rise and swell). With a cover they are
  coloured across the cover's colours (lifted towards white), drifting with the pill; without
  one, in the accent colour.
- Card, Music tab: cover, title, artist, previous / play-pause / next, elapsed and total time,
  and a progress bar you can click or drag to seek. Five bar styles (thin line, thick bar, dots,
  wave, segments); the Rosé theme uses hearts. Blurred cover as the card background (optional).
  Controls a player does not support are dimmed.
- Choose which apps the card follows: every app, only the listed ones, or all but the listed ones.
- **Lyrics are on the Music tab** (`music.lyrics`, off by default; the separate Lyrics tab is
  gone). With them on, the Music card is wider (`shape::LYRICS_CARD_W` × `LYRICS_CARD_H`, 580 ×
  214, top edge only; a side edge has no room and keeps the compact player): the player on the
  left (cover, title, artist, progress bar with the time left, previous / round play-pause /
  next), the words on the right. Time-synced lines from LRCLIB, looked up once per song and
  cached on disk; music videos are skipped.
  - The line being sung is white and the others grey; long lines wrap (`wrap_text`, measured
    with the renderer's DirectWrite); lines of different heights are laid out with
    `lrc::line_offset`. Click a line to jump there.
  - **A pause in the singing is three dots** that fill one by one as the pause runs out
    (`Lyrics::gap_progress`): the empty lines LRCLIB has for instrumental gaps, plus one before
    the first line when the song opens with 5 s or more of intro (`Lyrics::with_intro_gap`).
  - **The wheel over the words scrolls them** (two lines a notch, `App::scroll_lyrics`). Once
    scrolled away from the live line a **Return to live** button shows (an arrow points to the
    live line); clicking it, or scrolling back onto the live line, follows the song again. The
    view also resets when the song changes or the card closes.
  - **Reactive background** (`music.reactive_background`, on by default): the cover's colours
    (the same as the pill's, dark enough for white text) spread across the card and each part
    brightens with the bass, middle or treble of the music at that place (`music::glow_step`:
    rises fast, falls slowly), flashing when a new lyric line starts (`music::pulse_step`). It
    follows the audio and the line changes, not the meaning of the words. The audio capture runs
    for it only while the Music tab is open (`sync_music`).
- "Reopen tab" setting: open on the last tab used, or on Music whenever something is loaded.
- Notices on the pill when Caps Lock, Num Lock or Scroll Lock changes.

**Daily tools (M4)**
- The card has icon tabs: Home, Music, Clipboard, Shelf, Reminders, Controls. List tabs
  use a taller card (`shape::TALL_H`); lists scroll with the mouse wheel.
- **Wheel over the tab row switches tabs:** with the pointer on the row of tab icons (its height,
  from just left of the first icon to the card's right edge, gear and X included), wheel down goes
  to the next tab and up to the previous. It stops at the first and last tab instead of wrapping.
  Touchpad scrolling adds up until it makes a whole notch; turning the other way starts over.
  Anywhere else on the card the wheel still scrolls the list. Logic and tests:
  `pillbox-core/src/wheel.rs`.
- **Typing into Pillbox:** clicking a text box (clipboard search, reminder box, to-do, quick
  note, calculator) gives Pillbox the keyboard; Esc, clicking elsewhere, or the card closing
  hands it back to the window that had it. Nothing else ever takes focus. The card stays open
  while a box has the keyboard. Keys: arrows, Home/End, Backspace/Delete, Ctrl+Backspace,
  Ctrl+Left/Right, Ctrl+V, Enter, Esc.
- **Global shortcuts** (rebindable in Settings, checked as you type; see Hotkeys below). A
  shortcut another app already owns is reported on the pill and in the log.
- **Clipboard history:** text, links, pictures, files; sub-tabs All / Text / Links / Pictures /
  Files / Pinned / Snippets; search; click a row to copy it again; pin and delete buttons;
  **right-click a row to open it** (`cliphistory::open_target`): files and folders open in their
  own apps (at most 5 at once; "Those files are gone" if none exist), an `http(s)://` or
  `mailto:` link opens in the browser or mail app (anything else that looks like a link, a
  program path for example, is never handed to the shell to run: it opens as text), text opens
  in the text editor and a picture in the picture viewer. Text and pictures go through a plain
  copy in `%TEMP%\Pillbox\opened\clipboard-<id>.txt|png`, because the history itself is
  encrypted; copies older than a day are deleted the next time one is written. (Right-click
  used to pin; pinning is the row's pin button.) "Copy the text in this picture" (Windows OCR,
  offline); optional two-column list; snippets
  from Settings. Stored encrypted with DPAPI (`data\clipboard\`, readable only by your Windows
  account). Copies marked private by the copying app (`ExcludeClipboardContentFromMonitorProcessing`,
  `CanIncludeInClipboardHistory = 0`, `Clipboard Viewer Ignore`) are never read, nor copies from
  apps on the ignore list. The newest 30 unpinned pictures are kept.
- **Shelf:** drop files on Pillbox (it opens on the Shelf tab), drag them out again into any
  app (one, or all selected), click to select, copy them as files, remove or clear. Holds
  references, not copies. New screenshots go on the shelf (setting).
  Right-click a file for a menu drawn in the card:
  - Open
  - Open with… (Windows' own app chooser)
  - Show in folder
  - Download… (Windows' Save dialog in Downloads; saves a copy). Only for files that came from
    the phone, meaning they are in the phone's receive folder.
  - Send to phone
  - Remove from shelf

  Folders and missing files get fewer lines. A click outside the menu only closes it.
  (Right-click used to remove the file at once.)
- **Reminders:** quick-add box that understands "tea in 10m", "call mum at 18:30", "tomorrow
  9am dentist", "bins friday 7am", "daily 8:00 meds", "weekdays 9:30 standup", "every friday
  16:00 timesheet". When one is due the card opens on Reminders, even over an app with a hide
  rule, and the Windows alarm sound loops until Snooze or Done. Repeating ones move on to their
  next day. The "Next event" resting face shows the next reminder.
- **Calls and microphone:** Ctrl+Alt+Shift+M mutes every microphone at the Windows endpoint level
  (works in every app). While any app uses the microphone the pill shows a call timer, the app
  and a mute indicator; a small dot shows microphone (orange) or camera (green) use. Ignore list
  in Settings.
- **Controls tab:** speaker volume and mute, brightness (built-in panel via WMI, external
  monitors via DDC/CI), then two cards, **Wi-Fi** and **Bluetooth**, then tiles for the
  microphone (mute), Windows' sound page, the camera mirror and (when switched on) the chat
  drawer, the call line during a call, and six launcher slots with app icons.
  - *The cards:* the round switch on a card turns the radio on or off; its grey line says what
    is connected ("Home", "Headphones", "2 connected", "Off", "No Wi-Fi adapter").
    A click anywhere else on the card **expands** it: the card grows taller (the window is sized
    for it) and a list opens under the two cards. Click again to fold it; closing Pillbox
    folds it too.
  - *Wi-Fi list:* networks in range with signal bars, the connected one first, then saved ones,
    then the strongest. A saved network has a **Connect** button, the connected one a
    **Disconnect** button. A new network (password) cannot be joined from here: the row says so
    and "More in Windows" opens Windows' Wi-Fi page. The list is read through the Native Wifi
    API (`wlanapi`) on a worker thread, and looks again every 4 seconds only while it is open.
  - *Bluetooth list:* the paired devices (classic and Low Energy, one line per address), the
    connected ones first (the others say "Not connected"). **Audio devices** (headphones, earbuds, speakers) have **Connect** /
    **Disconnect**; mice, controllers, phones and the rest only show their state, because
    Windows reconnects them by itself and has no call to force it. "Add in Windows" opens the
    Bluetooth page for pairing.
  - The wheel scrolls a long list; a switch that fails (device off, out of range) says so under
    the list.
- **Camera mirror:** the Mirror tile shows the first webcam, flipped like a mirror, in the card.
  The camera runs only while the mirror is on screen; closing it, switching tab or closing the
  card releases the camera (its light goes off). If no picture arrives within 6 seconds (privacy
  switch, shutter, camera busy) the mirror says so and lets go.
- **Launcher:** up to six apps, files, folders or URLs, each with an optional global shortcut.
- **Home widgets** (choose and order in Settings): clock, 24-hour day clock with sunrise/sunset,
  world clock (IANA zone names), weather, air quality, sun and moon (computed locally), next
  reminders, to-do, quick note, calculator, focus timer, countdown, day and year progress, PC
  stats (CPU, RAM, network speed in Mbps or MB/s), battery, screen time today, ping, website
  check, currency rates, photos (random picture from a folder). A widget's corner button pops it
  out onto the desktop as its own small window: its buttons work there too (focus timer start,
  pause and reset, to-do ticks and delete, next photo), and so do its text boxes (to-do,
  calculator, quick note: click, type, Enter, Esc hands the keyboard back); pressing anywhere
  else drags it; right-click closes it; its place is remembered.
- **Weather look:** Minimal (icon and temperature) or Detailed (a wide tile: temperature, then
  the condition, today's high and low, feels-like, wind and humidity).
- **Screenshot** (Ctrl+Alt+S) of the monitor under the pointer, with Pillbox left out: copied
  to the clipboard, saved to `Pictures\Screenshots`, put on the shelf.
- **Colour picker** (Ctrl+Alt+C): copies the hex code of the pixel under the pointer and shows it.

**Games (M5a)**
- **Fullscreen detection:** when a fullscreen app is in front on Pillbox's monitor, Pillbox
  either shrinks to a stats strip or hides completely (setting). Borderless/windowed fullscreen
  is detected by the window's client area covering its monitor; Direct3D exclusive fullscreen by
  `SHQueryUserNotificationState`. Nothing can draw over exclusive fullscreen, so play games in
  borderless mode to see the strip.
- **Which apps count:** by default browsers, video players, slide shows, remote desktop and a few
  others are not treated as games (built-in list, plus `games.ignore_apps`). "Every fullscreen
  app" turns that off. Per-app rules win over all of this.
- **Stats strip:** CPU and RAM updated 10 times a second, download and upload twice a second
  (averaged over that half second, two significant digits), only while the strip shows. It
  redraws only when a shown number changes. On Pillbox's surface or as bare numbers with a shadow; optional usage bars under
  CPU and RAM (red above 90%). The strip never takes clicks: every click goes to the game.
- **Reminders over games:** a due reminder still opens the card over a game (or a hide rule);
  Done/Snooze returns to the strip or hidden state.
- **GPU on the strip (M5b):** the busiest GPU's 3D load and its temperature ("GPU 45% · 71°"),
  read twice a second on a worker thread while the strip shows. Load comes from the "GPU Engine"
  performance counters (what Task Manager shows); temperature from
  `D3DKMTQueryAdapterInfo(KMTQAITYPE_ADAPTERPERFDATA)`, Task Manager's own source. A driver that
  does not report temperature shows only the load.
- **Play time (M5b):** time with a fullscreen game in front is counted per game and per day
  (`data\playtime.json`, last 60 days, never sent anywhere). The Play time widget shows today,
  the last 7 days and the most played game, and "Playing <game>" while one runs. Names come from
  the executable (`FortniteClient-Win64-Shipping.exe` shows as "Fortnite"). Counted whenever
  fullscreen detection says "game", whatever Pillbox shows; not counted with detection off.
  **Only games count:** browsers, media players, game launchers and their helper processes
  (`steamwebhelper.exe`, `epicgameslauncher.exe`...), Discord, Spotify, office apps and similar
  (the `NOT_GAMES` list in `pillbox-core/src/games.rs`) are never counted, even with "every
  fullscreen app" on, and such entries found in an older `playtime.json` are removed when
  Pillbox starts.
  - *Expanding it:* a click on the Play time tile (it has a small chevron) opens it into a wide
    card with a chip for each of the last 7 days ("Today", "Thu 19", ...). The chosen day shows
    its total in the heading and every game of that day, longest first, with its time and a bar
    against the longest game (the game being played now is in the accent colour and counts the
    seconds not yet written to the log). A day with more than 6 games shows 5 and "+N more"; a
    day without play says "No games played". The card is as tall as the chosen day needs, and
    the Home tab scrolls as usual. The chevron at the top right folds it again. A popped-out
    Play time widget on the desktop stays compact.
- **System tab (after M7, always there, after Controls):** one card for each of CPU, GPU, RAM, the
  busiest disk, the connection in use and, only on a PC that has a battery, the battery, in two
  rows (3 + 3, or 3 + 2 without a battery). **The open card sizes itself to the cards:** as wide
  and as tall as they need (three columns of 176 px cards, 592 px with the margins; Pillbox's
  window is that wide), at least the normal card; with four cards it is the normal width, and
  when the samples show a different number of cards it re-sizes. On the left and right edges the
  width stays normal and the cards get narrower. Each card has a title, the figure, a second
  line and a bar (the disk has read and write lines; the network card has no bar). It was a Home
  tile first; a `system` in an old `widgets.shown` is ignored. The figures refresh twice a second
  (`widgets.system_refresh_ms`, 500; 250 to 5000, Settings > Widgets > System window > "Refresh") while
  the tab or the System window is open; the Home stats strip stays at every 2 s. Closed, or on
  another tab, nothing is read. The GPU reader samples twice a second on its own thread, so a
  period under 500 ms repeats GPU figures.
- **The card count is fixed from the first frame:** CPU, RAM, disk and network always; GPU if
  DXGI lists a card to show (asked when the tab or window opens, a few ms, not taken from the GPU
  reader's first sample, which comes up to half a second later); battery on a laptop. Figures
  that are not read yet show "…". (It used to show 6 cards, then 5 until the GPU sample came,
  then 6 again.) When the tab closes the GPU, disk and network figures are cleared, so the next
  visit does not start with old numbers.
- **Tab icons stay put:** every tab's icon is always on the same pixels, the ones it has on the Home
  tab (Home's slot is as wide as its highlighted name). They are placed from the left edge of the
  normal open card, not of the card that is open, so the wider System card does not move them.
  On another tab only that icon is highlighted; no tab name is shown except Home's.
- **System window (`widgets.system_window`, off by default):** the same cards in a desktop window
  like a popped-out widget, for people who want them always on screen. **Resize:** drag its right
  edge: the cards share the width (at least 100 px each, at most the screen; the height is
  fixed). **Move:** drag anywhere else. **Close:** its small X or a right-click; open it from the
  tray menu ("System window", checked while open) or Settings > Widgets. Position and width are
  kept in `data\system_window.json`; first time it opens just below Pillbox.
  - *GPU:* the dedicated card if there is one, else the integrated GPU. The card names it as
    people know it ("GeForce RTX 5060 Laptop GPU": the (R) and (TM) marks and the NVIDIA / AMD
    prefix dropped, so "Intel(R) UHD Graphics" is "Intel UHD Graphics"), in a smaller size when
    it is long. A "dedicated" adapter is a real (non-software) DXGI adapter with at least
    512 MB of its own video memory; the biggest wins. This is a heuristic: an AMD APU with a
    large memory carve-out would count as dedicated. What it picked is logged ("dedicated GPU:
    ..." or "integrated GPU: ..."). Load is that adapter's 3D engine load from the same "GPU
    Engine" counters as the strip; a card that is powered down on a hybrid laptop has no engine
    instances and reads 0%. The GPU monitor thread runs only while the card is shown.
  - *Disk:* activity, not fullness: the physical disk with the highest "Active time" (100 minus
    the `PhysicalDisk` `% Idle Time` counter, as Task Manager does). The card shows that
    percentage, the **read** speed and the **write** speed (`Disk Read Bytes/sec`, `Disk Write
    Bytes/sec`) on two lines; the first disk if all are idle. Disks with several partitions show
    all their letters ("D: E:"). The counters are rates, so the card shows "…" for the first two
    seconds. They are read only while the tab or window is shown.
  - *Network:* among the physical adapters that are up, the one carrying traffic. Windows cannot
    say which of Wi-Fi and Ethernet is "the" connection when both have a default route with the
    same metric (they do on the dev PC), so the busiest adapter wins, and the label only switches
    when another is more than twice as busy and above 20 KB/s, so it does not flicker while both
    are idle. With no traffic the one Windows routes the internet through (`GetBestInterface`
    to 1.1.1.1, a routing-table lookup that sends nothing) is shown. VPN and virtual adapters
    are ignored, so traffic inside a VPN tunnel shows on the physical adapter. Units follow
    `widgets.network_mbps`.
  - *Battery:* percent and plugged in or on battery; the cell is not drawn on a PC without a
    battery (desktop).
  - Logic and tests: `pillbox-core/src/sysinfo.rs`; drawing: `widgets.rs` (`paint_system_window`);
    opening, saving and resizing: `app/sysview.rs`, `platform/widgetwin.rs`.
- **Notification mirror (M8, off by default):** turn on "Mirror Windows notifications" in
  Settings (`notifications.enabled`). Pillbox then reads the toast notifications Windows
  holds (the ones in the notification centre) and shows them two ways. This reads the text of
  every notification on the PC (it stays on the PC); that is why it is off until you ask.
  - *List:* a **Notifications** tab, newest first: the app's icon, app name, time, title and text.
  - *Stack on the closed pill:* when notifications arrive, the pill shows up to three
    overlapping app icons, the newest one's title and "+N" for the rest, for 6 seconds
    (`notifications.stack_seconds`). Click it to open the list. Like a flash, it waits for a click instead
    of opening the card when the pointer is on it. Notifications that were already there when
    Pillbox started are listed but never stacked. Not shown (and not read) while a game is
    in front; the ones that arrived meanwhile stack when it ends.
  - *Icons:* each app's own logo on a light disc. Windows gives packaged apps (Claude, Phone Link,
    Settings) a logo through the listener; classic desktop programs get none there, so the icon
    the Start menu shows for the app (`shell:AppsFolder\<app id>`) is used. An app with neither
    keeps a coloured disc with its first letter. Fetched once per app per run, by the thumbnail
    worker; kept in memory so a rebuilt GPU device can upload them again.
  - *Duration:* `notifications.stack_seconds` (default 6, 1..60) is how long a new
    notification stays stacked.
  - *Wheel:* turn the wheel over the stacked pill to flip through the stacked notifications
    (down = next, up = previous); the one in front is shown with its place ("2/3", or "+2" for
    the newest). The stack is kept as it was when you started flipping and stays for the full
    duration again after each turn; a new arrival puts itself in front.
  - *Per-app rules:* `notifications.rules`, each `{ "app": "<name or app id>", "mode": "hide" |
    "list_only" }`, matched without regard to case against the name shown on the notification
    or its app id. `hide`: never listed or stacked (as if the app posted nothing). `list_only`:
    on the Notifications tab, never on the pill. Edited in Settings > Notifications; they apply
    at once, also to what is already on the tab.
  - *Open the app:* click a notification on the Notifications tab to start the app that posted
    it, as the Start menu would (`shell:AppsFolder\<app id>`). Windows cannot jump to the
    notification's own page, so this only brings the app up. Sources with no Start-menu entry
    (system components, programs that name themselves) show "Could not open <app>". Opening
    Windows PowerShell's test notification opens a terminal window.
  - *How it reads:* `UserNotificationListener` works for this unpackaged exe on this PC without
    a package identity or a consent prompt (see "M8 spike" below), but live change events do not,
    so a worker polls: every 3 seconds, and every second for 12 seconds after a change. A new
    notification shows on the pill up to 3 seconds after Windows shows it.
- **Chat drawer (M5b, off by default):** a panel below Pillbox showing a Twitch / YouTube
  live chat, in WebView2. Open and close it with Ctrl+Alt+Shift+T or the button on the Controls
  tab. It appears without taking focus; click into the page to type. The header drags the
  drawer; its ✕ closes it. It stays open over fullscreen games (setting). Logins and cookies are
  kept in `%LOCALAPPDATA%\Pillbox\WebView2`. No page loads and no browser process runs until the
  drawer is opened; closing it ends its browser processes. (A general web-page drawer existed in
  M5b and was removed afterwards at the user's request; old `web_*` keys in a settings file are
  ignored with a warning.)
- **Send to phone (M6):** hover a file on the Shelf tab and press the small phone button in its
  corner. A QR code replaces the shelf; scan it with the phone's camera (same network, no app or
  account). The page shows the file's name and size, a preview for pictures, and a Download
  button. The panel says "Phone connected", then "Sent to your phone ✓" when the last byte went out.
- **Phone to PC (M6):** the phone button at the top of the Shelf tab ("Receive from phone" when
  the shelf is empty) shows a code for a page where the phone picks photos or files and/or types
  text. Files are saved in `Downloads\Pillbox` (setting) and put on the shelf; text is copied to the
  clipboard (retried for up to 5 s if another app holds it). A size check on the page refuses an
  upload over the limit before anything is sent.
- **The link lives only while the code is on screen.** The card stays open while a code shows
  (moving the pointer away does not close it). The code and its server go away on Done, after
  `phone.code_minutes` (3 by default; then the card closes too if the pointer is elsewhere), when
  you switch tabs, hide Pillbox, a game takes over, or Pillbox shortcut closes the card.
  From then on the old link gets "connection refused"; a transfer already running may finish.
  Each code has a new port and token.
- **Firewall warning (M6):** when a code opens, Pillbox reads (never changes) Windows
  Firewall's settings on a worker thread. If the phone's connection would be dropped, the panel
  says so in red within half a second instead of "Waiting…". That covers no rule for pillbox.exe
  (the usual first-time case), a block rule, "block all incoming connections", or a Public
  network. The warning goes away as soon as a phone actually gets through.

**Scripts and agents (M7)**

- **Flash:** `pillboxctl flash "Build finished" --source build [--ms 3000] [--urgent]` puts a
  short line on the closed pill for `script.flash_seconds` (5). Up to 3 wait behind the one
  showing; a new flash with the same `--source` replaces its predecessor. It never takes focus
  and never opens the card: while a flash shows, resting the pointer on the pill waits for a
  click instead of opening the card (it opens when the flash goes, if the pointer is still
  there). **Clicking a "Claude is done / needs you" flash brings the Claude desktop app to the
  front**; any other flash opens the Activity tab. (The app is started through the shell by its
  AppUserModelId `Claude_pzs8sxrjxfjjc!Claude`, hard-coded for this PC's install; a second start
  only activates the running window.) The pill is narrow: about
  20 characters fit, the rest is cut with "…". During a game, a fullscreen app or a hide/strip
  rule, flashes are dropped unless sent with `--urgent`.
- **Status:** `pillboxctl status build --title "cargo build" --detail "compiling" --state running
  --progress 40` makes a live entry on the **Activity** tab (a new tab, on by default). The same
  id updates it in place and fields you leave out keep their value, so `status build --state ok`
  turns it green. States: `running`, `ok`, `failed`, `info`. `--remove` takes it away. Finished
  statuses leave after `script.keep_minutes` (60); a running one that was never closed after 6 h.
- **Agents:** the Activity tab lists every **session** of every **agent**, grouped as *needs you*,
  *working*, *done*. A card shows the project folder, the agent, its **model**, its **thinking
  effort**, what it is doing right now ("Running cargo test", "Editing phone.rs"), or how its
  last answer ended; click a card for the timeline of the last steps.
- **The Claude card** sits on top of the Activity tab (unless `claude` is in
  `script.hidden_agents`): the Claude app's own icon, "Claude Code", a status chip and a line:
  - **Working** (accent): what the newest working session does and where ("Editing tools.rs ·
    Dynamic Island", "(+1 more)" when several work).
  - **Needs you** (amber card): what it waits for.
  - **Idle** (green): Claude runs but no session is working; "Finished 3 min ago · Dynamic
    Pillbox" after a turn, else "Ready, nothing running".
  - **Offline** (faded, no limits): no `claude.exe` runs, so the desktop app is closed (also not
    in the tray) and no Claude Code runs in a terminal. Its sessions are taken off then, since
    Claude sends no session end when it is quit.
  - While it runs, a bar for the **5-hour** and the **weekly** limit: share used (amber from 80%,
    red from 95%) and when each resets; numbers older than 10 minutes say "as of …". Without
    limits: "Usage limits show while Claude Code runs in a terminal" (see Usage limits below).
  - A click on the card opens the Claude app. The sessions follow under "Sessions".
  - Whether Claude runs is one process-list snapshot (about 1 ms) when the tab opens and every
    3 s while it is open; nothing runs while the tab is closed. Logic and tests:
    `Board::kind_summary` in `pillbox-core/src/script.rs`.
- **Flash when it matters:** when a session goes from working to done, or needs you, the pill
  flashes "Claude is done" / "Codex needs you" (`script.agent_flashes`).
- **Buttons on a card** (shown when the pointer is on it): **Open Claude** (Claude Code cards:
  the same jump to the desktop app), **New chat** (Claude Code and Codex:
  opens Windows Terminal, or a console, in the same folder running `claude` / `codex`), **Stop**
  (ends the agent's process and what it started; asks to be clicked twice; see below) and ✕
  (takes the card off).
- **Allow / Deny** (`script.answer_permissions`, off by default): when Claude Code asks for a
  permission, the card turns amber with Allow and Deny buttons. Needs the reply hook
  (`pillboxctl hooks claude --reply`). If nobody clicks within `script.answer_seconds` (60), or
  Pillbox is not running, Claude Code asks in its own window as usual.
- **Not possible:** typing a free-text reply into a running Claude Code or Codex chat. Neither
  has a hook or command that injects text into a session that is already running, so Pillbox
  offers Allow / Deny, New chat and Stop instead.
- **Hooks:** `pillboxctl hooks claude` prints the JSON to add to `~\.claude\settings.json`;
  `pillboxctl hooks codex` the same for `~\.codex\hooks.json`. `--apply` adds it for you: it makes
  a backup (`settings.json.pillbox-backup-<time>`), adds only Pillbox's entries next to your
  own hooks (existing hooks and key order stay), and does nothing if they are already there. The
  Pillbox never edits those files by itself.
- **Usage limits** come only from Claude Code's *status line* (nothing else documented exposes
  them; reading the sign-in token to ask Anthropic's servers was ruled out). Connect it with
  `pillboxctl statusline claude --apply` (in PowerShell a quoted path needs `&` in front:
  `& "<path>\pillboxctl.exe" statusline claude --apply`): it makes a backup of `~\.claude\settings.json`, sets
  the `statusLine` command to `"<path>\pillboxctl.exe" statusline claude` (keeping its
  `refreshInterval` and other fields), and keeps the old command in
  `%APPDATA%\Pillbox\statusline-chain.json`. On each refresh pillboxctl sends the JSON to the
  Pillbox, runs the old command through `cmd /D /S /C` with the same input (no console window,
  at most 3 s) and prints what it printed, so an existing status line (on this PC the `node`
  script `claude-status-bridge.js`) keeps working and keeps writing its own files. With no old
  command, or if it prints nothing, pillboxctl prints "Claude 5h 42%". Running it again does
  nothing. To undo, put the backup back or delete the `statusLine` entry.
  **Checked 2026-11-19: the Claude desktop app does not run the status line** (with pillboxctl
  set as the status line, no `pillboxctl statusline` ran in 8 s while a desktop chat worked, and
  the bridge's `~\.ai-usage-bridge\claude.json` was never written), and it keeps no copy of the
  limits in its local data (searched for `five_hour` / `rate_limits`: none). Its hooks do run
  (`pillboxctl hook` ran on each tool call), so status works from desktop chats, but limits
  arrive only from Claude Code in a terminal. Without them the model and effort still show (they come from
  the transcript).
- **Usage from the Claude account (`script.claude_account_usage`, off by default; Settings >
  Agents, "Claude usage limits").** While the Activity tab is open and Claude
  runs, Pillbox asks `https://api.anthropic.com/api/oauth/usage` at most once a minute (every
  5 minutes after a failure), with the access token from Claude Code's
  `~\.claude\.credentials.json` (`claudeAiOauth.accessToken`) and the header `anthropic-beta:
  oauth-2025-04-20`. The answer's `five_hour` / `seven_day` `utilization` (percent) and
  `resets_at` (ISO time) fill the same bars as the status line; whichever came last wins, limit
  by limit. A status line tick without `rate_limits` (Claude Code sends those before the first
  request of a session, every second) keeps the known numbers; before 2026-11-19 it wiped them,
  so the bars showed for a moment and then fell back to "Asking Anthropic…".
  Checked live 2026-11-19 after `claude` + `/login` in a terminal: "account usage received",
  64% 5-hour and 18% weekly, steady over two frames 6 s apart. This
  is **not a published API** (it is what Claude Code's `/usage` screen reads) and can change or
  stop without notice. The token is read fresh each time and never stored, logged, refreshed or
  written back (refreshing it here could sign Claude Code out). When there are no numbers, the
  card says why: no sign-in, "Sign-in expired: open Claude Code to renew it" (HTTP 401/403),
  "Could not reach Anthropic", and so on.
  **On this PC (2026-11-19) the file has no access token:** the desktop app keeps its own
  sign-in elsewhere and leaves `accessToken` empty (`expiresAt` 0), so the card says "No Claude
  Code sign-in here". Signing in Claude Code once in a terminal (`claude`, then `/login`) would
  fill it. Pillbox does not read the desktop app's own storage or the environment of its
  processes for a token. After the terminal sign-in the file has a token and the request
  works. Codex has no known source for limits: shown as "limits unknown".
- **Custom agents:** any name works: `pillboxctl agent builder --session a --state working --doing
  "Indexing" --step "started" [--model M] [--effort E] [--cwd DIR] [--pid N]`, states `working`,
  `needs-you`, `done`, `idle`, `--end` removes it. It appears on its first call.

## Settings

File: `%APPDATA%\Pillbox\settings.json`. Created with defaults on first run. Every key is
optional; a bad value keeps its default and is reported in the log, numbers are clamped.

**Retired keys** (`settings::RETIRED`) are dropped without a warning, so an older file loads
quietly: `layout.width`, `layout.height`, `layout.roundness`, `layout.open_width`,
`layout.open_height`, `layout.tall_height` (replaced by `layout.size`; the shapes are now the
old defaults, `shape::NOTICE_W` / `PILL_H` / `OPEN_W` / `OPEN_H` / `TALL_H`, always fully
round), `behavior.fps` (shape changes always animate at 60 fps, `MOTION_FPS`) and
`phone.max_upload_mb` (a fixed 4096 MB cap, `PHONE_MAX_UPLOAD_MB`). A file with a custom
`layout.height` and no `layout.size` gets the size that gives the same pill height, rounded to
5% (`settings::migrate`: 45 → 125%). The retired keys stay in the file until something saves it.

```jsonc
{
  "layout": {
    "style": "pill",            // "pill" | "notch"  (older files say "island": still read)
    "side": "top",              // "top" | "left" | "right"
    "monitor": "primary",       // "primary" or a display name such as "\\\\.\\DISPLAY2"
    "distance": 8,              // 0..300 DIPs from the screen edge (not scaled by size); ignored for a notch
    "size": 1.0,                // 0.8..1.4, scales the whole Pillbox (closed, open, text): drawn at DPI × size
    "hidden_until_hover": false,
    "reveal": {                 // what shows a hidden Pillbox; it hides again when the reason ends
      "music": "while_playing", // "never" | "while_playing" | "on_song_change"
      "song_seconds": 6,        // 2..30, for "on_song_change"
      "notifications": true, "flashes": true, "calls": true, "reminders": true, "notices": true
    }
  },
  "behavior": {
    "open_delay_ms": 120,       // 0..3000
    "close_delay_ms": 180,      // 0..3000
    "animation_speed": 1.0,     // 0.5..2
    "reopen_tab": "last",       // "last" | "music"
    "clock_24h": true,
    "pill_clock": "time",       // "time" | "day_time" ("Fri 14:05") | "date_time" ("20 Nov 14:05"); the time face only
    "visualizer": "pixel_bars"  // "off" | "pixel_bars" | "rounded_bars" | "equalizer" | "burst" | "wave" | "dots"
  },
  "music": {
    "enabled": true,
    "source_mode": "all",       // "all" | "only" | "except"
    "source_apps": [],          // parts of app names, e.g. ["spotify"]; used by only/except
    "title_in_pill": true,
    "pill_colors": true,        // the cover's colours flow along the resting pill
    "reactive_background": true, // the Music tab's background glows with the music and the lyric lines
    "art_background": true,
    "progress_style": "line",   // line | bar | dots | wave | segments
    "lyrics": false             // the lyrics column on the Music tab; off = lrclib.net is never contacted
  },
  "popups": {
    "lock_keys": true           // Caps / Num / Scroll Lock notices
  },
  "appearance": {
    "surface": "black",         // black | graphite | pure_black | acrylic | liquid_glass |
                                // candy_glass | rose | lavender | peach | sakura
    "accent": "#7C5CFF",
    "app_theme": "default",     // default | midnight | ocean | forest | sunset | violet | ember | mono
                                // fades: aurora | golden_hour | deep_sea | midnight_neon |
                                // forest_mist | lava | mono_fade | cotton_candy | custom
    "animate": true,            // let Aurora and Lava drift and Midnight neon shimmer
    "fade": {                   // the "custom" app theme
      "stops": [                // 2..3 colours, in order; more are dropped
        { "color": "#2B1C6B", "at": 0.0 },   // at: 0..1 along the fade
        { "color": "#7A2C7E", "at": 0.55 },
        { "color": "#B8455A", "at": 1.0 }
      ],
      "angle": 20.0,            // 0..360: 0 = left to right, 90 = top to bottom
      "softness": 1.0,          // 0 = hard edges .. 1 = fully blended
      "opacity": 1.0            // 0.3..1: how much of the surface shows through
    },
    "glass_blur": 0.5,          // 0..1
    "hide_from_capture": false,
    "resting_face": "time",     // none | time | date | weather | battery | cpu | next_event | pet
    "clock": {                  // the text on the closed pill (the clock and the other text faces)
      "size": 1.0,              // 0.8..1.3
      "bold": true,
      "seconds": false,         // "14:05:09"; the pill redraws once a second
      "leading_zero": false,    // "09:05"
      "color": "text",          // "text" (the surface's) | "accent" | "custom"
      "custom_color": "#FFFFFF" // used for "custom"
    }
  },
  "weather": {
    "enabled": false,           // off = no network use at all
    "place": "",                // the city search's name for the coordinates, shown in Settings only
    "latitude": 0.0,
    "longitude": 0.0,
    "fahrenheit": false
  },
  "hotkeys": {                  // "" = no shortcut; bad text keeps the default
    "toggle_pillbox": "Ctrl+Alt+N", "play_pause": "Ctrl+Alt+P", "mute_mic": "Ctrl+Alt+Shift+M",
    "hide": "Ctrl+Alt+H", "screenshot": "Ctrl+Alt+S", "color_picker": "Ctrl+Alt+C",
    "chat_drawer": "Ctrl+Alt+Shift+T"
  },
  "calls": { "call_timer": true, "sensor_dot": true, "ignore_apps": [] },
  "clipboard": {
    "enabled": true,            // off = the clipboard is never read
    "max_items": 200,           // 10..5000 unpinned items
    "expire_days": 0,           // 0 (never) | 1 | 7 | 30: unpinned items older than this are dropped
                                // (at start, on settings change, on each copy, when the tab opens)
    "pictures": true,
    "two_columns": false,
    "snippets": [ { "name": "Address", "text": "..." } ],
    "ignore_apps": []           // e.g. ["keepass.exe"]
  },
  "shelf": { "captures": true },
  "reminders": { "chime": true, "snooze_minutes": 10 },
  "launcher": {
    "slots": [ { "name": "Notepad", "path": "C:\\Windows\\notepad.exe", "args": "", "hotkey": "Ctrl+Alt+1" } ]
  },
  "widgets": {
    "shown": ["clock", "next_events", "pc_stats", "focus_timer", "day_progress", "quick_note"],
      // clock | day_clock | world_clock | weather | air_quality | sun_moon | next_events | todo |
      // quick_note | calculator | focus_timer | countdown | day_progress | year_progress |
      // pc_stats | system | battery | screen_time | play_time | ping | website_check | currency | photos
    "system_window": false,     // a desktop window with the System tab's cards (see System window)
    "system_refresh_ms": 500,   // how often the System tab / window read the PC (250..5000)
    "online": false,            // off = ping, currency, website check, air quality never fetch
    "world_clocks": ["UTC"],    // IANA names, e.g. "Asia/Tokyo"
    "weather_look": "minimal",  // "minimal" | "detailed" (a wide tile)
    "clock_color": "#FFFFFF",
    "network_mbps": true,
    "ping_host": "1.1.1.1",
    "websites": [],
    "currency_base": "EUR",
    "currencies": ["USD", "GBP"],
    "countdown_name": "", "countdown_date": "",   // YYYY-MM-DD
    "photos_folder": "",        // empty = Pictures
    "focus_minutes": 25
  },
  "games": {
    "enabled": true,           // react to fullscreen apps at all
    "behavior": "strip",       // "strip" | "hide"
    "every_fullscreen_app": false,  // true = browsers and video players count too
    "strip_background": "surface",  // "surface" | "numbers_only"
    "usage_bars": true,
    "ignore_apps": []          // e.g. ["kodi.exe"]; never treated as games
  },
  "drawers": {
    "chat_enabled": false,      // off = no chat page is ever loaded
    "chat_source": "twitch",    // "twitch" | "youtube"
    "chat_channel": "",         // Twitch channel name, or the YouTube live video id
    "width": 380,               // 260..900
    "height": 560,              // 200..1200
    "over_games": true          // an open drawer stays over a fullscreen game
  },
  "phone": {
    "enabled": true,            // off = no phone buttons, no server ever starts
    "receive_folder": "",       // "" = Downloads\Pillbox
    "received_to_shelf": true,  // files from the phone go on the shelf
    "code_minutes": 3           // 1..30; the QR code and its server close after this
  },
  "notifications": {            // M8: mirror Windows notifications (reads their text)
    "enabled": false,
    "stack_seconds": 6,         // 1..60; how long new ones stay on the pill
    "rules": []                 // e.g. [{ "app": "Claude", "mode": "list_only" }]  (mode: "hide" | "list_only")
  },
  "script": {                   // M7: pillboxctl, flashes, statuses, agents
    "enabled": true,            // off = no server, no Activity tab
    "flash_seconds": 5,         // 1..60; how long a flash stays on the pill
    "keep_minutes": 60,         // 1..1440; finished agents and statuses stay this long
    "stale_minutes": 20,        // 2..240; a silent "working" agent shows as idle after this
    "agent_flashes": true,      // flash "Claude is done" / "Claude needs you"
    "answer_permissions": false, // Allow / Deny buttons (needs hooks --reply)
    "answer_seconds": 60,       // 5..600; a question waits this long for a click
    "hidden_agents": [],        // lower-case agent names to leave off the tab, e.g. ["codex"]
    "claude_account_usage": false // ask Anthropic for the account's limits with Claude Code's sign-in
  },
  "app_rules": [
    { "exe": "game.exe", "behavior": "hide" }   // "hide" | "strip" | "show"; beats "games"
  ]
}
```

Data files (not settings) live in `%APPDATA%\Pillbox\data\`: `clipboard\` (DPAPI-encrypted
history and pictures), `shelf.json`, `reminders.json`, `todos.json`, `note.txt`,
`screentime.json`, `widgets.json` (popped-out widget positions), `playtime.json`. A damaged JSON file is kept as
`<name>.bad` and the app starts empty.

(The real file is plain JSON; comments are only for this README.)

## Network use

Nothing by default. Two opt-in features use the network:

- **Weather** (enabled *and* shown as the resting face or a widget): asks `api.open-meteo.com`
  for current conditions and today's high/low at the configured coordinates, at most every 25
  minutes. No key, no account.
- **Lyrics** (`music.lyrics`): for each new song, sends its title, artist, album and length to
  `lrclib.net`, once. The answer (or "none found") is cached in `%APPDATA%\Pillbox\lyrics`, so a
  song heard before never causes another request.
- **Online widgets** (`widgets.online`, off by default, and only the widgets you show), at most
  every 10 minutes and when the Home tab opens (not more than once a minute): ICMP ping to
  `ping_host`; `api.frankfurter.app` for currency rates; `air-quality-api.open-meteo.com` with your
  weather coordinates; a GET request to each website you list.
- The weather widget uses the same Open-Meteo request as the weather face.
- **City search** (Settings > Widgets > Weather > Location): only when Search is pressed, the
  Settings window sends the typed name (nothing else) to `geocoding-api.open-meteo.com` and
  shows up to six places. Picking one stores its coordinates; nothing is sent again.
- **Chat drawer** (`drawers.chat_enabled`, off by default): only while the drawer is open,
  WebView2 loads the `twitch.tv` / `youtube.com` chat page like a browser would, including
  whatever that site loads.
- **Phone transfer** (M6): nothing goes to the internet. While a QR code is on screen, a small
  HTTP server listens on **one** local-network address of this PC (never on all interfaces).
  How it is locked down:
  - Address: a private (RFC 1918) IPv4 address of an adapter with a gateway. Networks Windows
    files as Private come first, then Wi-Fi before Ethernet. VPN, public and host-only virtual
    adapters are never used.
  - Port: random in 49152–65535 (not the OS's next free one).
  - Token: 128 random bits from `BCryptGenRandom` as the first part of the path, compared in
    constant time.
  - Requests are answered only from the same subnet, and only with a Host header naming exactly
    that address and port (stops DNS rebinding). Everything else gets the same plain 404.
  - 20 token-shaped wrong guesses close the code.
  - Headers: `no-store`, `no-referrer`, `nosniff`, `DENY` framing, and a CSP that allows only
    the page's own nonce-tagged script, its own images and its own form target.
  - Uploads: file names are cut to their last part, with Windows-forbidden characters and
    reserved names (CON, COM1…) replaced. Files are streamed to `<name>.part` and renamed to a
    free name, and the part is deleted on any error. The size limit is enforced while
    streaming, and a declared oversized body is refused at once (413).
  - The token is never logged.

  It is plain HTTP: someone on the same Wi-Fi who can sniff traffic could read the link and the
  file while the code is open. On a normal WPA2/WPA3 home network other devices cannot, but on a
  shared or open network they might. HTTPS would need a certificate the phone does not trust,
  which means a warning page on every scan.
- **Script API** (M7, `script.enabled`, on by default): nothing goes to the internet and nothing
  is reachable from the network. A small HTTP server listens on **`127.0.0.1` only** (the
  loopback address, never the LAN) from the moment Pillbox starts, on a random port in
  49152–65535. It writes `{port, token, pid}` to `%APPDATA%\Pillbox\data\api.json` (in `data`, so
  the settings watcher is not woken) and deletes the file when Pillbox quits. `pillboxctl`
  and the agent hooks read it. A request is answered only if
  - the Host header is exactly `127.0.0.1:<port>` (stops DNS rebinding),
  - it has **no `Origin` header** (a web page in a browser always sends one on a cross-site
    request, so a page you visit cannot make your browser talk to the API), and
  - it carries the 128-bit random token (from `BCryptGenRandom`) as `Authorization: Bearer`,
    compared in constant time.

  Everything else gets the same plain 404. Bodies over 256 KB get 413. The token is never logged
  and changes at every start. **Who can read it:** `api.json` lives under your profile, so other
  Windows accounts cannot; but any program running as *you* can read it and push flashes, statuses
  and agent events, and can answer a permission question Pillbox is holding. That is the same
  trust as any local secret in `%APPDATA%`. With "Allow / Deny" on, that includes a malicious
  program of yours *approving* a tool use that Claude Code asked for: leave it off if you do not
  want that. (A program running as you could also just edit Claude's settings.)
  The server only reads one more file: the transcript of a Claude Code session, and only from
  inside `~\.claude\projects`, and only its last 192 KB, at the start and end of a turn, to find
  the model and effort.

## Measured cost (after M3, this laptop, 100% scaling, Black surface)

| State | CPU (of one core) | Memory (Task Manager figure) | Working set |
|---|---|---|---|
| Idle, nothing playing | 0% | 37 MB | 70 MB |
| Music playing, pill closed (visualizer running) | about 2% | 37 MB | 70 MB |
| After M4, idle (10 s sample, default widgets) | 0% | 44 MB private | 68 MB |
| After M5a, idle (30 s, measured in CPU cycles) | 0.03% (0.003% of the machine) | 43 MB private | 68 MB |
| Stats strip showing over a game | about 0.8% (was 1.0% before the fixes round) | | |
| Camera mirror open (1280x720 camera, shown at 640x360) | about 1.2% | | |
| After M5b, idle (30 s, CPU cycles) | 0.02% (0.002% of the machine) | 43 MB private | 60 MB |
| Stats strip with GPU readings, quiet desktop | about 0.5% | | |
| A drawer open (measured with the since-removed web drawer on example.com) | | Pillbox 46 MB private | WebView2: 6 processes, ~340 MB working set (shared pages counted twice) |
| After M6, idle, no code shown yet | | 43 MB private | 60 MB, 9 threads |
| M6: QR code on screen, server waiting (10 s) | 0.01% (0.001% of the machine) | 50 MB private | 75 MB, 12 threads |
| M6: after the code closed (also after 6 open/close rounds) | 0.03% (20 s) | 50 MB private | 75 MB, 10–11 threads |
| After the play-time, icon and Controls work: idle, Pillbox closed (20 s) | 0.02% of one core (0.002% of the machine) | | |
| Controls tab open, no list (12 s) | 0.02% of one core | | |
| Controls tab with the Bluetooth list open (16 s; it looks again every 4 s, about 20 ms of CPU each time) | 0.56% of one core (0.056% of the machine) | 45 MB private | 17 threads |

The first code costs about 7 MB once (Winsock, the Network List Manager COM component, the HTTP
stack); it does not grow with more codes. The server's own thread and runtime end when the code
closes.

From M5 on, CPU is measured from CPU cycles (`QueryProcessCycleTime`); the older 0% figures came
from Windows' 15.6 ms tick-sampled process times, which can miss short bursts.

While music plays Pillbox redraws 30 times a second and listens to the audio output; that
stops the moment playback pauses. The 40 MB target is met by Task Manager's measure but not by
total working set.

**After M7 (script server running, nothing playing, card closed):** 46.6 MB private memory and
13 threads, 0.001% of the machine over 10 s (0.01% of one core). M6 idle was 43 MB and 9 threads,
so the always-on script server costs about 3–4 MB and 4 threads. Each flash, status or hook call
wakes Pillbox once; a status line that ticks every second (Claude Code's `refreshInterval`)
is dropped without a redraw unless a visible number changed.

**After M8 (notification mirror on, nothing arriving):** 0.037% of the machine over 30 s in
steady state (0.37% of one core), against 0.001% with the mirror off: one read every 3 s at about
12 ms of CPU each. A change makes it read every second for 12 s, so a busy notification centre
costs more for a while. 47 MB private memory, 13 threads (11 threads with the mirror off). Off
by default for this reason as well as for privacy.

**System tab / window:** 0.022% of the machine over 6 s while open (the readers
for GPU, disk and network run only then; closed they cost nothing). Refreshing twice a second
instead of every 2 s (2026-11-19, with music playing, so the base is higher): 0.31% of the machine
at 2 s against 0.37% at 0.5 s with the System tab open, two runs each over 10 s. So the faster
refresh adds about 0.06%, and only while the tab or window is on screen.

## Hotkeys

Global (work in every app; rebind or clear them in Settings → Shortcuts):

| Default | Action |
|---|---|
| Ctrl+Alt+N | Open or close the card (stays open until pressed again or the pointer passes over and leaves) |
| Ctrl+Alt+P | Play / pause |
| Ctrl+Alt+Shift+M | Mute / unmute every microphone (Windows endpoint level) |
| Ctrl+Alt+H | Hide / show Pillbox |
| Ctrl+Alt+S | Screenshot of the monitor under the pointer |
| Ctrl+Alt+C | Copy the colour under the pointer as `#RRGGBB` |
| Ctrl+Alt+Shift+T | Open / close the chat drawer |
| (per slot) | Launcher slots, if you give them a shortcut |

Accepted text: `Ctrl`, `Alt`, `Shift`, `Win` plus one key: letters, digits, `F1`..`F24`, `Space`,
`Enter`, `Tab`, `Esc`, `Backspace`, `Delete`, `Insert`, `Home`, `End`, `PageUp`, `PageDown`, arrows,
`PrintScreen`, `Pause`, `Num0`..`Num9`, punctuation (`;` `Plus` `,` `-` `.` `/` `` ` `` `[` `\` `]`
`'`) and media keys. Typing keys need Ctrl, Alt or Win (Shift alone is refused).

The microphone default used to be Ctrl+Alt+M, but another program on this PC owns it, so the
default is now Ctrl+Alt+Shift+M.

## One-time permissions

None for M1–M5 and M7. (M7: nothing to grant; to connect an agent you add its hooks yourself,
see "Scripts and agents": `pillboxctl hooks claude --apply`.) The chat drawer needs the Microsoft Edge WebView2 Runtime, which Windows 11 has
built in (the camera mirror needs "Let desktop apps access your camera" on, which is
Windows' default). "Start with Windows" writes
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Pillbox` (no admin). Wi-Fi/Bluetooth
switches, brightness, microphone mute and clipboard reading all work for a normal user.

**Phone transfer and the Windows firewall (M6).** Windows Defender Firewall blocks incoming
connections unless the app is allowed. The first time a QR code opens, Windows may ask whether
`pillbox.exe` may communicate. Tick **Private networks** only. If no prompt appears, or the phone
can't load the page, allow it once from an admin terminal (your decision; Pillbox never
changes firewall settings itself):

```
netsh advfirewall firewall add rule name="Pillbox phone transfer" dir=in action=allow program="<full path of pillbox.exe>" protocol=TCP profile=private
```

The network must be set to Private (Settings → Network & internet → your network → Network
profile type). On this PC the Wi-Fi is set to Public and the Ethernet to Private, both on the
same router, so Pillbox uses the Ethernet address. A phone on that Wi-Fi is on the same
subnet and should reach it, but no real phone has been tested yet. If only a Public network is
available, the panel says so.

## Developer aids (environment variables)

| Variable | Effect |
|---|---|
| `PILLBOX_SCALE` | Pretend the monitor is at this DPI scale, e.g. `1.5` |
| `PILLBOX_DEBUG` | Any value: debug logging (clicks, measured animation fps) |
| `PILLBOX_DATA_DIR` | Use this folder instead of `%APPDATA%\Pillbox` (tests use it) |
| `PILLBOX_DUMP` | Path of a `.bmp` to write when the dump command is sent (see below) |
| `PILLBOX_NO_FOCUS` | Any value: Pillbox never changes the foreground window (text boxes do not take the keyboard). For automated tests that post typed characters while you use another app |
| `PILLBOX_FAKE_TRACK` | `Title\|Artist\|Album\|seconds`: a silent pretend track instead of real media sessions; the visualizer gets pretend moving levels |
| `PILLBOX_FAKE_LYRICS` | Path of an `.lrc` file: the pretend track (`PILLBOX_FAKE_TRACK`) shows these lyrics instead of asking the network |
| `PILLBOX_FAKE_CLAUDE` | `0` or `1`: pretend Claude is closed or running (the Claude card's Offline state without quitting the app) |
| `PILLBOX_SETTINGS_PAGE` | Settings window: open on this page (`general`, `music`, `games`, ...: a page title or its first word). With any `PILLBOX_SETTINGS_*` aid set, the window opens without taking the focus |
| `PILLBOX_SETTINGS_QUERY` | Settings window: start with this text in the search box |
| `PILLBOX_SETTINGS_SCROLL` | Settings window: start the page scrolled down this many points |
| `PILLBOX_SETTINGS_SHOT` | Settings window: save a PNG of the window here (egui's own screenshot, so it works behind other windows), then close. `PILLBOX_SETTINGS_SHOT_DELAY` (ms) waits longer first |

The Settings window cannot be clicked by posting mouse messages to it: winit asks Windows for a
leave notice, and with the real pointer elsewhere it arrives in the same egui frame as the click
and clears the pointer before the hit test. Its clicks are tested headless instead
(`settings_ui::tests`: real pages, synthetic pointer events, no file written).

The hidden tray window (class `Pillbox.Tray`) accepts `WM_COMMAND` with these ids, which is how
the automated checks drive the app: 100 Settings, 101 Hide/Show, 102 Start with Windows,
103 Quit, 200 dump the next frame to `PILLBOX_DUMP`, 201 force open, 202 force close,
210..219 show the Home / Music / (Music again: the lyrics are on it) / Clipboard / Shelf / Reminders / Controls / Activity / Notifications / System tab,
230 a click inside Pillbox at window pixel (x, y) packed into lParam as `x | y << 16` (it goes
straight into Pillbox's event queue: the real pointer, focus and other apps are untouched),
231 the same as a right-click, 232 / 233 one notch of the wheel down / up at window pixel
`x | y << 16` (on the closed pill it flips the notification stack; on the tab row, y about 21,
it switches tabs; lParam 0 is the top-left corner).
Frame dumps exist because glass surfaces are invisible to screenshots.

## Layout of the code

| Path | What |
|---|---|
| `crates/pillbox-core` | Platform-free logic with unit tests: spring, fixed timestep, geometry and hit-testing, placement, settings parsing, themes, face text, weather parsing, the glass displacement map, the LRC lyrics parser, LRCLIB requests, the audio spectrum (FFT), cover-art colours |
| `crates/Pillbox/src/platform` | Every `unsafe` block: window, Direct2D/DirectComposition renderer and screen capture, tray, monitors, timers, foreground-app hook, media sessions (`media.rs`), audio loopback (`audio.rs`) |
| `crates/Pillbox/src/app.rs` | State machine and main loop (safe Rust) |
| `crates/Pillbox/src/app/tools.rs` | M4 tools on top of it: clipboard, shelf, reminders, controls, text boxes, widgets, pop-out windows |
| `crates/Pillbox/src/tabs.rs`, `widgets.rs` | Drawing of the list tabs and widget tiles |
| `crates/pillbox-core` (M4 additions) | Hotkey parsing, calendar maths, reminder parser and repeats, clipboard history model, text-box editing, calculator, sun/moon, PNG helpers |
| `crates/Pillbox/src/lyrics.rs` | Lyrics lookup and cache |
| `crates/Pillbox/src/app/games.rs` | M5: mode decision (rules, fullscreen), stats strip sampling |
| `crates/Pillbox/src/app/mirror.rs` | Camera mirror state: when the camera may run |
| `crates/Pillbox/src/platform/fullscreen.rs`, `camera.rs` | Fullscreen detection; Media Foundation webcam reader |
| `crates/pillbox-core/src/games.rs` | Which fullscreen apps count as games, smoothing, speed formatting, GPU counter names (tested) |
| `crates/pillbox-core/src/playtime.rs` | Play-time log, week totals, the last days with their games (`recent_days`), day labels, game names from executables (tested) |
| `crates/pillbox-core/src/appicon.rs`, `crates/Pillbox/build.rs` | The app icon drawn in code at any size, packed as `.ico` / resource bytes; `build.rs` writes them as a `.res` file for the linker (tested) |
| `crates/pillbox-core/src/links.rs` | Wi-Fi and Bluetooth lists for the Controls cards: dedupe and order, signal bars, device kinds, one line per address, card summaries (tested) |
| `crates/Pillbox/src/platform/links.rs` | Worker thread for those lists: Native Wifi (`wlanapi`) to list, join and leave; `Windows.Devices.Bluetooth` to list, the driver's disconnect request and the service switch to connect |
| `crates/Pillbox/src/tabs/controls.rs` | The Controls tab drawing: sliders, the Wi-Fi/Bluetooth cards and lists, tiles, call line, launcher |
| `crates/Pillbox/src/platform/gpu.rs` | GPU load (PDH) and temperature (D3DKMT) worker |
| `crates/pillbox-core/src/notes.rs` | M8: the notification list, what just arrived (the pill stack), text cleaning, badge letters and colours (tested) |
| `crates/Pillbox/src/platform/notifications.rs` | M8: `UserNotificationListener` polled on a worker thread |
| `crates/Pillbox/src/app/notes.rs`, `tabs/notifications.rs` | M8: applying readings, the stack on the pill, the Notifications tab |
| `crates/pillbox-core/src/sysinfo.rs` | System card choices: busiest disk, GPU to show, connection in use (tested) |
| `crates/Pillbox/src/platform/diskio.rs` | Disk activity (PDH `PhysicalDisk` counters) for the System card |
| `spike-identity/` | M8 spike (throwaway): can an exe read notifications, with and without package identity |
| `crates/Pillbox/src/platform/drawer.rs`, `app/drawers.rs` | Drawer window with WebView2; when drawers open and close |
| `crates/Pillbox/vendor/WebView2Loader.dll` | Microsoft's WebView2 loader (x64, signed), embedded in the exe |
| `crates/pillbox-core/src/phone.rs` | M6 logic (tested): picking the address, subnet/Host/token checks, safe file names, QR matrix, the phone pages |
| `crates/Pillbox/src/phone.rs` | M6 phone server (axum on a one-thread tokio runtime, safe Rust) |
| `crates/Pillbox/src/app/phone.rs` | When the QR code shows and closes; what Pillbox does with what arrives |
| `crates/Pillbox/src/platform/firewall.rs` | Read-only Windows Firewall check (`INetFwPolicy2`), decided by `pillbox_core::phone::firewall_verdict` (tested) |
| `crates/Pillbox/src/platform/net.rs` | Adapter addresses (`GetAdaptersAddresses`), network categories (Network List Manager), `BCryptGenRandom`, Downloads folder |
| `crates/pillbox-core/examples/qrdecode.rs` | Test aid: decodes the QR code in a frame dump |
| `crates/pillbox-core/src/script.rs` | M7 logic (tested): request parsing, flash queue, statuses, the agent state machine, hook-event mapping for Claude Code and Codex, transcript model/effort, status-line limits, the permission reply JSON |
| `crates/Pillbox/src/script.rs` | M7 script server (axum on a one-thread tokio runtime, loopback only) and the transcript tail reader |
| `crates/Pillbox/src/app/script.rs` | Applies what the server received: flashes, the Activity board, Allow/Deny, Stop, New chat |
| `crates/Pillbox/src/tabs/activity.rs` | The Activity tab drawing |
| `crates/pillboxctl` | The command line (`flash`, `status`, `agent`, `hook`, `statusline`, `hooks`, `ping`); `statusline.rs` is status line mode and `--apply`; its only `unsafe` is the process list in `platform.rs` |
| `crates/Pillbox/src/scene.rs` | What gets drawn (safe Rust) |
| `crates/Pillbox/src/settings_ui/` | Settings window (egui), run as `pillbox.exe --settings` in its own process: `mod.rs` (the window, sidebar, search, autosave, file sync, dev aids, headless tests), `kit.rs` (colours, fonts, icons and the controls: section cards and rows, switch, segmented buttons, slider, picture cards, lists, shortcut recorder), `pages.rs` (the ten pages and the picture-card previews) |
| `crates/Pillbox/src/platform/settings_window.rs` | Rounded corners and a dark frame for the frameless Settings window (DWM) |
| `spike/` | Throwaway prototype from before M1. Safe to delete |

## How it is built

- Window: `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP`, drawn
  with Direct2D and DirectWrite on Direct3D 11, presented through DirectComposition (real
  per-pixel alpha). The integrated GPU is preferred so the discrete one can stay asleep.
- Click-through: the window region (`SetWindowRgn`) follows the animated shape, padded by 1 px.
- Hover: ordinary `WM_MOUSEMOVE` / `WM_MOUSELEAVE` plus open and close delays.
- Animation: springs stepped at a fixed 1/120 s, frames paced by a high-resolution waitable timer.
- Idle: the main loop sleeps in `MsgWaitForMultipleObjects` on the message queue and a
  change-notification handle for the settings folder. No threads, no polling.
- Glass: Windows.Graphics.Capture of Pillbox's monitor, with Pillbox excluded via
  `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`. The strip behind the window is copied,
  blurred (Gaussian blur effect) and refracted (displacement-map effect), at most 30 times a second.
- Settings window: egui via eframe (OpenGL), in a separate process started on demand, so its
  memory and any crash stay out of Pillbox. The two share only the settings file. No new
  crates for the redesign: the controls are drawn with egui's painter, fonts and icons come
  from the Windows fonts folder, the window is frameless (`with_decorations(false)`) with its
  own drag area (the sidebar top and the page heading) and minimise / close buttons. While open
  it repaints once a second (to notice file changes); Pillbox's idle cost is unchanged.
- Size: `layout.size` multiplies the monitor's DPI scale into Pillbox's drawing scale
  (`shape::draw_scale`, `Monitor::scale`), so every shape, text and hit area grows together;
  `Monitor::dpi` keeps the plain DPI scale for the windows that do not grow with Pillbox
  (chat drawer, System window, popped-out widgets).
- Per-app rules: `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)`, so Windows tells us when the front
  app changes.
- Now playing: WinRT `GlobalSystemMediaTransportControlsSessionManager` on its own thread;
  Windows raises events for track, play-state and position changes. Artwork is decoded with WIC.
- Visualizer: WASAPI loopback capture of the default output device on its own thread, a
  1024-point FFT into six bands with automatic gain. Runs only while music is playing.
- Lock keys: Raw Input (`RIDEV_INPUTSINK`), not a keyboard hook, so it cannot slow typing.
- M4 additions: `RegisterHotKey` for shortcuts; `AddClipboardFormatListener` for the clipboard
  (event-driven); DPAPI (`CryptProtectData`) for the stored history; `Windows.Media.Ocr` for text
  in pictures; OLE `RegisterDragDrop` / `DoDragDrop` with the shell's own data object for the
  shelf; `IShellItemImageFactory` for file icons and thumbnails (worker thread);
  `IAudioEndpointVolume` for volume and microphone mute; WMI and DDC/CI for brightness and
  `Windows.Devices.Radios` for Wi-Fi/Bluetooth (worker thread); the Native Wifi API and
  `Windows.Devices.Bluetooth` for the lists on the Controls cards (a second worker thread);
  `Windows.Globalization.Calendar`
  for world clocks; `PlaySound` with the Windows alarm sound for reminders. Background work runs
  on plain threads and wakes the UI thread with a posted message, as before.
- M5a additions: `SHQueryUserNotificationState` plus a client-area check of the foreground
  window (on foreground changes, display changes, and a 2-second re-check timer that draws
  nothing); `GetSystemTimes`, `GlobalMemoryStatusEx` and `GetIfTable2` for the strip, smoothed;
  an empty window region while the strip shows, so it is fully click-through. Camera: Media
  Foundation source reader in asynchronous mode (`IMFSourceReaderCallback`) with video processing
  to RGB32, converted and mirrored on Media Foundation's thread, uploaded into one reused Direct2D
  bitmap. Desktop widgets: `WM_NCHITTEST` returns `HTCLIENT` over buttons and `HTCAPTION`
  elsewhere.
- **New dependency (M5b): `webview2-com` 0.38** for the drawers. The spec named WebView2 for
  embedded pages; writing its ~100 COM interfaces by hand would be far more unsafe code. 0.38 is
  the last release built on `windows` 0.61, so the app still uses one `windows` version. Its
  loader binding expects `WebView2Loader.dll` next to the exe, so the loader is embedded with
  `include_bytes!`, written to `%LOCALAPPDATA%\Pillbox\` on first use and loaded by full path
  (`LoadLibraryW` + `GetProcAddress`). The exe keeps running from any folder on its own.
- Threads instead of an async runtime: the plan suggested `tokio`, but each background job here
  (media, audio, one-shot HTTP requests) is a simple blocking loop, so plain threads with
  channels do the job with less memory.
- **Stack change (M6): `axum` 0.8 + `tokio` for the phone server**, as the spec suggested for
  local servers. Why:
  - An HTTP server must handle a phone's parallel requests (page, preview, download),
    streaming multi-gigabyte uploads, `multipart/form-data` parsing, keep-alive and a clean
    "stop accepting, let running transfers finish" shutdown. Hand-rolling that on threads would
    be a lot of security-sensitive parsing code.

  How it is kept small:
  - tokio is not the app's runtime. Each QR code starts one thread with a *current-thread*
    tokio runtime. Dropping the code's handle signals shutdown, and after transfers finish (at
    most 10 minutes) the thread and runtime are gone.
  - Everything else in the app stays on plain threads, and no tokio code runs while no code is
    on screen.
  - axum is built with only `http1`, `tokio` and `multipart`.
  - Also new: `tokio-util` and `futures-core` (stream a file to the phone and notice when the
    last byte went out) and `qrcode` 0.14 (pure-Rust QR encoder, no image features; drawn by the
    Pillbox itself as pixel-snapped rectangles).
  - Test-only: `rqrr` (a QR decoder) proves the drawn code decodes. It is used in a unit test
    and in `examples/qrdecode.rs` and is not in the app binary.
  - New `windows` features: `Win32_Networking_NetworkListManager` and
    `Win32_NetworkManagement_WindowsFirewall` (read-only rule check).
- **Stack (M7): the same `axum` + `tokio`, now always on, for the script API.** Reusing the M6
  server code and its tested checks (constant-time token, Host check) is safer than a second,
  hand-rolled HTTP parser, and the hold-open permission request needs an async wait. Unlike the
  phone server it lives as long as Pillbox: one thread, a current-thread runtime, blocked
  in the OS until a request arrives (0.001% CPU measured; about +3–4 MB and +4 threads). No new
  crate for it; the `tokio` `sync` feature (a one-shot channel per held permission question) was
  already on.
- **Stack (M8 and the System card): no new crates.** New `windows` features only:
  `ApplicationModel`, `Foundation_Collections`, `UI_Notifications` and
  `UI_Notifications_Management` (the notification reader, plain WinRT calls with no `unsafe`
  beyond `RoInitialize`), and, for the System card, the already enabled `Win32_Graphics_Dxgi`
  (adapter list), `Win32_System_Performance` (the `PhysicalDisk` counters) and
  `Win32_NetworkManagement_IpHelper` (`GetBestInterface`). `spike-identity/` is a separate
  throwaway crate in the workspace and is not built with the app (`default-members`).
- **New crate (M7): `pillboxctl`.** Hooks start it on every tool call, so it is its own small
  binary (about 2.4 MB, starts in a few ms) and not `pillbox.exe` with a flag, which would load
  the whole renderer. Its dependencies: `ureq` 2 **without** TLS (plain HTTP to 127.0.0.1;
  Pillbox's own `ureq` keeps TLS for the web), `serde_json` with `preserve_order` (keeps the
  key order of your `settings.json` when `hooks --apply` merges in; this feature is shared with
  Pillbox when built together, harmless there) and the `windows` crate with
  `Win32_System_Diagnostics_ToolHelp` (the process list, to find the agent's process).
  It is not in `default-members`: `cargo run` still means Pillbox.
- **Hooks are `pillboxctl.exe`, not scripts.** The existing hooks on this PC are PowerShell
  scripts (about 150 ms to start); the statusLine one needs Node. A native executable starts
  in milliseconds and needs nothing else. In status line mode it runs the replaced status line
  itself (std `Command` with `raw_arg`, no new dependency), so with the node script chained
  one refresh takes about 0.4 s, mostly Node starting.

## Known fragile things

- **The lyrics column needs the wide card.** It is on the top edge only; on a left or right edge
  the Music tab keeps the compact player and shows no lyrics. The reactive background reacts to
  the audio levels and to lyric line changes, not to what the words mean, and it needs the
  loopback audio capture (like the visualizer), which hears everything the PC plays.
- **Lyric line heights come from measured text.** Wrapping uses the renderer's DirectWrite
  measure of each candidate line for the 17 lines around the one in view, every frame (the
  measure cache holds 1024 entries and clears itself when full). A song with very long lines
  scrolled fast could redo some measuring; nothing grows without limit.
- **Recording a shortcut that is already taken.** The shortcut recorder sees only keys that reach
  the Settings window. A combination another app (or Pillbox itself) has registered with
  `RegisterHotKey` is swallowed by Windows, so it cannot be recorded; swapping two of the
  Pillbox's own shortcuts needs one of them cleared first. The Win key is not offered (egui does
  not report it); it can still be written in the file.
- **The frameless Settings window.** Moving it uses `ViewportCommand::StartDrag` from the
  sidebar top and the page heading; resizing relies on winit keeping a resizable frame on an
  undecorated window. Both were not tried with a real pointer (only the headless click tests).
- **Glass feedback loop.** Windows.Graphics.Capture delivers a new frame every time Pillbox
  itself redraws, even though Pillbox is excluded from the capture. The app breaks the loop
  by hashing the captured strip and ignoring identical frames. Observed behaviour, not documented.
- **Glass costs more.** A whole-monitor capture session runs while a glass surface is showing.
  When something behind Pillbox moves constantly (video), it re-blurs up to 30 times a second.
- **Capture border.** The app asks for no yellow capture border (`IsBorderRequired = false`).
  If a Windows build ignores that, a thin border appears around the screen while glass is on.
- **Monitor names are not stable on this laptop.** The same built-in panel was reported as
  `\\.\DISPLAY5` on one start and `\\.\DISPLAY1` on another (hybrid graphics). A monitor chosen
  by name can therefore stop matching; Pillbox then falls back to the primary monitor.
- **Soft shadows outside the pill are not possible in this window.** The window region clips
  drawing as well as input. A shadow needs a second, permanently click-through window.
- **Lock-key state is tracked, not read.** Windows gives no event for "Caps Lock is now on"
  and a window that never has focus cannot ask reliably. Pillbox reads the three states once
  at start-up and flips its own copy on each key press. If a program changes a lock state
  without a key press, the notice will say the opposite until Pillbox restarts.
- **Lock-key notices wake Pillbox on every key press** (Raw Input delivers all keys; only
  the three lock keys are looked at, nothing is stored). Turn the option off to avoid that.
- **Players report their position differently.** Media Player only reports every few seconds,
  so after a seek Pillbox shows the requested position until the player's next report.
  Players that never report a length get no seek bar. foobar2000 and some others need their own
  plug-in or setting before they appear to Windows at all.
- **The visualizer hears everything**, not just the music app: it listens to the default output
  device, so a game or call playing at the same time moves the bars too.
- **Lyrics timing depends on the match.** LRCLIB is matched by title, artist, album and length;
  a different edit of the song gives lyrics that drift. Browser titles such as
  "Artist - Song (Official Video)" are skipped as videos, others may simply not be found.
- **Hidden until hover** makes a 3-pixel band on the screen edge (as long as the pill) swallow
  clicks while Pillbox is out of sight, and the gap between edge and pill while it is showing.
- **Windows may allow a just-started Pillbox to take the foreground without a click.** During
  testing, a freshly launched test instance's `SetForegroundWindow` succeeded with a game in
  front. In normal use Pillbox only asks right after you click one of its text boxes.
- **Typing into Pillbox takes focus, briefly and on purpose.** Clicking a text box calls
  `SetForegroundWindow` on Pillbox (allowed because the click was its last input) and gives
  the keyboard back on Esc / click elsewhere / close. If the previous window closed meanwhile,
  Windows decides who gets focus. Another program that grabs the foreground on its own (a window
  manager did during testing) can take the keyboard away mid-typing.
- **Mic/camera indicator reads undocumented registry data.** Which app uses the microphone or
  camera comes from `HKCU\...\CapabilityAccessManager\ConsentStore\{microphone,webcam}`, the data
  behind Windows' own privacy icons (a `LastUsedTimeStop` of 0 means "in use"). Not a published
  API; a Windows update could change it, and then the dot and call timer simply stop appearing.
  Watched with registry change notifications, no polling.
- **Global shortcuts can be taken.** `RegisterHotKey` fails if another program owns the
  combination. Some apps with low-level keyboard hooks, or games with exclusive input, can also
  swallow them. Tested only with key presses injected while an ordinary window was in front.
- **Clipboard pictures cost memory and disk.** Each recorded picture is kept as an encrypted PNG
  (a 1080p screenshot is a few MB). Only 30 unpinned pictures are kept.
- **Brightness depends on the display.** Built-in panels go through WMI; external monitors only
  if their DDC/CI option is on. Changes are coalesced on a worker thread.
- **Mouse wheel over Pillbox** relies on Windows' "scroll inactive windows" setting (on by
  default); with it off the lists do not scroll and the tab row does not switch tabs.
- **Windows can stack a normal window above the always-on-top Pillbox.** Seen 2026-11-19 on
  build 26300: right-click a clipboard picture (Photos opens), close Photos, and the window
  Windows activates next (Claude here) sat above Pillbox in the z-order, although the
  Pillbox still had `WS_EX_TOPMOST`; playing music did not bring it back. Pillbox now puts
  itself back on top (`Window::raise`: `SetWindowPos(HWND_TOPMOST, NOMOVE | NOSIZE |
  NOACTIVATE)`) on every foreground change (and once more 400 ms later), when a notice shows,
  when notifications stack, and on every media update. Re-tested the same sequence: only the
  shell's own topmost windows stay above it.
- **"Claude runs" means a process named `claude.exe`.** The desktop app (`Claude.exe` in
  `WindowsApps\Claude_…`), its built-in Claude Code and the terminal `claude` all have that name.
  Another program called `claude.exe` would count too, and a renamed one would not.
- **The status line chain** (`pillboxctl statusline claude --apply`) runs your old status line
  command through `cmd.exe`, while Claude Code ran it through its own shell. A command that only
  works in bash (`$VAR`, `~/`) may print nothing there; then pillboxctl prints its own
  "Claude 5h 42%" line instead. If Claude Code ever stops sending `rate_limits` to the status
  line, the limits stop updating and the card says "as of …".
- **Popped-out widgets** each have their own GPU surface (a few MB each). Typing into one takes
  the keyboard like Pillbox does; the app to return it to is remembered once for all of the
  Pillbox's windows, so moving between widget and Pillbox boxes still hands it back correctly.
- **Real DPI changes and multiple monitors** are handled in code but were only exercised with
  `PILLBOX_SCALE`. A second monitor is now attached; Pillbox stays on the one in settings.
- **Which fullscreen app is a game is a guess.** Windows has no "this is a game" signal for
  borderless windows (the shell even reports "accepts notifications" for a borderless game), so a
  fullscreen window counts unless its executable is on the built-in non-game list or your ignore
  list. A fullscreen app missing from the list (a new browser, a viewer) shrinks Pillbox; add it
  to `games.ignore_apps` or give it a "show" rule.
- **Fullscreen without a focus change** (F11, a game switching from windowed to borderless) is
  noticed by the 2-second re-check, so the strip can appear up to 2 seconds late.
- **Only Pillbox's monitor counts.** A game fullscreen on another monitor leaves Pillbox as
  it is, on purpose.
- **GPU temperature uses kernel-thunk data.** `D3DKMTQueryAdapterInfo` with
  `KMTQAITYPE_ADAPTERPERFDATA` is in the Windows Driver Kit headers and is what Task Manager
  reads, but it is not a mainstream app API; a driver may report 0 (then no temperature is
  shown). The "busiest adapter" is chosen from the 3D engine load, so on a hybrid laptop an idle
  desktop may show the integrated GPU.
- **The chat drawer is a full browser.** An open drawer runs Microsoft Edge WebView2 processes
  (hundreds of MB while open, none after closing). Sites may show their own consent screens
  (Twitch does, once). A drawer you dragged is put back under Pillbox when settings change.
- **The camera may open but send nothing** (privacy switch or shutter closed, camera held by
  another app). Some laptops instead send Windows' own "camera off" picture, which the mirror
  shows as it is. Only the first camera is used.
- **GPU device loss** (driver update, sleep, GPU switch) rebuilds the renderer on the next draw.
  That path has not been triggered in testing.
- **Phone transfer depends on the firewall and the network type.** See One-time permissions.
  Guest Wi-Fi and some mesh/office networks isolate devices from each other ("client
  isolation"). The phone then cannot reach the PC at all, and nothing on the PC can fix that.
- **The firewall warning only looks at rules that name pillbox.exe.** `INetFwPolicy2` does not show
  that most program-less rules are limited to one Store app's package or one service. On this PC,
  29 such rules (Teams, Xbox, Camera…) look like "allow everything" through that API but let
  nothing reach Pillbox, so they are ignored. A genuine allow-everything rule, or a Group
  Policy or third-party firewall, can therefore produce a warning that is wrong. It clears the
  moment a phone connects. (A page load from this PC never clears it: local connections skip the
  firewall.)
- **The network category comes from the Network List Manager.** Adapters are matched to
  networks by adapter GUID. If that lookup fails, every network counts as Public and Wi-Fi is
  picked first.
- **Claude Code's hook payloads are not a stable contract.** Pillbox reads
  `hook_event_name`, `session_id`, `cwd`, `transcript_path`, `prompt`, `tool_name`,
  `tool_input`, `last_assistant_message` and `notification_type`, all seen in real payloads
  (SessionStart, UserPromptSubmit, SessionEnd were captured live from `claude 2.1.300`; the
  others are as the user's existing notifier hooks use them). An unknown event or a missing
  field is ignored, but a renamed field would silently stop updating that part of a card.
- **Model and effort come from the transcript, not from the hooks.** The last assistant line of
  `transcript_path` carries `message.model` and `perTurnEffort` (undocumented transcript
  format). A `/model` switch inside a running chat shows up at the next turn start or end. A
  session's card shows the effort of its last answer, not a configured default.
- **Allow / Deny answers the hook with `hookSpecificOutput.decision.behavior`** (`allow` /
  `deny`), the documented format of a PermissionRequest hook as I know it. **The hold-open
  round trip was tested against the real Pillbox (hook held, click released it, timeout and
  "island not running" both fall back), but no real Claude Code session has been asked a real
  permission through it yet**, because the headless `claude` here could not log in
  (expired OAuth session in this shell). Try it once before relying on it.
- **Stop needs the agent's process id.** `pillboxctl hook` finds it by walking up from itself to
  the first `claude.exe` / `codex.exe`. If the agent runs under another name (an npm install
  starts `node.exe`), a card has no Stop button. Before killing, Pillbox checks that the
  process id still is a `claude.exe` / `codex.exe` (ids are reused), never touches Windows' own
  processes or itself, and ends the whole process tree with `taskkill /T /F`.
  Stopping a Claude Code session started from the Claude desktop app ends that session.
- **Claude Code is connected on this PC** (`pillboxctl hooks claude --reply --apply`, 2026-11-19;
  backup `~\.claude\settings.json.pillbox-backup-1795108015`). The hook paths point at
  `target\release\pillboxctl.exe`: if that file moves, run the command again. While
  `pillboxctl.exe` is being rebuilt the hooks briefly fail to start, which Claude Code ignores.
  Running chats keep the hooks they started with; new chats use the new ones.
- **Codex is wired the same way but untested here.** `~\.codex\hooks.json` exists on this PC,
  but there is no `codex` command, so no real Codex event was ever sent. Antigravity and
  Hermes are not installed and have no hook integration: they can report through `pillboxctl
  agent`, nothing more. Codex's hook payloads are assumed to match Claude Code's field names.
- **The Activity tab does not survive a restart.** The board lives in memory; sessions reappear
  on their next event.
- **Hooks must stay fast.** Each Claude Code tool call starts `pillboxctl.exe` (a few ms); with
  Pillbox closed it exits at once. The reply hook is the only one that waits.
- **Phones may pause a big download when the screen locks.** The server allows a running
  transfer to finish for up to 10 minutes after the code closes, then cuts it off.
- **The link is plain HTTP** (see Network use for what that means on shared networks).
- **Notification access is undocumented for an unpackaged exe.** The Microsoft docs describe a
  manifest capability (package identity) for `UserNotificationListener`. On this Insider build
  the exe has access with no identity and no prompt; a stable Windows 11 may answer "denied" (the
  tab then says where to allow it) or fail. A sparse package is the documented fallback and has
  not been built.
- **Notifications are read by polling.** `NotificationChanged` fails without package identity,
  and a read costs about 12 ms of CPU, so there is a delay of up to 3 s and a standing idle
  cost (see Measured cost). Only toast notifications are read; apps that post none show nothing.
  Classic desktop apps give no logo through the listener, so their icon comes from the Start
  menu entry; a program with no Start menu entry keeps the letter badge.
- **The System window is resized with `WS_THICKFRAME` and no frame.** Windows only lets a window
  be sized by the user if it has a sizing border, so the window has one and answers
  `WM_NCCALCSIZE` with "no frame" (the whole window is client area, nothing is drawn for it);
  `WM_NCHITTEST` makes the right 10 px the sizing handle and `WM_GETMINMAXINFO` fixes the
  height and the width range. Tested by sending those messages and `SetWindowPos`, not by
  dragging with a real mouse.
- **The GPU shown on the System card is a heuristic** (dedicated = at least 512 MB of its own
  video memory); an AMD APU with a big memory carve-out would be taken for a dedicated card.
  The choice is logged at start.
- **Wi-Fi list and joining are untested on hardware.** The development PC has no Wi-Fi adapter,
  so only the "No Wi-Fi adapter" path was run for real; reading the list, `WlanConnect` and
  `WlanDisconnect` follow the documented calls and compile, and the ordering logic is unit
  tested. Windows 11 may also return an empty list until **Location** is on (the same rule as
  for the Windows flyout). Only networks with a saved profile can be joined from the card.
- **Bluetooth connect and disconnect for audio devices** use two driver-level tricks, not a
  documented "connect" call: *disconnect* sends `IOCTL_BTH_DISCONNECT_DEVICE` (value
  `0x0041000C`, which `windows-rs` does not define) to the first Bluetooth radio, and *connect*
  switches the device's A2DP, hands-free and headset services off and on again with
  `BluetoothSetServiceState`. Listing the paired devices was checked on the development PC;
  connecting and disconnecting a real device was not run (it would interrupt whatever the user
  is listening to). A device may reconnect itself right after a disconnect if it initiates the
  link. Only the first Bluetooth radio is used.
- **The window is 442 px tall at 100%** (the Controls card with a list open is 426 DIPs tall), so
  the transparent window is 96 px taller than it was; the shape region still limits clicks to
  the visible card, and the layout tests state the new numbers.
- **The exe icon is a hand-written `.res` file.** `build.rs` writes the resource entries
  itself (no resource compiler) and passes the file with `cargo:rustc-link-arg-bins`; it works
  with the MSVC linker only (other targets build without an icon). Explorer caches icons, so an
  old icon can linger until the cache is rebuilt.
- This PC runs a Windows Insider build (26300); shell behaviour may shift between updates.

## M8 spike: reading Windows notifications (findings; the mirror is built on them)

`spike-identity/` is a throwaway test exe (not part of the app). Run on this PC (Windows 11
Insider 26300), with **no package identity**, it:

- had notification access already (`GetAccessStatus` = Allowed, no consent prompt) and listed
  the 12 toast notifications on screen: app name, app id (AUMID), title and body text (two text
  elements), and the app logo for packaged apps (Claude, Phone Link, Settings).
- got **no logo** for classic desktop apps (the NVIDIA App): the call returns null, so a fallback
  (the exe's icon, or a letter) is needed.
- **could not subscribe to `NotificationChanged`**: `0x80070490` "Element not found". Without
  identity that event is not available, so the mirror polls `GetNotificationsAsync` (toasts only).
  Measured: a new toast was seen 0.2 s after it appeared and its removal 0.2 s after it was
  dismissed, with a 1 s poll; one read of 12 notifications took 23 ms on average (debug build).
- **a read is not cheap:** even `GetNotificationsAsync().get()` alone costs about 12 ms of CPU in
  Pillbox's process (Windows marshals every notification), whatever is done with the result.
  Polling every second cost 0.3% of the machine, so the mirror polls every 3 s (see Measured
  cost). Hints that a notification arrived were tried and dropped: the notification database
  (`wpndatabase.db`) is not rewritten when a toast arrives, and no new window appears.
  Event-driven reading would need a package identity (a sparse package signed with a
  self-signed certificate you trust); not tried, since polling works.
- This relies on the notification-access setting this Windows build exposes; the Microsoft docs
  describe a manifest capability (so package identity), and a stable Windows 11 may refuse an
  unpackaged exe. A sparse package is the documented fallback; it was not needed here and was
  not built or tested. The test never records what a notification says, only its app and ids.

## Not available yet, by design

- **CPU** shows load only. CPU temperature was dropped (no documented Windows API for it).
- **Choosing speakers and microphones** from Pillbox: Windows has no documented API to
  change the default audio device (only the undocumented `IPolicyConfig`), so the Controls tab
  opens Windows' sound settings instead.
- **Sending a folder or several shelf files at once to the phone**: one file per code for now
  (the phone button is not shown on folders).
- **Next events** come from Pillbox's own reminders; no calendar account is read.
- **Replying with text to a running agent chat.** See "Scripts and agents": no hook can do it.
- **The Activity tab keeps no history** across restarts.

## Out of scope (decided)

Pillbox pop-ups for volume, brightness, battery and headphones (they would need the native ones
suppressed), native volume/brightness OSD suppression, ETW-based FPS and CPU temperature, taskbar replacement,
and features that rely on undocumented Windows internals (night light, focus assist and power-mode
toggles, naming the app that is capturing the screen).
