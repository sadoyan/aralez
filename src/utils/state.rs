use crate::utils::tools::Die;
use std::sync::{LazyLock, RwLock};

#[derive(Debug)]
pub struct SharedState {
    pub first_run: bool,
}

pub static GLOBAL_STATE: LazyLock<RwLock<SharedState>> = LazyLock::new(|| RwLock::new(SharedState { first_run: true }));

pub fn mark_not_first_run() {
    let mut state = GLOBAL_STATE.write().die("Writing global state failed");
    state.first_run = false;
}

pub fn is_first_run() -> bool {
    let state = GLOBAL_STATE.read().die("Writing global state failed");
    state.first_run
}
