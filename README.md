# bloqsync

> ## Hinweis / Notice
>
> **Der gesamte Code in diesem Repository wurde von KI erzeugt**
> (Modell: **DeepSeek V4.1**). Es ist **kein** von Hand geschriebenes Projekt.
>
> Der Autor hat **nicht vor, dieses Projekt zu maintainen** oder anderweitig
> weiterzuentwickeln. Es wird **as-is** bereitgestellt, ohne Support/Gewähr.
>
> **Lizenz: [Unlicense](LICENSE)** (Public Domain). Jeder darf **absolut alles**
> damit machen – benutzen, verändern, verkaufen, veröffentlichen, ohne
> Bedingungen. **Einfach benutzen oder einen Fork daraus machen.**

Hochperformante Bildschirm-Synchronisation („Ambilight“) für die
**ROBOBLOQ SyncLight** USB-LED-Leiste unter Linux (GNOME/Wayland).

Dies ist ein sauberer Neuaufbau (Greenfield). Das bekannte Projekt
`openLightsSync` diente nur als Referenz – es nutzt den langsamen Pfad und
eine falsche Frame-Kodierung und erreicht dadurch nur wenige FPS mit
Aussetzern.

## Ergebnis

- **~25–30 FPS** bei vollem **54-LED**-Farbverlauf, flüssig, **ohne Flackern**
- Echtes Per-LED/Per-Zonen-Steuerung über das schnelle `setSyncScreen`-Protokoll
- Capture über **PipeWire** (xdg-desktop-portal ScreenCast), nicht per Screenshot-Subprozess
- Geräte-Autoerkennung, Konfigurations-Persistenz, Tauri-GUI

---

## Hardware & Protokoll (reverse-engineered & verifiziert)

Gerät: `VID 0x1A86 / PID 0xFE07` (Hersteller „ROBOBLOQ“), **HID Interface 0**
(Vendor, Usage Page `0xFF00`), Interface 1 = Tastatur (Touch-Buttons, ignorieren).

- **Unnummerierte 64-Byte-HID-Reports** (Report-Descriptor ohne Report-ID).
- Live verifiziert: **Firmware 1.9.4, 54 LEDs**, UUID `a1b2c3d4e5f60718`.
  Die Serial ist bei allen Geräten `0123456789` → zur Identifikation
  USB-Topologie-Pfad (z. B. `1-2`) + UUID nutzen.

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
3. Für 54 LEDs werden also 5 Frames à 11 Sektionen gesendet; Updates laufen
   mit ~25–30 FPS.

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

## Bekannte Grenzen / nächste Schritte

- **Mehrere Monitore 1:1 mit mehreren Leisten**: Kern unterstützt mehrere
  Geräte/Streams; die GUI bedient aktuell eine Leiste. Als Nächstes: mehrere
  Streams pro Portal-Session auf N Leisten mappen (UI).
- **Hotplug/Reconnect**: bei Abziehen/Anstecken wird die Engine neu gestartet
  (manuell). Watchdog geplant.
- **Delta-Updates**: nur geänderte Sektionen senden → höhere FPS / weniger
  USB-Last bei wenig Bewegung.
- **Gamma/Threshold-Kalibrierung**, USB-Hotplug-Erkennung, Autostart.

## Lizenz

Noch festzulegen.
