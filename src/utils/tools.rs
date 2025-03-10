use std::any::type_name;
use std::sync::atomic::AtomicUsize;

#[derive(Debug)]
#[allow(dead_code)]
pub struct UpstreamsStruct {
    pub proto: String,
    pub path: String,
    pub address: (String, u16, bool),
    pub atom: AtomicUsize,
}

#[allow(dead_code)]
pub fn typeoff<T>(_: T) {
    let to = type_name::<T>();
    println!("{:?}", to);
}

pub fn string_to_bool(val: Option<&str>) -> Option<bool> {
    match val {
        Some(v) => match v {
            "yes" => Some(true),
            "true" => Some(true),
            _ => Some(false),
        },
        None => Some(false),
    }
}
