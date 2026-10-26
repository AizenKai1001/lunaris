//! The server's admin console (docs/ADMIN.md): commands are lines of a text file that the server reads while it runs, so that an owner can act on a running server from another shell, a script, the supervisor or over ssh,
//! without a network port, a password or a protocol (whoever can write the file is the admin).
//!
//! `lunaris server ... --admin DIR` makes the folder and watches `DIR/commands.txt`: every line appended to it is run (a line is `COMMAND ARGUMENTS`; `#` starts a comment) and what the server says goes to
//! `DIR/output.log`. `DIR/bans.txt` keeps the bans (read when the server starts, written when they change). The commands:
//!
//! | command | does |
//! |---|---|
//! | `help` | lists the commands |
//! | `players` | who is in: slot, name, address, round trip |
//! | `status` | players in, tick, joins, refusals |
//! | `kick NAME [reason]` | removes a player (the client is told: "removed from the server") |
//! | `ban NAME [FOR]` | bans the name and the address of the player now in (the address only if a player of that name is in), and removes them; FOR (`90s`, `30m`, `2h`, `7d`) makes the ban run out, without it it is for good |
//! | `ban name NAME [FOR]` / `ban ip ADDRESS [FOR]` | bans only that name / only that address (an address bans every player behind it) |
//! | `unban NAME-OR-ADDRESS`, `bans` | lifts a ban; lists the bans |
//! | `say TEXT` | tells everybody, as `SERVER` in the chat |
//! | `mute NAME [seconds]`, `unmute NAME` | a muted player's chat is dropped (default 10 minutes) |
//! | `save` | the world is saved now |
//! | `wipe world yes`, `wipe all yes` | a fresh world (the characters kept, or gone with `all`), then the server stops (the game's own command, `SimGame`) |
//! | `stop` | the server saves and stops (what Ctrl+C does) |
//!
//! Anything else is offered to the game ([`Game::admin`]). A ban of a name is case-insensitive; a banned player who tries to join is refused with the reason "banned" (the handshake costs nothing before that).
use super::server::{unix_now, Bans, Game, NetServer};
use super::transport::Transport;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// The most of the command file read at once.
const CHUNK: u64 = 1 << 20;

/// The admin console of a running server: where its files are and how far into the command file it has read.
pub struct Admin {
    dir: PathBuf,
    read: u64,
    /// In the middle of a line that was too long to be a command: its end is skipped.
    skipping: bool,
    next_poll: u64,
    /// The server was asked to stop (`stop`).
    pub stop: bool,
}

impl Admin {
    /// Make the folder, read the bans in it into the server and begin watching `commands.txt` from its end (what is in it now was run by an earlier server).
    pub fn open<T: Transport, G: Game>(dir: &Path, server: &mut NetServer<T, G>) -> std::io::Result<Admin> {
        std::fs::create_dir_all(dir)?;
        let cmds = dir.join("commands.txt");
        if !cmds.exists() {
            std::fs::write(&cmds, "")?;
        }
        // (bytes that are not text are lost, the lines round them are not: a damaged ban file must not leave the server with no bans at all)
        if let Ok(bytes) = std::fs::read(dir.join("bans.txt")) {
            server.bans = Bans::parse(&String::from_utf8_lossy(&bytes));
        }
        let read = std::fs::metadata(&cmds).map(|m| m.len()).unwrap_or(0);
        Ok(Admin { dir: dir.to_path_buf(), read, skipping: false, next_poll: 0, stop: false })
    }

    /// Run the lines that have been appended since the last look, at most twice a second. `now` is the server's clock in milliseconds.
    pub fn poll<T: Transport, G: Game>(&mut self, server: &mut NetServer<T, G>, now: u64) {
        if now < self.next_poll {
            return;
        }
        self.next_poll = now + 500;
        let path = self.dir.join("commands.txt");
        let Ok(mut f) = std::fs::File::open(&path) else { return };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.read {
            self.read = 0; // (the file was emptied: begin again)
        }
        if len == self.read {
            return;
        }
        let mut buf = Vec::new();
        if f.seek(SeekFrom::Start(self.read)).is_err() || f.take(CHUNK).read_to_end(&mut buf).is_err() {
            return;
        }
        // (a line that is too long to be a command, or has bytes that are not text in it, must not stop the console for good: it is skipped, and the lines after it are run)
        let mut start = 0;
        if self.skipping {
            match buf.iter().position(|&b| b == b'\n') {
                Some(i) => {
                    start = i + 1;
                    self.skipping = false;
                }
                None => {
                    self.read += buf.len() as u64;
                    return;
                }
            }
        }
        // (only whole lines: a half-written one waits for its end)
        let whole = buf[start..].iter().rposition(|&b| b == b'\n').map_or(start, |i| start + i + 1);
        if whole == start && buf.len() as u64 >= CHUNK {
            self.skipping = true;
            self.read += buf.len() as u64;
            self.say("(a line of more than a megabyte was skipped)\n");
            return;
        }
        self.read += whole as u64;
        let before = server.bans.clone();
        for raw in buf[start..whole].split(|&b| b == b'\n') {
            let text = String::from_utf8_lossy(raw);
            let line = text.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let reply = self.run(server, line);
            let shown: String = line.chars().take(200).collect();
            self.say(&format!("> {shown}\n{reply}\n"));
        }
        if server.bans != before {
            // (written beside and renamed over it: a crash in the middle must not leave half a ban list)
            let path = self.dir.join("bans.txt");
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, server.bans.to_text()).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
        if server.game.wants_stop() {
            self.stop = true;
        }
    }

