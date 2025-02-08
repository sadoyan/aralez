use dashmap::DashMap;
use std::sync::atomic::AtomicUsize;

pub fn dashmaps(map1: &DashMap<String, (Vec<(String, u16)>, AtomicUsize)>, map2: &DashMap<String, (Vec<(String, u16)>, AtomicUsize)>) -> bool {
    if map1.len() != map2.len() {
        return false; // Different number of keys
    }
    for entry1 in map1.iter() {
        let key = entry1.key();
        let (vec1, _) = entry1.value(); // Extract value

        if let Some(entry2) = map2.get(key) {
            let (vec2, _) = entry2.value(); // Correctly extract value
            if vec1 != vec2 {
                return false;
            }
        } else {
            return false;
        }
    }
    true
}
