use crate::utils::tools::*;
use std::collections::HashSet;
use tokio::sync::RwLockReadGuard;

#[allow(dead_code)]
pub fn dm(map1: &RwLockReadGuard<UpstreamMap>, map2: &UpstreamMap) -> bool {
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
// #[allow(dead_code)]
pub fn dam(map1: &UpstresmDashMap, map2: &UpstresmDashMap) -> bool {
    // Step 1: Check if both maps have the same keys
    let keys1: HashSet<_> = map1.iter().map(|entry| entry.key().clone()).collect();
    let keys2: HashSet<_> = map2.iter().map(|entry| entry.key().clone()).collect();
    if keys1 != keys2 {
        return false;
    }

    // Step 2: Check if the inner maps have the same keys
    for entry1 in map1.iter() {
        let hostname = entry1.key();
        let inner_map1 = entry1.value();

        let Some(inner_map2) = map2.get(hostname) else {
            return false; // Key exists in map1 but not in map2
        };

        let inner_keys1: HashSet<_> = inner_map1.iter().map(|e| e.key().clone()).collect();
        let inner_keys2: HashSet<_> = inner_map2.iter().map(|e| e.key().clone()).collect();
        if inner_keys1 != inner_keys2 {
            return false;
        }

        // Step 3: Compare values (ignore order)
        for path_entry in inner_map1.iter() {
            let path = path_entry.key();
            let (vec1, _counter1) = path_entry.value();

            let Some(entry2) = inner_map2.get(path) else {
                return false; // Path exists in map1 but not in map2
            };
            let (vec2, _counter2) = entry2.value(); // ✅ Correctly extract values

            // Compare AtomicUsize values
            // if counter1.load(Ordering::Relaxed) != counter2.load(Ordering::Relaxed) {
            //     return false;
            // }

            // Convert Vec to HashSet to compare unordered values
            let set1: HashSet<_> = vec1.iter().collect();
            let set2: HashSet<_> = vec2.iter().collect();
            if set1 != set2 {
                return false;
            }
        }
    }

    true
}