    fn say(&self, text: &str) {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join("output.log")) {
            let _ = f.write_all(text.as_bytes());
        }
    }

    /// Run one command and return what it says.
    pub fn run<T: Transport, G: Game>(&mut self, server: &mut NetServer<T, G>, line: &str) -> String {
        let (cmd, args) = line.split_once(char::is_whitespace).map_or((line, ""), |(c, a)| (c, a.trim()));
        match cmd.to_ascii_lowercase().as_str() {
            "help" => "help players status kick ban unban bans say mute unmute save wipe stop (docs/ADMIN.md)".to_string(),
            "players" => {
                let rows = server.players();
                if rows.is_empty() {
                    return "nobody is in".to_string();
                }
                rows.iter().map(|p| format!("{:>3} {:<24} {} {:.0} ms", p.slot, p.name, p.addr, p.rtt_ms)).collect::<Vec<_>>().join("\n")
            }
            "status" => format!("{} players in, tick {}, {} joins, {} refused, {} bans", server.client_count(), server.world.tick, server.stats.joins, server.stats.refused, server.bans.count(unix_now())),
            "kick" => match server.player_in(args) {
                Some((slot, _)) => {
                    let name = server.name_of(slot).unwrap_or("").to_string();
                    server.kick(slot, super::server::REFUSED_KICKED);
                    format!("{name} was removed")
                }
                None => format!("no player called {}", short(args.split_whitespace().next().unwrap_or(""))),
            },
            "ban" => {
                // `ban ip ADDRESS [FOR]`, `ban name NAME [FOR]`, `ban NAME [FOR]` (the name and the address of a player that is in): FOR is 90s, 30m, 2h or 7d; without it the ban is for good.
                // A name may have spaces in it: the longest name of a player that is in which `ban` is followed by is the player (`ban Big Bob 2h`), and in `ban name NAME FOR` a last word that is a time is FOR.
                let usage = "ban NAME [FOR], ban name NAME [FOR] or ban ip ADDRESS [FOR]: FOR is 90s, 30m, 2h or 7d (without it the ban is for good)";
                let mut w = args.split_whitespace();
                let (target, kind, slot, for_tok): (String, &str, Option<usize>, Option<String>) = match w.next() {
                    Some("ip") => match w.next() {
                        Some(a) => (a.to_string(), "ip", None, w.next().map(str::to_string)),
                        None => return usage.to_string(),
                    },
                    Some("name") => {
                        let rest: Vec<&str> = w.collect();
                        match rest.split_last() {
                            // (a last word that starts with a digit is meant as a time: if it is not one, that is said below)
                            Some((last, init)) if !init.is_empty() && last.starts_with(|c: char| c.is_ascii_digit()) => (init.join(" "), "name", None, Some(last.to_string())),
                            Some(_) => (rest.join(" "), "name", None, None),
                            None => return usage.to_string(),
                        }
                    }
                    Some(first) => match server.player_in(args) {
                        Some((slot, rest)) => (server.name_of(slot).unwrap_or("").to_string(), "player", Some(slot), rest.split_whitespace().next().map(str::to_string)),
                        None => (first.to_string(), "player", None, w.next().map(str::to_string)),
                    },
                    None => return usage.to_string(),
                };
                let secs = match &for_tok {
                    Some(t) => match duration(t) {
                        Some(d) => Some(d),
                        None => return format!("{} is not a time: 90s, 30m, 2h or 7d", short(t)),
                    },
                    None => None,
                };
                let until = secs.map(|d| unix_now().saturating_add(d));
                let how = secs.map_or(String::new(), |d| format!(" for {}", lasts(d)));
                match kind {
                    "ip" => match target.parse::<std::net::IpAddr>() {
                        Ok(ip) => {
                            ban_ip(server, ip, until);
                            let gone: Vec<usize> = server.players().iter().filter(|p| p.addr.ip() == ip).map(|p| p.slot).collect();
                            for s in &gone {
                                server.kick(*s, super::server::REFUSED_BANNED);
                            }
                            format!("{ip} is banned{how} ({} removed)", gone.len())
                        }
                        Err(_) => format!("{} is not an address", short(&target)),
                    },
                    "name" => {
                        ban_name(server, &target, until);
                        match server.slot_of_name(&target) {
                            Some(s) => {
                                server.kick(s, super::server::REFUSED_BANNED);
                                format!("{target} is banned{how} and was removed")
                            }
                            None => format!("{target} is banned{how}"),
                        }
                    }
                    _ => match slot {
                        Some(s) => {
                            let ip = server.players().iter().find(|p| p.slot == s).map(|p| p.addr.ip());
                            ban_name(server, &target, until);
                            if let Some(ip) = ip {
                                ban_ip(server, ip, until);
                            }
                            server.kick(s, super::server::REFUSED_BANNED);
                            format!("{target} is banned{how} (name and address {}) and was removed", ip.map_or("?".to_string(), |i| i.to_string()))
                        }
                        None => format!("no player called {target}: use `ban name {target}` to ban the name alone"),
                    },
                }
            }
            "unban" => {
                // (the whole of what follows: a name may have spaces in it)
                let a = args.trim();
                let lower = a.to_lowercase();
                let by_name = server.bans.names.remove(&lower) | server.bans.timed_names.remove(&lower).is_some();
                let by_ip = a.parse::<std::net::IpAddr>().is_ok_and(|ip| server.bans.ips.remove(&ip) | server.bans.timed_ips.remove(&ip).is_some());
                if by_name || by_ip {
                    format!("{a} is not banned any more")
                } else {
                    format!("{a} was not banned")
                }
            }
            "bans" => {
                let now = unix_now();
                let mut v: Vec<String> = server.bans.names.iter().map(|n| format!("name {n}")).collect();
                v.extend(server.bans.ips.iter().map(|i| format!("ip {i}")));
                v.extend(server.bans.timed_names.iter().filter(|(_, t)| **t > now).map(|(n, t)| format!("name {n} ({} left)", lasts(t - now))));
                v.extend(server.bans.timed_ips.iter().filter(|(_, t)| **t > now).map(|(i, t)| format!("ip {i} ({} left)", lasts(t - now))));
                if v.is_empty() {
                    "no bans".to_string()
                } else {
                    v.join("\n")
                }
            }
            "stop" => {
                self.stop = true;
                "stopping: the world is saved first".to_string()
            }
            _ => server.admin_game(&cmd.to_ascii_lowercase(), args).unwrap_or_else(|| format!("unknown command `{}` (help lists them)", short(cmd))),
        }
    }
}

