use std::sync::{LazyLock, RwLock};

#[derive(Debug)]
pub struct SharedState {
    pub first_run: bool,
}

pub static GLOBAL_STATE: LazyLock<RwLock<SharedState>> = LazyLock::new(|| RwLock::new(SharedState { first_run: true }));

pub fn mark_not_first_run() {
    let mut state = GLOBAL_STATE.write().unwrap();
    state.first_run = false;
}

pub fn is_first_run() -> bool {
    let state = GLOBAL_STATE.read().unwrap();
    state.first_run
}
