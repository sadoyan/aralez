use crate::utils::lazylock::{CLIENT_HEADERS, SERVER_HEADERS, UPSTREAMS};
use crate::utils::types::{BuildError, HeaderEntries, Headers, HostPaths, HostRouters, InnerMap, Node, PathRouter, Pool, UpstreamsMap};
use log::info;
use matchit::{InsertError, Router};
use rustc_hash::FxHashMap;
use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

fn build_host_router(routes: FxHashMap<Arc<str>, (Vec<Arc<InnerMap>>, AtomicUsize)>) -> Result<Router<Arc<Pool>>, (String, InsertError)> {
    let mut entries: Vec<(String, Vec<Arc<InnerMap>>, AtomicUsize)> = routes
        .into_iter()
        .map(|(k, (servers, index))| (k.trim_end_matches('/').to_string(), servers, index))
        .collect();
    entries.sort_by_key(|(k, _, _)| k.len());

    let mut built: FxHashMap<String, Arc<Pool>> = FxHashMap::default();
    let mut router = Router::new();

    for (path, servers, index) in entries {
        let mut parent = None;
        let mut p = path.as_str();
        while let Some(i) = p.rfind('/') {
            p = &p[..i];
            if let Some(a) = built.get(p) {
                parent = Some(a.clone());
                break;
            }
        }

        let pool = Arc::new(Pool { servers, index, parent });

        let patterns: Vec<String> = if path.is_empty() {
            vec!["/".into(), "/{*rest}".into()]
        } else {
            let pat = to_pattern(&path);
            if pat.contains("{*") { vec![pat] } else { vec![pat.clone(), format!("{pat}/{{*rest}}")] }
        };
        for pat in patterns {
            router.insert(pat, pool.clone()).map_err(|e| (path.clone(), e))?;
        }
        built.insert(path, pool);
    }
    Ok(router)
}

pub fn build_router(input: UpstreamsMap) -> Result<(), BuildError> {
    let mut hosts: HostRouters = FxHashMap::default();
    for (host, routes) in input {
        let router = build_host_router(routes).map_err(|(path, source)| BuildError::Route { host: host.clone(), path, source })?;
        hosts.insert(host, router);
    }
    UPSTREAMS.store(Arc::new(hosts));
    UPSTREAMS.load();
    Ok(())
}
pub fn build_header_routers(client: Headers, server: Headers) -> Result<(), BuildError> {
    let c: HostPaths<HeaderEntries> = build_host_paths(client)?;
    let s: HostPaths<HeaderEntries> = build_host_paths(server)?;
    info!("Built header routers for {} client, {} server hosts", c.len(), s.len());
    CLIENT_HEADERS.store(Arc::new(c));
    SERVER_HEADERS.store(Arc::new(s));
    Ok(())
}
pub fn find<'a, T>(router: &'a PathRouter<T>, path: &str, accept: impl Fn(&T) -> bool) -> Option<&'a T> {
    let mut cur: Option<&Node<T>> = Some(&**router.at(path).ok()?.value);
    while let Some(n) = cur {
        if accept(&n.value) {
            return Some(&n.value);
        }
        cur = n.parent.as_deref();
    }
    None
}

fn to_pattern(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let b = s.as_bytes();
    let mut i = 0;

    while i < s.len() {
        match b[i] {
            b'{' => {
                if b.get(i + 1) == Some(&b'{') {
                    out.push_str("{{");
                    i += 2;
                    continue;
                }
                if let Some(rel) = s[i + 1..].find('}') {
                    let name = &s[i + 1..i + 1 + rel];
                    let ident = name.strip_prefix('*').unwrap_or(name);
                    if !ident.is_empty() && ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                        out.push_str(&s[i..i + rel + 2]);
                        i += rel + 2;
                        continue;
                    }
                }
                out.push_str("{{");
                i += 1;
            }
            b'}' => {
                out.push_str("}}");
                i += if b.get(i + 1) == Some(&b'}') { 2 } else { 1 };
            }
            _ => {
                let c = s[i..].chars().next().unwrap();
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    out
}
pub fn norm_path(path: &str) -> &str {
    let p = path.trim_end_matches('/');
    if p.is_empty() { "/" } else { p }
}

pub fn collapse_slashes(path: &str) -> Cow<'_, str> {
    if !path.contains("//") {
        return Cow::Borrowed(path);
    }
    let mut out = String::with_capacity(path.len());
    let mut prev_slash = false;
    for c in path.chars() {
        if c == '/' {
            if prev_slash {
                continue;
            }
            prev_slash = true;
        } else {
            prev_slash = false;
        }
        out.push(c);
    }
    Cow::Owned(out)
}
fn patterns_for(path: &str) -> Vec<String> {
    if path.is_empty() {
        return vec!["/".into(), "/{*rest}".into()];
    }
    let pat = to_pattern(path);
    if pat.contains("{*") { vec![pat] } else { vec![pat.clone(), format!("{pat}/{{*rest}}")] }
}

pub fn build_path_router<T>(routes: impl IntoIterator<Item = (Arc<str>, T)>) -> Result<PathRouter<T>, (String, InsertError)> {
    let mut entries: Vec<(String, T)> = routes.into_iter().map(|(k, v)| (k.trim_end_matches('/').to_string(), v)).collect();
    entries.sort_by_key(|(k, _)| k.len());

    let mut built: FxHashMap<String, Arc<Node<T>>> = FxHashMap::default();
    let mut router = Router::new();

    for (path, value) in entries {
        let mut parent = None;
        let mut p = path.as_str();
        while let Some(i) = p.rfind('/') {
            p = &p[..i];
            if let Some(a) = built.get(p) {
                parent = Some(a.clone());
                break;
            }
        }
        let node = Arc::new(Node { value, parent });
        for pat in patterns_for(&path) {
            router.insert(pat, node.clone()).map_err(|e| (path.clone(), e))?;
        }
        built.insert(path, node);
    }
    Ok(router)
}

pub fn build_host_paths<T>(input: FxHashMap<Arc<str>, FxHashMap<Arc<str>, T>>) -> Result<HostPaths<T>, BuildError> {
    let mut hosts = HostPaths::default();
    for (host, routes) in input {
        let r = build_path_router(routes).map_err(|(path, source)| BuildError::Route { host: host.clone(), path, source })?;
        hosts.insert(host, r);
    }
    Ok(hosts)
}
