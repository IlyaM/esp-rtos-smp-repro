//! esp-rtos, ESP32-S3, scheduler on both cores: three unpinned threads that do
//! nothing but yield. See README.md.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    handler,
    interrupt::{Priority, software::SoftwareInterrupt},
    main,
    peripherals::FROM_CPU_INTR2,
    system::{Cpu, Stack},
    time::Duration,
    timer::timg::TimerGroup,
};
use esp_println::println;
use esp_rtos::{CurrentThreadHandle, thread::ThreadSpawner};

esp_bootloader_esp_idf::esp_app_desc!();

const THREADS: usize = 3;

/// How long the priority-3 interrupt keeps the first core busy. It stands in
/// for any interrupt above the context switch's priority.
/// Set to 0 to run without it.
const STALL_US: u32 = 20;

static RUNNING: [AtomicBool; THREADS] = [const { AtomicBool::new(false) }; THREADS];
static LOOPS: [AtomicU32; THREADS] = [const { AtomicU32::new(0) }; THREADS];

#[handler(priority = Priority::Priority3)]
fn stall_first_core() {
    SoftwareInterrupt::new(unsafe { FROM_CPU_INTR2::steal() }).reset();
    Delay::new().delay_micros(STALL_US);
}

fn spin(id: usize) -> ! {
    loop {
        // Only this thread touches its flag, so finding it set means that two
        // cores run this thread at once.
        if RUNNING[id].swap(true, Ordering::SeqCst) {
            panic!("thread {} is running on both cores", id);
        }
        LOOPS[id].fetch_add(1, Ordering::Relaxed);
        if STALL_US > 0 && Cpu::current() == Cpu::AppCpu {
            SoftwareInterrupt::new(unsafe { FROM_CPU_INTR2::steal() }).raise();
        }
        RUNNING[id].store(false, Ordering::SeqCst);

        esp_radio_rtos_driver::yield_task();
    }
}

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    esp_alloc::heap_allocator!(size: 32 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    // Bound on the first core, raised from the second.
    SoftwareInterrupt::new(peripherals.FROM_CPU_INTR2).set_interrupt_handler(stall_first_core);

    static APP_CORE_STACK: static_cell::ConstStaticCell<Stack<8192>> =
        static_cell::ConstStaticCell::new(Stack::new());
    esp_rtos::start_second_core(peripherals.CPU_CTRL, APP_CORE_STACK.take(), || {});

    for id in 0..THREADS {
        // No `with_pinned_to`: either core may run the thread.
        ThreadSpawner::new(4096).spawn(move || spin(id)).detach();
    }

    let mut seconds = 0;
    loop {
        CurrentThreadHandle::get().delay(Duration::from_secs(1));
        seconds += 1;
        println!(
            "{} s, loops: {} {} {}",
            seconds,
            LOOPS[0].load(Ordering::Relaxed),
            LOOPS[1].load(Ordering::Relaxed),
            LOOPS[2].load(Ordering::Relaxed),
        );
    }
}
