# bloqsync

> ## Hinweis / Notice
>
> **Dieser Code wurde vollständig von KI erzeugt** – Modell **DeepSeek V4.1
> Flash**, verwendet über **OpenRouter**. Verbrauch für dieses Projekt:
> **~504,3K Tokens (Context)**; Kosten über OpenRouter: **1,29 US‑$**. Es ist
> **kein** von Hand geschriebenes Projekt.
>
> Der Autor hat **nicht vor**, dieses Projekt zu maintainen oder
> weiterzuentwickeln. Bereitstellung **as‑is**, ohne Support oder Gewährleistung.
>
> **Lizenz: [Unlicense](LICENSE)** (Public Domain). Jeder darf **absolut alles**
> damit machen – benutzen, ändern, verkaufen, veröffentlichen, ohne Bedingungen.
> **Einfach benutzen oder einen Fork daraus machen.**
>
> ### Keine Verbindung zu Robobloq / Marken
> Dieses Projekt steht in **keinerlei Verbindung** zu Robobloq, Corsair, iCUE
> oder verbundenen Firmen und wird von **niemandem** davon gesponsert,
> unterstützt, autorisiert oder geprüft. „ROBOBLOQ“, „SyncLight“ und andere
> genannte Namen sind Marken ihrer jeweiligen Inhaber und werden hier **nur
> beschreibend** verwendet (um zu benennen, welches Gerät angesteuert wird).
> Es wird **kein Anspruch** auf diese Marken erhoben. Das Reverse Engineering
> erfolgte **für Interoperabilität** mit einem selbst erworbenen Gerät.
>
> ### Nur mit genau diesem Modell getestet
> Getestet wurde **ausschließlich** mit:
> **ROBOBLOQ SyncLight, 24″-Variante, 54 LEDs** (Firmware **1.9.4**), auf
> **Zorin OS 18.1 / GNOME 46 / PipeWire 1.0.5**, mit **zwei Monitoren und
> zwei Leisten**.
>
> **Andere Größen, LED-Zahlen und Revisionen sind NICHT getestet.** Das
> Protokoll sollte zwar skalieren, aber: die Update-Rate **sinkt mit steigender
> LED-Zahl** (mehr Sektionen → mehr 64-Byte-Frames pro Update), und das
> **Standard-Zonenlayout der GUI ist auf 54 LEDs ausgelegt** (18/18/18).
> Nutzung auf eigenes Risiko – es wurden keine Tests mit anderen Modellen
> durchgeführt.

Hochperformante Bildschirm-Synchronisation („Ambilight“) für die
**ROBOBLOQ SyncLight** USB-LED-Leiste unter Linux (GNOME/Wayland).

Dies ist ein sauberer Neuaufbau (Greenfield). Das bekannte Projekt
`openLightsSync` diente nur als Referenz – es nutzt den langsamen Pfad und
eine falsche Frame-Kodierung und erreicht dadurch nur wenige FPS mit
Aussetzern.

## Installation

**Für Anwender (empfohlen): fertige Pakete aus den GitHub *Releases*.**

- **Debian / Ubuntu / Zorin / Mint / Pop!\_OS (.deb)**
  ```bash
  sudo apt install ./bloqsync_*.deb
  ```
  (oder Doppelklick – `apt` zieht alle Abhängigkeiten automatisch)
- **Fedora / openSUSE (.rpm)**
  ```bash
  sudo dnf install ./bloqsync-*.rpm
  ```
- **Andere Distributionen (AppImage)**
  ```bash
  chmod +x bloqsync_*.AppImage && ./bloqsync_*.AppImage
  ```

Beim `.deb`/`.rpm` werden die nötigen Runtime-Bibliotheken (WebKitGTK, GTK3,
PipeWire, PulseAudio, …) automatisch mitinstalliert – **kein Rust, keine
Entwicklerpakete, keine Handarbeit**. Das Paket enthält außerdem die
udev-Regel (Leisten-Zugriff + Tastatur-Interface unterdrücken).

**Aus dem Quellcode (Entwickler):** `./install.sh` (installiert Build-Deps,
baut und packt ein `.deb`) oder siehe „Bauen & Ausführen".

