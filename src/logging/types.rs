use crate::logging::core::StructuredSystemLog;
use crate::logging::elasticsearch::ElasticSearch;

pub trait WriteLog {
    fn writelog(&self, msg: &StructuredSystemLog);
}
pub fn sendlog(backend: &str, msg: &StructuredSystemLog) {
    match backend {
        "elasticsearch" => ElasticSearch.writelog(msg),
        "somewhereelse" => println!("somewhereelse"),
        _ => log::warn!("Unsupported logging mechanism : {}", backend),
    }
}
