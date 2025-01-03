use std::collections::HashMap;
use std::io::{prelude::*, BufReader};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{str, thread};

static PASSWORD: &str = "password";

struct CacheEntry {
    expires_at: Option<Instant>,
    value: String,
}

impl CacheEntry {
    fn new(ttl_seconds: u64, value: &str) -> CacheEntry {
        let expires_at = if ttl_seconds == 0 {
            None
        } else {
            Some(Instant::now() + Duration::from_secs(ttl_seconds))
        };

        CacheEntry {
            expires_at,
            value: value.to_string(),
        }
    }

    fn new_from_request(parts: &Vec<&str>) -> CacheEntry {
        let mut ttl = 0_u64;
        let value;

        if parts.len() == 4 {
            ttl = parts[2].parse::<u64>().unwrap_or(0);
            value = parts[3];
        } else {
            value = parts[2];
        }

        return CacheEntry::new(ttl, value);
    }
}

fn key_from_request(parts: &Vec<&str>) -> String {
    return parts[1].to_string();
}

fn keys_from_request(parts: &Vec<&str>) -> Vec<String> {
    let mut keys = Vec::new();
    for i in 1..parts.len() {
        keys.push(parts[i].to_string());
    }
    return keys;
}

/**
 * Handle DEL request
 */
fn handle_del(parts: &Vec<&str>, store: &mut HashMap<String, CacheEntry>, writer: &mut TcpStream) {
    let keys = keys_from_request(&parts);
    let mut deleted = 0;
    for key in keys {
        let result = store.remove(&key);
        if Option::is_some(&result) {
            deleted += 1;
        }
    }

    writeln!(writer, ":{}", deleted).ok();
}

/**
 * Handle SET request
 */
fn handle_set(parts: &Vec<&str>, store: &mut HashMap<String, CacheEntry>, writer: &mut TcpStream) {
    store.insert(
        key_from_request(&parts),
        CacheEntry::new_from_request(&parts),
    );
    writeln!(writer, "+OK").ok();
}

fn get_by_key<'a>(store: &'a HashMap<String, CacheEntry>, key: &str) -> Option<&'a CacheEntry> {
    match store.get(key) {
        Some(entry) => {
            if let Some(expiration) = entry.expires_at {
                if Instant::now() >= expiration {
                    return None;
                }
            }
            Some(entry)
        }
        None => None,
    }
}

/**
 * Handle GET request
 */
fn handle_get(parts: &Vec<&str>, store: &HashMap<String, CacheEntry>, writer: &mut TcpStream) {
    let key = &key_from_request(parts);
    match get_by_key(store, key) {
        Some(entry) => {
            writeln!(writer, "${}\r\n{}", entry.value.len(), entry.value).ok();
        }
        None => {
            writeln!(writer, "$-1").ok();
        }
    }
}

/**
 * Handle TTL request
 */
fn handle_ttl(parts: &Vec<&str>, store: &HashMap<String, CacheEntry>, writer: &mut TcpStream) {
    let key = &key_from_request(parts);
    match get_by_key(store, key) {
        Some(entry) => {
            if let Some(expiration) = entry.expires_at {
                let seconds_left = expiration
                    .checked_duration_since(Instant::now())
                    .unwrap()
                    .as_secs();
                writeln!(writer, "{}", seconds_left).ok();
            } else {
                writeln!(writer, "-1").ok();
            }
        }
        None => {
            writeln!(writer, "-1").ok();
        }
    }
}

fn main() -> std::io::Result<()> {
    let store = Arc::new(Mutex::new(HashMap::new()));

    // Bind the TCP listener to localhost:7878
    let listener = TcpListener::bind("127.0.0.1:7878")?;
    println!("Server listening on 127.0.0.1:7878");

    // Accept incoming connections in a loop
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let store = Arc::clone(&store);
                thread::spawn(move || handle_connection(stream, store));
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }

    Ok(())
}

fn periodic_cleanup(store: &Arc<RwLock<HashMap<String, CacheEntry>>>, interval: u64) {
    let store_for_cleanup = Arc::clone(&store);
    std::thread::spawn(move || {
        use std::time::Duration;
        loop {
            std::thread::sleep(Duration::from_secs(interval));

            let mut map = store_for_cleanup.write().unwrap();

            // Retain only non-expired entries
            map.retain(|_k, entry| {
                match entry.expires_at {
                    Some(exp) => Instant::now() < exp,
                    None => true, // no TTL
                }
            });
        }
    });
}

fn log(string: &str) {
    println!("{}", string);
}

fn check_authenticated(writer: &mut TcpStream, authendificated: bool) -> bool {
    if !authendificated {
        writeln!(writer, "-NOAUTH Authentication required.").ok();
    }

    return authendificated;
}

fn check_password(writer: &mut TcpStream, parts: &Vec<&str>) -> bool {
    if parts[1] == PASSWORD {
        writeln!(writer, "+OK").ok();
        return true;
    }

    writeln!(writer, "-ERR invalid password").ok();
    return false;
}

fn check_params(writer: &mut TcpStream, parts: &Vec<&str>, command: &str) -> bool {
    if parts.len() < 2 {
        writeln!(writer, "-ERR wrong number of arguments for '{}'", command).ok();
        return false;
    }
    return true;
}

fn handle_connection(stream: TcpStream, store: Arc<RwLock<HashMap<String, CacheEntry>>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut writer = stream;
    let mut authendificated = false;

    loop {
        let mut buffer = String::new();
        let bytes_read = match reader.read_line(&mut buffer) {
            Ok(n) => n,
            Err(_) => {
                log("Error reading client, closing connection");
                return;
            }
        };

        if bytes_read == 0 {
            log("Zero bytes, connection closed");
            return;
        }

        let trimmed = buffer.trim();
        if trimmed.is_empty() {
            log("Zero bytes when trimmed, connection closed");
            return;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.is_empty() {
            writeln!(writer, "-ERR empty command").ok();
        }

        let command = parts[0].to_uppercase();
        match command.as_str() {
            "AUTH" => {
                if !check_params(&mut writer, &parts, &command) {
                    continue;
                }
                authendificated = check_password(&mut writer, &parts);
            }

            "PING" => {
                writeln!(writer, "+PONG").ok();
            }

            "GET" => {
                if !check_authenticated(&mut writer, authendificated) {
                    continue;
                }
                let store = store.read().unwrap();
                handle_get(&parts, &store, &mut writer);
            }

            "TTL" => {
                if !check_authenticated(&mut writer, authendificated) {
                    continue;
                }
                let store = store.read().unwrap();
                handle_ttl(&parts, &store, &mut writer);
            }

            "SET" => {
                if !check_authenticated(&mut writer, authendificated) {
                    continue;
                }
                let mut store = store.write().unwrap();
                handle_set(&parts, &mut store, &mut writer);
            }

            "DEL" => {
                if !check_authenticated(&mut writer, authendificated) {
                    continue;
                }
                let mut store = store.write().unwrap();
                handle_del(&parts, &mut store, &mut writer);
            }

            "QUIT" => {
                writeln!(writer, "+OK").ok();
                return;
            }

            _ => {
                writeln!(writer, "-ERR: unknown command '{}'", &command).ok();
            }
        }
    }
}