> Beim ersten Bildschirm-Sync fragt GNOME einmal pro Login nach der
> Freigabe (Sicherheitsfunktion des Portal). Danach läuft alles automatisch.

## Ergebnis

- Flüssiger Sync mit vollem **54-LED**-Farbverlauf (Standard: **24 fps**,
  Glättung **0.22**; bis ~30 fps möglich)
- Echtes Per-LED/Per-Zonen-Steuerung über das schnelle `setSyncScreen`-Protokoll
- Capture über **PipeWire** (xdg-desktop-portal ScreenCast), nicht per Screenshot-Subprozess
- Geräte-Autoerkennung, Konfigurations-Persistenz, Tauri-GUI
- **Kino-Modus (Audio-reactive)** für DRM-Inhalte (Netflix u. a.): feste
  Grundfarbe + fließende, kontrastbasierte Helligkeit aus dem Ton

---

## Hardware & Protokoll (reverse-engineered & verifiziert)

Gerät: `VID 0x1A86 / PID 0xFE07` (Hersteller „ROBOBLOQ“), **HID Interface 0**
(Vendor, Usage Page `0xFF00`), Interface 1 = Tastatur (Touch-Buttons, ignorieren).

- **Unnummerierte 64-Byte-HID-Reports** (Report-Descriptor ohne Report-ID).
- Live verifiziert: **Firmware 1.9.4, 54 LEDs**, UUID (Beispiel `a1b2c3d4e5f60718`; pro Gerät verschieden).
  Die Serial ist bei allen Geräten identisch (`0123456789`), taugt also nicht
  zur Unterscheidung – dafür **Geräte-UUID** verwenden (Fallback: USB-Port-Pfad).

### Rahmen (Framing)

```
RB:  52 42  LEN   ID  ACT  payload…  CHK          LEN = Gesamtlänge (1 Byte)
SC:  53 43  LENhi LENlo ID ACT payload… CHK        LEN = 16-bit Big-Endian
CHK = (Summe aller vorherigen Bytes) mod 256
ID  = Sequenzzähler, erster Wert 2, 255 → 1
```

### Wichtige Aktionen

| ACT | Name | Rahmen | Bedeutung |
|----|------|--------|-----------|
| `0x80` 128 | `setSyncScreen` | **SC** | Farb-Streaming (schnell) |
| `0x86` 134 | `setSectionLED` | RB | persistente Farbe |
| `0x87` 135 | `setBrightness` | RB | Helligkeit (1 Byte) |
| `0x82` 130 | `readDeviceInfo` | RB | Antwort: id[5:8], displaySize[8], **lamps[11]**, uuid[12:20], ver[21:23] |

**Farbkodierung** = 5-Byte-Sektionen `[start, R, G, B, end]`, 1-basiert,
inklusive. `end=254` = „bis Ende der Leiste“. Eine Sektion mit `start==end`
adressiert eine einzelne LED.

### Die entscheidenden Erkenntnisse (empirisch)

1. **Keine Reassemblierung über mehrere Reports!** Ein großer SC-Frame
   (277 B, 5 Reports) wird **ignoriert**. Jeder Befehl muss in **ein**
   64-Byte-Report passen → **max. 11 Sektionen pro Frame**.
2. **Mehrere kleine Frames pro Update funktionieren** (das Gerät behält
   bereits gesetzte Sektionen – Komposition), **aber** nur mit einer kurzen
   Pause: **~3 ms zwischen den Frames**. Ohne Pause „rollt“/flackert die
   Leiste. Mit 3 ms Abstand ist der volle 54-LED-Farbverlauf stabil.
3. Für 54 LEDs werden also 5 Frames à 11 Sektionen gesendet; möglich sind bis
   zu ~30 Updates/s (Standard: 24).

> `openLightsSync` scheiterte, weil es (a) nur den langsamen `0x86`-Pfad mit
> 20 ms-Sleeps nutzte, (b) `setSyncScreen` mit 1-Byte-Länge + CRC16 statt
> 16-bit-BE + Summen-Checksum baute und (c) pro Frame einen Screenshot-
> Subprozess startete.

