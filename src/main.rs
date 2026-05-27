use tikv_jemallocator::Jemalloc;

mod tls;
mod utils;
mod web;

#[global_allocator]
static ALLOC: Jemalloc = Jemalloc;
// static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
// pub static A: CountingAllocator = CountingAllocator;

fn main() {
    web::start::run();
}
