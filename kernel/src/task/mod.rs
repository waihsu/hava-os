//! Timer-driven task scheduling foundation.
//!
//! Tasks are selected round-robin and cooperatively switch at safe yield
//! points. Timer IRQs request a reschedule; they never switch directly from an
//! interrupt-handler stack.

use core::{arch::global_asm, sync::atomic::{AtomicBool, Ordering}};

use x86_64::instructions::{hlt, interrupts};
use spin::Mutex;

const MAX_TASKS: usize = 8;
const TIME_SLICE_TICKS: u64 = 10;
const TASK_STACK_SIZE: usize = 4096 * 4;

#[repr(align(16))]
struct TaskStack([u8; TASK_STACK_SIZE]);

static mut CONSOLE_STACK: TaskStack = TaskStack([0; TASK_STACK_SIZE]);
static mut IDLE_STACK: TaskStack = TaskStack([0; TASK_STACK_SIZE]);
static mut CONTEXT_PROBE_STACK: TaskStack = TaskStack([0; TASK_STACK_SIZE]);

#[repr(C)]
struct CpuContext {
    rsp: u64,
    rbx: u64,
    rbp: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
}

impl CpuContext {
    const fn empty() -> Self {
        Self { rsp: 0, rbx: 0, rbp: 0, r12: 0, r13: 0, r14: 0, r15: 0 }
    }
}

static mut KERNEL_CONTEXT: CpuContext = CpuContext::empty();
static mut CONSOLE_CONTEXT: CpuContext = CpuContext::empty();
static mut IDLE_CONTEXT: CpuContext = CpuContext::empty();
static mut PROBE_CONTEXT: CpuContext = CpuContext::empty();
static CONTEXT_PROBE_COMPLETED: AtomicBool = AtomicBool::new(false);
static RESCHEDULE_REQUESTED: AtomicBool = AtomicBool::new(false);

global_asm!(
    r#"
    .global hava_context_switch
    hava_context_switch:
        mov [rdi + 0], rsp
        mov [rdi + 8], rbx
        mov [rdi + 16], rbp
        mov [rdi + 24], r12
        mov [rdi + 32], r13
        mov [rdi + 40], r14
        mov [rdi + 48], r15
        mov rsp, [rsi + 0]
        mov rbx, [rsi + 8]
        mov rbp, [rsi + 16]
        mov r12, [rsi + 24]
        mov r13, [rsi + 32]
        mov r14, [rsi + 40]
        mov r15, [rsi + 48]
        ret
    "#
);

unsafe extern "C" {
    fn hava_context_switch(current: *mut CpuContext, next: *const CpuContext);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Runnable,
    Running,
    Sleeping,
}

impl TaskState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Runnable => "runnable",
            Self::Running => "running",
            Self::Sleeping => "sleeping",
        }
    }
}

#[derive(Clone, Copy)]
pub struct TaskSnapshot {
    pub id: u8,
    pub name: &'static str,
    pub state: TaskState,
    pub stack_bytes: Option<usize>,
}

#[derive(Clone, Copy)]
struct Task {
    id: u8,
    name: &'static str,
    state: TaskState,
    wake_tick: u64,
    stack_bytes: Option<usize>,
}

struct Scheduler {
    tasks: [Option<Task>; MAX_TASKS],
    task_count: usize,
    current: usize,
    switches: u64,
}

impl Scheduler {
    const fn new() -> Self {
        Self {
            tasks: [None; MAX_TASKS],
            task_count: 0,
            current: 0,
            switches: 0,
        }
    }

    fn add(&mut self, name: &'static str, stack_bytes: Option<usize>) {
        let index = self.task_count;
        assert!(index < MAX_TASKS, "task table is full");
        self.tasks[index] = Some(Task {
            id: index as u8,
            name,
            state: if index == 0 { TaskState::Running } else { TaskState::Runnable },
            wake_tick: 0,
            stack_bytes,
        });
        self.task_count += 1;
    }

    fn rotate(&mut self) -> Option<(usize, usize)> {
        if self.task_count < 2 {
            return None;
        }
        let previous = self.current;
        if self.tasks[self.current].unwrap().state == TaskState::Running {
            self.tasks[self.current].as_mut().unwrap().state = TaskState::Runnable;
        }
        for offset in 1..=self.task_count {
            let candidate = (self.current + offset) % self.task_count;
            if self.tasks[candidate].unwrap().state == TaskState::Runnable {
                self.current = candidate;
                self.tasks[self.current].as_mut().unwrap().state = TaskState::Running;
                self.switches += 1;
                return Some((previous, self.current));
            }
        }
        None
    }

    fn wake_due_tasks(&mut self, ticks: u64) {
        for task in self.tasks[..self.task_count].iter_mut().flatten() {
            if task.state == TaskState::Sleeping && ticks >= task.wake_tick {
                task.state = TaskState::Runnable;
            }
        }
    }

    fn sleep_named(&mut self, name: &str, wake_tick: u64) -> bool {
        let Some(index) = self.tasks[..self.task_count]
            .iter()
            .position(|task| task.is_some_and(|task| task.name == name)) else {
            return false;
        };
        let task = self.tasks[index].as_mut().unwrap();
        task.state = TaskState::Sleeping;
        task.wake_tick = wake_tick;
        RESCHEDULE_REQUESTED.store(true, Ordering::Relaxed);
        true
    }

    fn is_awake(&self, name: &str) -> bool {
        self.tasks[..self.task_count]
            .iter()
            .flatten()
            .any(|task| task.name == name && task.state != TaskState::Sleeping)
    }
}

static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());

