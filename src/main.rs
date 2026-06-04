mod tls;
mod utils;
mod web;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
// pub static A: CountingAllocator = CountingAllocator;

fn main() {
    if std::env::args().any(|a| a == "--version" || a == "-v") {
        println!("aralez {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    web::start::run();
}
