# esp-rtos: task-switch race with both cores scheduling (ESP32-S3)

**Status:** bench builds of this program stop within a second on an ESP32-S3;
with every thread pinned they run on. This exact program, started from power-on
and read over serial, has not been run. See "What was measured".

## What the program does

- Starts esp-rtos on both cores (`esp_rtos::start`, `esp_rtos::start_second_core`).
- Spawns three threads that are not pinned to a core. Each one loops over
  `yield_task()`, so every switch moves a thread that is still ready and that
  the other core can take at once.
- Each thread sets a flag of its own while it runs, and panics if it finds the
  flag already set: two cores are then running the same thread.
- A priority-3 timer interrupt on the first core fires every 1 ms and keeps
  the core busy for 100 µs. It stands in for any interrupt above the context
  switch's priority.
- The main thread prints the loop counters once a second.

No radio, no embassy. It uses esp-hal `main` at `bdcc3b087` (2026-10-09).

## Running

Needs the `esp` toolchain (espup) and espflash, and any ESP32-S3 board.

```sh
cargo run --release                  # stops
cargo run --release --features pin   # the control: runs on
```

Start it with a power-on or an espflash reset. A reset through a debug probe
can leave the second core running, and `start_second_core` then panics with
`CoreAlreadyRunning`.

## Expected

The counters keep growing forever, as they do with `--features pin`.

## What was measured

On one ESP32-S3 board, with builds that differ from this program in two ways:
the panic message went to memory instead of the serial port, and the chip was
restarted once at boot to clear a second core left running by the probe.
State was read over the debug probe; every read halts the cores.

| Threads | Stall interrupt | Result |
| --- | --- | --- |
| unpinned | 1 kHz, 100 µs | stopped within the first second, 2 of 2 boots; the cores were in an exception loop and no panic message was produced |
| unpinned | 20 kHz, 20 µs | `LoadProhibited` in `run_scheduler` on the second core 164 ms after boot, 2 of 2 boots |
| pinned | 20 kHz, 20 µs | ran 45 s and 60 s, until stopped |
| unpinned | none | ran 60 s, 30 s and 30 s with no debugger halt; panicked 4 times when the debugger halted the cores: `Stack overflow detected` ([task/mod.rs](https://github.com/esp-rs/esp-hal/blob/bdcc3b0872b8affbafa8f3bab79f398d07a49b5a/esp-rtos/src/task/mod.rs#L533)), the stack pointer lying in another thread's stack |

Not measured: the pinned control with the 1 kHz stall; what the serial port
shows when the program stops; a start from power-on.

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