/// Registers the kernel's initial logical tasks before timer IRQs begin.
pub fn init() {
    let mut scheduler = SCHEDULER.lock();
    scheduler.add("kernel", None);
    scheduler.add("console", Some(stack_size(&raw const CONSOLE_STACK)));
    scheduler.add("idle", Some(stack_size(&raw const IDLE_STACK)));
}

/// Switches to a private stack and back once to verify the assembly context
/// save/restore path before using it for scheduler-driven task execution.
pub fn demonstrate_context_switch() -> bool {
    CONTEXT_PROBE_COMPLETED.store(false, Ordering::Relaxed);
    unsafe {
        let stack_top = stack_top(&raw const CONTEXT_PROBE_STACK);
        // `ret` consumes this entry address, leaving the new Rust function
        // stack 16-byte aligned according to the x86_64 calling convention.
        let initial_rsp = stack_top - 16;
        (initial_rsp as *mut u64).write(context_probe_entry as *const () as usize as u64);
        PROBE_CONTEXT = CpuContext { rsp: initial_rsp, ..CpuContext::empty() };
        hava_context_switch(&raw mut KERNEL_CONTEXT, &raw const PROBE_CONTEXT);
    }
    CONTEXT_PROBE_COMPLETED.load(Ordering::Relaxed)
}

/// Builds initial return frames for the scheduler-owned console and idle tasks.
pub fn initialize_task_contexts(console_entry: extern "C" fn() -> !) {
    unsafe {
        initialize_context(
            &raw mut CONSOLE_CONTEXT,
            &raw const CONSOLE_STACK,
            console_entry,
        );
        initialize_context(&raw mut IDLE_CONTEXT, &raw const IDLE_STACK, idle_task_entry);
    }
}

unsafe fn initialize_context(
    context: *mut CpuContext,
    stack: *const TaskStack,
    entry: extern "C" fn() -> !,
) {
    let initial_rsp = stack_top(stack) - 16;
    unsafe { (initial_rsp as *mut u64).write(entry as *const () as usize as u64) };
    unsafe { context.write(CpuContext { rsp: initial_rsp, ..CpuContext::empty() }) };
}

extern "C" fn idle_task_entry() -> ! {
    crate::run_idle_task()
}

extern "C" fn context_probe_entry() -> ! {
    CONTEXT_PROBE_COMPLETED.store(true, Ordering::Relaxed);
    unsafe {
        hava_context_switch(&raw mut PROBE_CONTEXT, &raw const KERNEL_CONTEXT);
    }
    loop {
        hlt();
    }
}

/// Gives the compiler a real allocation for each future task stack and
/// returns its usable byte count. The stack pointer is installed during the
/// later context-switch step.
fn stack_size(stack: *const TaskStack) -> usize {
    // `stack` originates from a static allocation; we only inspect its array
    // length here, not its contents.
    unsafe { (*stack).0.len() }
}

fn stack_top(stack: *const TaskStack) -> u64 {
    unsafe { stack.cast::<u8>().add(TASK_STACK_SIZE) as u64 }
}

/// Called from the timer IRQ handler. Actual switching happens at the next
/// safe task yield point, never on the interrupt-handler stack.
pub fn on_timer_tick(ticks: u64) {
    SCHEDULER.lock().wake_due_tasks(ticks);
    if ticks.is_multiple_of(TIME_SLICE_TICKS) {
        RESCHEDULE_REQUESTED.store(true, Ordering::Relaxed);
    }
}

/// Puts the logical console task to sleep until a future timer tick.
pub fn sleep_console_for(ticks: u64) {
    interrupts::without_interrupts(|| {
        let wake_tick = crate::interrupts::ticks().saturating_add(ticks);
        SCHEDULER.lock().sleep_named("console", wake_tick);
    });
}

/// The keyboard event loop uses this to defer console input while sleeping.
pub fn is_console_awake() -> bool {
    interrupts::without_interrupts(|| SCHEDULER.lock().is_awake("console"))
}

/// Switches only when the timer has ended the current time slice.
pub fn yield_if_requested() {
    if RESCHEDULE_REQUESTED.swap(false, Ordering::Relaxed) {
        yield_now();
    }
}

/// Cooperatively switches from the running task to the next runnable task.
pub fn yield_now() {
    let interrupts_were_enabled = interrupts::are_enabled();
    interrupts::disable();
    let selection = SCHEDULER.lock().rotate();
    if interrupts_were_enabled {
        interrupts::enable();
    }
    let Some((current, next)) = selection else {
        return;
    };
    unsafe {
        hava_context_switch(context_for(current), context_for(next));
    }
}

unsafe fn context_for(task_id: usize) -> *mut CpuContext {
    match task_id {
        0 => &raw mut KERNEL_CONTEXT,
        1 => &raw mut CONSOLE_CONTEXT,
        2 => &raw mut IDLE_CONTEXT,
        _ => panic!("task has no CPU context"),
    }
}

/// Returns a fixed-size task snapshot without allocating from the heap.
pub fn snapshot() -> ([Option<TaskSnapshot>; MAX_TASKS], usize, u64) {
    interrupts::without_interrupts(|| {
        let scheduler = SCHEDULER.lock();
        let mut tasks = [None; MAX_TASKS];
        for (index, task) in scheduler.tasks[..scheduler.task_count].iter().enumerate() {
            let task = task.unwrap();
            tasks[index] = Some(TaskSnapshot {
                id: task.id,
                name: task.name,
                state: task.state,
                stack_bytes: task.stack_bytes,
            });
        }
        (tasks, scheduler.task_count, scheduler.switches)
    })
}
