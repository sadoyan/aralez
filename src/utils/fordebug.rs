use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct CountingAllocator;

pub static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
pub static DEALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
pub static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);
#[allow(dead_code)]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        DEALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        System.dealloc(ptr, layout)
    }
}

// Uncomment following lines and comment allocator in main.rs
// #[global_allocator]
// pub static A: CountingAllocator = CountingAllocator;
#[allow(dead_code)]
fn for_example() {
    let before = crate::utils::fordebug::ALLOC_COUNT.load(Ordering::Relaxed);
    let after = crate::utils::fordebug::ALLOC_COUNT.load(Ordering::Relaxed);
    println!("Allocations : {}", after - before);
}