### Schreibweise

Unnummeriertes hidraw: pro Befehl genau 64 Bytes schreiben (mit Nullen
aufgefüllt), **kein** führendes Report-ID-Byte.

---

## Architektur

```
src/
  protocol.rs   Frames (RB/SC), Sektions-Komprimierung, ≤64-B-Chunking, Tests
  device.rs     sysfs-Discovery, rohes hidraw-I/O, Identify, paced send
  capture.rs    PipeWire ScreenCast (ashpd + pipewire-rs), Latest-Frame-Slots
  sampling.rs   Randabtastung → LEDs, Layout (links/oben/rechts/unten), Smoothing
  engine.rs     Sync-Thread: Frame → Sampling → Smoothing → SC-Frames
  main.rs       CLI
gui/            Tauri v2 App (ui/ = HTML/JS/CSS), Config unter ~/.config/bloqsync/
```

Capture: `xdg-desktop-portal` ScreenCast v5 (GNOME-Picker beim Start), ein
PipeWire-Stream pro Monitor, Restore-Tokens möglich.

---

## Bauen & Ausführen

```bash
export PATH="$PATH:$HOME/.cargo/bin"

# CLI
cd ~/Code/bloqsync
cargo build
cargo test

# GUI
cd gui
cargo build
cargo run        # oder: ./target/debug/bloqsync-gui
```

Systemabhängigkeiten: `libhidapi-dev`, `libpipewire-0.3-dev`, `libspa-0.2-dev`,
`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`.

### CLI

```bash
bloqsync list                         # Geräte
bloqsync info                         # FW / LED-Anzahl / UUID
bloqsync fill 255 0 0                 # persistente Farbe
bloqsync off | brightness 200
bloqsync gradient | pattern | stream 8
bloqsync bench 5                      # erreichbare FPS
bloqsync calibrate 10                 # Orientierung bestimmen
bloqsync capture-test 6               # nur Capture testen
bloqsync sync 20                      # Bildschirm-Sync (Monitor wählen)
bloqsync sync 20 --reverse            # gespiegelte Montage
bloqsync sync 30 --fps 30 --smooth 0.4
```

### GUI

Gerät wählen → „Gespiegelt“ je nach Montage → „Monitor wählen & starten“
(GNOME-Dialog) → FPS/Glättung/Zonen/Helligkeit einstellen. Einstellungen
werden in `~/.config/bloqsync/config.json` gespeichert.

---

## Flackern vermeiden (wichtige Erkenntnisse)

Die Leiste hat nur 54 LEDs, muss aber ein 1920×1080-Bild abbilden. Folgt sie
jeder Mikro-Änderung, „springt“ sie sichtbar zwischen Farben. Empirisch
ermittelt:

- **FPS bewusst niedriger** halten: **~24 fps** ist der Sweet Spot. Die
  Glättung wirkt *pro Frame*, daher reagiert die Leiste bei 60 fps in
  derselben Zeit viel schneller auf Rauschen → unruhig.
- **Glättung ~0.22** (Regler 0–1; niedriger = träger/ruhiger).
- **Reports/Update = 0** (Multi-Frame, volle Detailtreue). `1` (atomar, max.
  11 Zonen) war der Versuch gegen „Weiß-Blitzer“, verursacht aber sichtbares
  Springen der Zonen.
- **Weiß-Blitzer** wurden isoliert: konstantes Rot (1 Report) und ein
  *bewegter* Multi-Frame-Verlauf (5 Frames, 3 ms Abstand, kontinuierlich)
  waren beide stabil. Die Blitzer entstehen also nicht durch Multi-Frame,
  sondern durch ein **zu reaktives** Farbverhalten.
- **Isolations-Tests** (mit `python`/CLI) haben gezeigt: Das Gerät hält den
  SC-Zustand, reassembliert aber **keine** Multi-Report-Frames, und verträgt
  keine back-to-back-Frames (3 ms Abstand nötig).

