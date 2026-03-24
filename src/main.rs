mod utils;
mod web;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
// pub static A: CountingAllocator = CountingAllocator;

fn main() {
    web::start::run();
}
