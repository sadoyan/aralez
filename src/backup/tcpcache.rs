/*
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;

type Db = Arc<RwLock<HashMap<Arc<[u8]>, Arc<[u8]>>>>;
const DBG: bool = true;
#[tokio::main]
pub async fn run() {
    println!("\n= = = = = = = = ASYNC TOKIO = = = = = = = = =\n");
    if 1 == 1 {
        return;
    }
    let listener = TcpListener::bind("0.0.0.0:6379").await.unwrap();
    println!("Server is running on 0.0.0.0:6379 !\n");
    let hashmap: Db = Arc::new(RwLock::new(HashMap::new()));
    loop {
        let (socket, _) = listener.accept().await.unwrap();
        let hashmap = hashmap.clone();
        tokio::spawn(async move {
            process(socket, hashmap).await;
        });
    }
}

async fn process(mut socket: TcpStream, db: Db) {
    let mut buf = vec![0; 1024];
    loop {
        let n = socket.read(&mut buf).await.expect("failed to read data from socket");
        match n > 3 {
            true => {
                if let Some((action, key, value)) = process_data(&buf[..n - 1], b' ') {
                    let mut map = db.write().await;
                    match action {
                        [115, 101, 116] => {
                            // SET
                            map.insert(Arc::from(key), Arc::from(value));
                            socket.write_all("Done!\n".as_ref()).await.expect("failed to write");
                        }
                        [103, 101, 116] => {
                            // GET
                            let t = map.get(&Arc::from(key));
                            match t {
                                Some(t) => {
                                    socket.write_all(t.as_ref()).await.expect("failed to write");
                                }
                                None => {
                                    socket.write_all("Not Found !\n".as_ref()).await.expect("failed to read");
                                }
                            }
                        }
                        [100, 101, 108] => {
                            // DEL
                            let y = map.remove(&Arc::from(key));
                            let mut _mssg = "";
                            match y {
                                Some(_) => {
                                    _mssg = "Deleted !\n";
                                }
                                None => _mssg = "Not Found !\n",
                            }
                            socket.write_all(_mssg.as_ref()).await.expect("failed to write");
                        }
                        [98, 121, 101] => {
                            //BYE
                            socket.write_all("Bye!: Closing the connection\n".as_ref()).await.expect("failed");
                            socket.shutdown().await.expect("shutdown socket error");
                            return;
                        }
                        _ => socket.write_all("Unknown command: send (get/set/del)\n".as_ref()).await.expect("failed to read"),
                    }
                }
            }
            false => {
                socket.write_all("Only get/set/del commands are accepted\n".as_ref()).await.expect("failed to respond");
            }
        }
    }
}

fn process_data(data: &[u8], delim: u8) -> Option<(&[u8], &[u8], &[u8])> {
    let action_bytes = &data[..3];

    if DBG {
        match data.get(4..) {
            Some(_d) => {
                println!(" DEBUG => {} : {} ", std::str::from_utf8(action_bytes).ok()?, std::str::from_utf8(&data[4..]).ok()?);
            }
            None => println!(" DEBUG => Goodbye"),
        }
    }

    match action_bytes {
        [103, 101, 116] | [100, 101, 108] => {
            let key = &data[4..];
            let val = &[];
            return Some((action_bytes, key, val));
        }
        [115, 101, 116] => {
            if let Some(pos) = data[4..].iter().position(|&b| b == delim) {
                let (key_bytes, value_bytes) = data[4..].split_at(pos);
                let value_bytes = &value_bytes[1..];
                return Some((action_bytes, key_bytes, value_bytes));
            }
        }
        _ => return Some((action_bytes, &[], &[])),
    }

    None
}
*/