### Optionales Filter-Toolbox (im UI)
Unter „Flacker-Filter“ stehen zum Vergleich bereit: `keine`, `Deadband`,
`Quantisieren`, `Quantisieren + weich`, `Median 3/5`, `Mittelwert 4`,
`Weich (Hysterese+Ramp)` und `Test (Negativ)` (diagnostisch). Der Filter wird
beim **Start** der Leiste angewendet.

## Kino-Modus (Audio-reactive, für DRM/Netflix)

Bei kopiergeschützten Streams liefert der ScreenCast nur Schwarz, daher gibt es
einen rein **audio-reaktiven** Modus:

- Capture des Default-Sink-**Monitors** (PulseAudio/PipeWire).
- FFT-Merkmale: Band-Energien, Gesamtlautheit (RMS mit AGC),
  Spektral-Centroid, Flatness, Onset, Stereo.
- **Kontext/Dynamik**: kurzfristige (~0.15 s) vs. langfristige (~3 s) Lautheit
  → **Kontrast** (Szenen-Schwellen statt Takt).
- Mapping: **feste Grundfarbe** + Ruhepegel; nur die Helligkeit fließt mit
  Szenen-Lautheit und Kontrast (time-based Trägheit). Kein Farbwechsel, kein
  Beat-Flimmern.
- Regler: Grundfarbe, Helligkeit, Ruhepegel, Trägheit, Kontrast, Puls,
  Sensitivität (persistent).

## Plug & Play (Autostart, Hotplug, Restore-Token)

- **Autostart:** Checkbox „Beim Login starten“ schreibt
  `~/.config/autostart/bloqsync.desktop`.
- **Auto-Sync:** Checkbox „Sync automatisch starten“ startet beim App-Start und
  hält die konfigurierten Leisten per **Watchdog** (alle 2 s) am Laufen –
  inkl. **automatischem Reconnect** nach dem Abziehen/Anstecken.
- **Restore-Token:** Die Monitor-Auswahl wird als persistenter Portal-Token in
  `~/.config/bloqsync/config.json` gespeichert. Dadurch erscheint der
  GNOME-Monitor-Dialog nur **einmal**; danach startet alles ohne Dialog.
- **Stabile Identität:** Leisten werden primär über ihre **Geräte-UUID**
  (z. B. `a1b2c3d4e5f60718`, aus dem Gerät ausgelesen) identifiziert – diese
  bleibt gleich, auch wenn die Leiste in einen **anderen USB-Port** gesteckt
  wird. Fallback: USB-Port-ID (`1-2`) bzw. `/dev/hidrawN`.
- **Tastatur-Nervigkeit:** Interface 1 der Leiste tippt beim Einstecken eine
  URL (Herstellerseite). Die Regel `contrib/99-bloqsync.rules` lässt libinput
  das Input-Gerät ignorieren:
  ```bash
  sudo cp contrib/99-bloqsync.rules /etc/udev/rules.d/
  sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=input
  ```

## Status & Grenzen

- **Nicht gewartet:** einmaliger KI-generierter Snapshot (siehe Hinweis oben).
  Issues/PRs werden voraussichtlich nicht bearbeitet → bitte **forken**.
- **Getestet nur mit einem Modell:** ROBOBLOQ SyncLight, **24″-Variante mit
  54 LEDs** (Firmware 1.9.4), auf GNOME/Wayland (Zorin OS 18.1 / GNOME 46,
  PipeWire 1.0.5), zwei Monitore + zwei Leisten. Andere Modelle/LED-Zahlen/
  Revisionen sind **ungetestet**.
- **Capture** via xdg-desktop-portal ScreenCast; der Monitor-Dialog erscheint
  nur beim ersten Mal (Restore-Token).
- **Reverse-engineertes Protokoll** – kann bei anderer Firmware/Revision
  abweichen.
- Multi-Monitor / Multi-Leiste **1:1** ist implementiert und getestet.

## Danksagung

Als Referenz dienten Community-Projekte rund um die SyncLight-Leiste,
insbesondere `openLightsSync`. Der eigentliche Code dieses Repos ist eigenständig
und KI-generiert.

## Lizenz

[Unlicense](LICENSE) – **Public Domain**. Jeder darf absolut alles damit machen,
ohne Bedingungen und ohne Gewährleistung.

