use dashmap::DashMap;
use futures::channel::mpsc::Sender;
use futures::SinkExt;
use std::fs;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

// pub fn discover() -> DashMap<String, (Vec<(String, u16)>, AtomicUsize)> {
//     read_upstreams_from_file()
// }

pub async fn dsc(mut tx: Sender<DashMap<String, (Vec<(String, u16)>, AtomicUsize)>>) {
    loop {
        let snd = read_upstreams_from_file();
        let _ = tx.send(snd).await.unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

fn read_upstreams_from_file() -> DashMap<String, (Vec<(String, u16)>, AtomicUsize)> {
    let upstreams = DashMap::new();

    // Read file contents
    let contents = match fs::read_to_string("etc/upstreams.txt") {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error reading file: {:?}", e);
            return upstreams;
        }
    };

    // Process each non-empty line
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        let mut parts = line.split_whitespace();

        let Some(hostname) = parts.next() else {
            continue;
        };
        let Some(address) = parts.next() else {
            continue;
        };

        let mut addr_parts = address.split(':');
        let Some(ip) = addr_parts.next() else {
            continue;
        };
        let Some(port_str) = addr_parts.next() else {
            continue;
        };

        let Ok(port) = port_str.parse::<u16>() else {
            continue;
        };
        // println!("Hostname {}, Address: {}, Port: {}", hostname, ip, port);
        // Insert into DashMap using `entry()` for efficiency
        upstreams
            .entry(hostname.to_string()) // Step 1: Find or create entry
            .or_insert_with(|| (Vec::new(), AtomicUsize::new(0))) // Step 2: Insert if missing
            .0 // Step 3: Access the Vec<(String, u16)>
            .push((ip.to_string(), port)); // Step 4: Append new data
    }

    upstreams
}

/*
fn read_upstreams_from_file1() -> DashMap<String, (Vec<(String, u16)>, AtomicUsize)> {
    let contents = std::fs::read_to_string("etc/upstreams.txt");
    let upstreams: DashMap<String, (Vec<(String, u16)>, AtomicUsize)> = DashMap::new();
    match contents {
        Ok(contents) => {
            let t = contents.lines().filter(|line| !line.trim().is_empty()).map(|x| x.to_string()).collect::<Vec<String>>();
            for x in t {
                let vc = x.split(" ").map(|x| x.to_string()).collect::<Vec<String>>();
                let hostname = vc[0].trim().to_string();
                let contents = vc[1].clone().split(":").map(|x| x.to_string()).collect::<Vec<String>>();
                let ip = contents[0].trim().to_string();
                let port = contents[1].trim().parse::<u16>().unwrap().to_owned();

                if upstreams.contains_key(&hostname) {
                    let mut upstream = upstreams.get_mut(&hostname).unwrap();
                    upstream.0.push((ip, port));
                } else {
                    let mut second = vec![];
                    second.push((ip, port));
                    upstreams.insert(hostname, (second.clone(), AtomicUsize::new(0)));
                }
            }
        }
        Err(e) => {
            println!("{:?}", e)
        }
    };
    upstreams
}
*/
