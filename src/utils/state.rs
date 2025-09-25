use once_cell::sync::Lazy;
use std::sync::RwLock;

#[derive(Debug)]
pub struct SharedState {
    pub first_run: bool,
}

pub static GLOBAL_STATE: Lazy<RwLock<SharedState>> = Lazy::new(|| RwLock::new(SharedState { first_run: true }));

pub fn mark_not_first_run() {
    let mut state = GLOBAL_STATE.write().unwrap();
    state.first_run = false;
}

pub fn is_first_run() -> bool {
    let state = GLOBAL_STATE.read().unwrap();
    state.first_run
}

/*
impl SharedState {
    pub fn mark_first_run(&mut self) {
        self.first_run = false;
    }
    pub fn is_first_run(&self) -> bool {
        self.first_run
    }
}
*/
