# esp-rtos on two cores: crash in the scheduler with an unpinned thread (ESP32-S3)

## What the program does

- Starts esp-rtos on both cores (`esp_rtos::start`, `esp_rtos::start_second_core`).
- Creates three threads with no core, the way esp-radio creates its timer
  thread (`esp_radio_rtos_driver::task_create(.., None, ..)`). They only
  count and yield, and panic if one finds itself running on both cores.
- A priority-3 timer interrupt keeps the first core busy for 100 µs every
  millisecond.
- The main thread prints the loop counters once a second.

No radio, no embassy. It uses esp-hal `main` at `bdcc3b087` (2026-10-09).

## Running

Needs the `esp` toolchain (espup) and espflash, and any ESP32-S3 board.

```sh
cargo run --release
```

Start it with a power-on or an espflash reset. A reset through a debug probe
can leave the second core running, and `start_second_core` then panics with
`CoreAlreadyRunning`.

## Expected

The counters keep growing forever.

## What was measured

ESP32-S3, one boot each, with a build that differs from this program in two
ways: the panic message and the counters went to memory instead of the serial
port, and the chip was restarted once at boot to clear a second core left
running by the debug probe.

| esp-hal | Result |
| --- | --- |
| `bdcc3b087`, unmodified | panics 165 ms after boot: `Detected a write to the stack guard value on ProCpu`. The first core's stack pointer is inside the second core's main stack. |
| with [4 lines](https://github.com/IlyaM/esp-hal/commit/843047c4f81815951b915424d9e3db11bdc68ca8) that pin a thread created with no core | runs; stopped after 60 s |

Not measured: this exact program started from power-on and read over serial.

## Why

- A task switch copies the incoming context into the interrupt frame
  ([`task_switch`](https://github.com/esp-rs/esp-hal/blob/bdcc3b0872b8affbafa8f3bab79f398d07a49b5a/esp-rtos/src/task/xtensa.rs#L105-L114)).
  The frame lies on the stack of the thread being switched out. It is read
  back at interrupt exit, after the scheduler lock is released.
- A thread that is not pinned can be taken by the other core as soon as the
  lock is released
  ([scheduler.rs, with a TODO about it](https://github.com/esp-rs/esp-hal/blob/bdcc3b0872b8affbafa8f3bab79f398d07a49b5a/esp-rtos/src/scheduler.rs#L333-L347)).
- The other core runs that thread on that stack and overwrites the frame.

The thread that crashes is the one being switched in, not the unpinned one.
