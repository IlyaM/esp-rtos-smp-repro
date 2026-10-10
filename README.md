# esp-rtos: task-switch race with both cores scheduling (ESP32-S3)

## What the program does

- Starts esp-rtos on both cores (`esp_rtos::start`, `esp_rtos::start_second_core`).
- Creates three threads with no core, the way esp-radio creates its timer
  thread (`esp_radio_rtos_driver::task_create(.., None, ..)`). Each one loops
  over `yield_task()`.
- A priority-3 timer interrupt on the first core fires every 1 ms and keeps
  the core busy for 100 µs. It stands in for any interrupt above the context
  switch's priority.
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

On one ESP32-S3 board, one boot each, with a build that differs from this
program in two ways: the panic message and the counters went to memory instead
of the serial port, and the chip was restarted once at boot to clear a second
core left running by the debug probe.

| esp-hal | Result |
| --- | --- |
| `bdcc3b087`, unmodified | panics 165 ms after boot: `Detected a write to the stack guard value on ProCpu`. The first core's stack pointer is inside the second core's main stack. |
| the same with [these 4 lines](https://github.com/IlyaM/esp-hal/commit/843047c4f81815951b915424d9e3db11bdc68ca8) (threads made with no core are pinned) | runs; stopped after 60 s and 800,000 loops per thread |

Not measured: this exact program started from power-on and read over serial.

## The mechanism

- On Xtensa the switch copies the incoming context into the interrupt frame
  ([`task_switch`](https://github.com/esp-rs/esp-hal/blob/bdcc3b0872b8affbafa8f3bab79f398d07a49b5a/esp-rtos/src/task/xtensa.rs#L105-L114)). That frame lies
  on the stack of the thread being switched out. The interrupt exit reads it
  back ([`RESTORE_CONTEXT`](https://github.com/esp-rs/esp-hal/blob/bdcc3b0872b8affbafa8f3bab79f398d07a49b5a/xtensa-lx-rt/src/exception/asm.rs#L522-L533))
  after the scheduler lock is released.
- A thread that is still ready is put back in the run queue before the switch
  ([scheduler.rs](https://github.com/esp-rs/esp-hal/blob/bdcc3b0872b8affbafa8f3bab79f398d07a49b5a/esp-rtos/src/scheduler.rs#L313-L320)), and if it is not
  pinned the other core is told to schedule
  ([scheduler.rs, with a TODO on this](https://github.com/esp-rs/esp-hal/blob/bdcc3b0872b8affbafa8f3bab79f398d07a49b5a/esp-rtos/src/scheduler.rs#L333-L347)).
- The other core can therefore resume that thread, on that stack, while the
  first core has not yet read the frame. The thread's next stack writes land on
  the frame, and the first core resumes the incoming thread from what is left.
- The handler runs at interrupt level 1, so an interrupt of a higher priority
  on the first core stretches the time between the lock release and the read.
  The scheduler lock masks interrupts, so one that became pending during the
  switch is taken at the moment the lock is released.
