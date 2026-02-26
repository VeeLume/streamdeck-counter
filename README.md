# V's Counter

A Stream Deck plugin providing counter, timer, and stopwatch actions. Built in Rust.

## Actions

### Counter

Increment, decrement, or apply math operations to a counter displayed on the button.

**Short press** and **long press** can each be configured independently:

| Operation | Behavior |
|-----------|----------|
| Add | Adds the configured value |
| Subtract | Subtracts the configured value |
| Multiply | Multiplies by the configured value |
| Divide | Divides by the configured value |
| Reset | Sets the counter to its initial value |
| Set | Sets the counter to a specific value |
| None | Does nothing |

**Settings:**

| Setting | Description |
|---------|-------------|
| Counter ID | Shared counter name. Leave empty for a per-key counter. |
| Initial Value | The value the counter resets to. |
| Short Action / Value | Operation applied on a short press. |
| Long Action / Value | Operation applied on a long press. |
| Long Press (ms) | How long to hold before triggering the long-press action (default: 500ms). |

Multiple buttons can share a counter by giving them the same Counter ID — all buttons displaying that counter will update together.

---

### Computed Display

Evaluates a mathematical expression over one or more counters and displays the result.

**Settings:**

| Setting | Description |
|---------|-------------|
| Expression | A math expression referencing counter IDs. |
| Missing as zero | Treat undefined counters as 0 (when unchecked, treats them as 1). |

**Expression syntax:**

- Single-letter identifiers refer to counters: `A`, `B`, `c`
- Quoted strings reference counters by ID: `"my-counter"`, `'total'`
- Function syntax: `var("complex-id")`
- Operators: `+`, `-`, `*`, `/`, parentheses for grouping

Examples:
```
A + B
(A - B) * 2
"kills" / "deaths"
var("team-1") + var("team-2")
```

The display updates automatically whenever any referenced counter changes.

---

### Timer

Countdown timer. Displays remaining time in MM:SS format (or HH:MM:SS for durations of one hour or more).

**Controls:**

| Action | Behavior |
|--------|----------|
| Short press | Start / pause |
| Long press | Reset to configured duration |

**Settings:**

| Setting | Description |
|---------|-------------|
| Duration (seconds) | Starting duration, minimum 1 second. |
| Long Press (ms) | Hold duration for reset (default: 500ms). |

The button shows a visual alert when the timer reaches zero.

---

### Stopwatch

Elapsed time tracker. Displays time in MM:SS format, switching to HH:MM:SS once one hour is reached.

**Controls:**

| Action | Behavior |
|--------|----------|
| Short press | Start / stop |
| Long press | Reset to 00:00 |

**Settings:**

| Setting | Description |
|---------|-------------|
| Long Press (ms) | Hold duration for reset (default: 500ms). |

---

## Building from Source

**Requirements:**

- Rust (stable, edition 2024)
- Windows 10 or later

**Build:**

```sh
cargo build --release
```

Copy the resulting binary into the plugin directory:

```sh
cp target/release/plugin.exe icu.veelume.counter.sdPlugin/bin/icu.veelume.counter.exe
```

**Dependencies:**

- [`streamdeck-lib`](https://github.com/veelume/streamdeck-lib) v0.4.3 — Stream Deck plugin API
- [`streamdeck-render`](https://github.com/veelume/streamdeck-render) v0.1.3 — PNG button rendering

---

## Architecture Notes

**Persistent storage:** Counter values are stored in Stream Deck's global settings as a JSON map, keyed by counter ID or button context ID. Values survive plugin restarts.

**Shared counters:** When multiple Counter or Computed Display buttons reference the same counter ID, they communicate via an internal pub/sub event (`COUNTER_CHANGED`). All subscribed buttons re-render immediately when a value changes.

**Timer/Stopwatch threading:** Each running timer or stopwatch spawns a background thread that ticks every 100ms. Threads are coordinated via an atomic epoch counter — starting, stopping, or resetting bumps the epoch, which causes the old thread to exit cleanly before a new one is spawned.

**Button rendering:** All button images are generated dynamically as 144×144 PNG files using the [UAV OSD Sans Mono](https://nicholaskruse.com/work/uavosd) font (by Nicholas Kruse, free for personal and commercial use). Font size scales down automatically to fit longer values.

---

## Plugin Info

| Field | Value |
|-------|-------|
| UUID | `icu.veelume.counter` |
| Version | 1.0.0.0 |
| SDK | Stream Deck SDK v3 |
| Min Stream Deck | 7.1 |
| Min Windows | 10.0 |
| Supported devices | Keypad controllers |
