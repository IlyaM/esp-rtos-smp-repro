# esp-rtos: task-switch race with both cores scheduling (ESP32-S3)

**Status: this program builds, but has not been run on a board yet.** Whether
it shows the fault, and how fast, is not known.

## What the program does

- Starts esp-rtos on both cores (`esp_rtos::start`, `esp_rtos::start_second_core`).
- Spawns three threads that are not pinned to a core. Each one loops over
  `yield_task()`, so every switch moves a thread that is still ready and that
  the other core takes at once.
- Each thread sets a flag of its own while it runs, and panics if it finds the
  flag already set: two cores are then running the same thread.
- A priority-3 software interrupt, raised from the second core, keeps the
  first core busy for 20 µs at a time. It stands in for any interrupt above
  the context switch's priority. `STALL_US = 0` in `src/main.rs` removes it.
- The main thread prints the loop counters once a second.

No radio, no embassy. It uses esp-hal `main` at `bdcc3b087` (2026-10-09).

## Running

Needs the `esp` toolchain (espup) and espflash, and any ESP32-S3 board.

```sh
cargo run --release
```

## Expected

The counters keep growing forever.

## What the fault should look like

One of: an exception with a PC that is not code, the panic
`thread N is running on both cores`, or the once-a-second line stopping.

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
