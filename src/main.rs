//! esp-rtos, ESP32-S3, scheduler on both cores: three unpinned threads that do
//! nothing but yield. See README.md.

#![no_std]
#![no_main]

use core::{
    cell::RefCell,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use critical_section::Mutex;
use esp_backtrace as _;
use esp_hal::{
    Blocking,
    delay::Delay,
    handler,
    interrupt::Priority,
    main,
    system::Stack,
    time::Duration,
    timer::{PeriodicTimer, timg::TimerGroup},
};
use esp_println::println;
use esp_rtos::{CurrentThreadHandle, thread::ThreadSpawner};

esp_bootloader_esp_idf::esp_app_desc!();

const THREADS: usize = 3;

/// A priority-3 timer interrupt on the first core fires every
/// `STALL_PERIOD_US` and keeps the core busy for `STALL_US`. It stands in for
/// any interrupt above the context switch's priority.
const STALL_PERIOD_US: u64 = 1000;
const STALL_US: u32 = 100;

static RUNNING: [AtomicBool; THREADS] = [const { AtomicBool::new(false) }; THREADS];
static LOOPS: [AtomicU32; THREADS] = [const { AtomicU32::new(0) }; THREADS];
static STALL_TIMER: Mutex<RefCell<Option<PeriodicTimer<'static, Blocking>>>> =
    Mutex::new(RefCell::new(None));

#[handler(priority = Priority::Priority3)]
fn stall_first_core() {
    critical_section::with(|cs| {
        if let Some(timer) = STALL_TIMER.borrow_ref_mut(cs).as_mut() {
            timer.clear_interrupt();
        }
    });
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

    static APP_CORE_STACK: static_cell::ConstStaticCell<Stack<8192>> =
        static_cell::ConstStaticCell::new(Stack::new());
    esp_rtos::start_second_core(peripherals.CPU_CTRL, APP_CORE_STACK.take(), || {});

    // Above the spinning threads, so that they cannot starve this one.
    CurrentThreadHandle::get().set_priority(1);

    for id in 0..THREADS {
        let spawner = ThreadSpawner::new(4096);
        // The control: with every thread pinned, the program runs on.
        #[cfg(feature = "pin")]
        let spawner = spawner.with_pinned_to(if id % 2 == 0 {
            esp_hal::system::Cpu::ProCpu
        } else {
            esp_hal::system::Cpu::AppCpu
        });
        spawner.spawn(move || spin(id)).detach();
    }

    let mut timer = PeriodicTimer::new(TimerGroup::new(peripherals.TIMG1).timer0);
    timer.set_interrupt_handler(stall_first_core);
    timer.start(Duration::from_micros(STALL_PERIOD_US)).unwrap();
    timer.listen();
    critical_section::with(|cs| STALL_TIMER.borrow_ref_mut(cs).replace(timer));

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