/// The first 40 characters of what was typed: what an answer repeats of a command, so that a line of a megabyte is not echoed back whole.
fn short(s: &str) -> String {
    s.chars().take(40).collect()
}

/// A time like `90s`, `30m`, `2h` or `7d`, in seconds.
fn duration(tok: &str) -> Option<u64> {
    let (n, unit) = tok.split_at(tok.len().checked_sub(1)?);
    let n: u64 = n.parse().ok()?;
    let k = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return None,
    };
    n.checked_mul(k).filter(|s| *s > 0)
}

/// A number of seconds as a person says it: `45 s`, `30 m`, `2 h`, `7 d` (the biggest unit it fills at least once).
fn lasts(secs: u64) -> String {
    match secs {
        0..=119 => format!("{secs} s"),
        120..=7199 => format!("{} m", secs / 60),
        7200..=172799 => format!("{} h", secs / 3600),
        _ => format!("{} d", secs / 86400),
    }
}

/// Ban an address for good (`until` None) or until a unix second; a ban for good replaces a timed one, and a timed one never shortens a ban for good.
fn ban_ip<T: Transport, G: Game>(server: &mut NetServer<T, G>, ip: std::net::IpAddr, until: Option<u64>) {
    match until {
        None => {
            server.bans.timed_ips.remove(&ip);
            server.bans.ips.insert(ip);
        }
        Some(t) if !server.bans.ips.contains(&ip) => {
            server.bans.timed_ips.insert(ip, t);
        }
        Some(_) => {}
    }
}

fn ban_name<T: Transport, G: Game>(server: &mut NetServer<T, G>, name: &str, until: Option<u64>) {
    let name = name.to_lowercase();
    match until {
        None => {
            server.bans.timed_names.remove(&name);
            server.bans.names.insert(name);
        }
        Some(t) if !server.bans.names.contains(&name) => {
            server.bans.timed_names.insert(name, t);
        }
        Some(_) => {}
    }
}
