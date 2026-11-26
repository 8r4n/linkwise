//! Each commissioned node polls neighbors in its assigned cyclic slot. Current
//! reachability is learned in authenticated sessions. Routes expire after silence.
use crate::{addr, bounded, link, secure, spool};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use spool::Station;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Peer {
    name: String,
    address: String,
    key: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    listen: String,
    slot: u64,
    slots: u64,
    slot_ms: u64,
    peers: Vec<Peer>,
    control_socket: Option<PathBuf>,
}
type Routes = BTreeMap<String, Vec<String>>;
#[derive(Serialize, Deserialize)]
struct Advertisement {
    routes: Routes,
}
struct View {
    seen: Instant,
    routes: Routes,
}
struct Router<'a> {
    station: &'a Station,
    name: &'a str,
    views: BTreeMap<String, View>,
    max_age: Duration,
}
impl Router<'_> {
    fn paths(&self, exclude: Option<&str>) -> Routes {
        let mut paths = Routes::from([(self.name.into(), vec![self.name.into()])]);
        for (peer, view) in &self.views {
            if view.seen.elapsed() > self.max_age || Some(peer.as_str()) == exclude {
                continue;
            }
            for (dest, path) in &view.routes {
                if path.contains(&self.name.to_owned())
                    || exclude.is_some_and(|x| path.iter().any(|n| n == x))
                    || path.len() >= 16
                {
                    continue;
                }
                let mut candidate = vec![self.name.into()];
                candidate.extend(path.iter().cloned());
                if paths
                    .get(dest)
                    .is_none_or(|old| (candidate.len(), &candidate) < (old.len(), old))
                {
                    paths.insert(dest.clone(), candidate);
                }
            }
        }
        paths
    }
    fn exchange(&mut self, peer: &Peer, raw: TcpStream, caller: bool) -> Result<()> {
        let (src, dst) = if caller {
            (self.name, peer.name.as_str())
        } else {
            (peer.name.as_str(), self.name)
        };
        let mut stream = secure::Stream::new(raw, &secure::key(&peer.key)?, caller, src, dst)?;
        let write_adv = |s: &mut secure::Stream| -> Result<()> {
            let data = serde_json::to_vec(&Advertisement {
                routes: self.paths(Some(&peer.name)),
            })?;
            ensure!(data.len() <= 65536, "advertisement budget");
            s.write_all(&(data.len() as u32).to_be_bytes())?;
            s.write_all(&data)?;
            Ok(())
        };
        let read_adv = |s: &mut secure::Stream| -> Result<Routes> {
            let mut h = [0; 4];
            s.read_exact(&mut h)?;
            let n = u32::from_be_bytes(h) as usize;
            ensure!(n <= 65536, "oversize advertisement");
            let mut data = vec![0; n];
            s.read_exact(&mut data)?;
            let a: Advertisement = serde_json::from_slice(&data)?;
            ensure!(
                a.routes.len() <= 1024
                    && a.routes.get(&peer.name) == Some(&vec![peer.name.clone()]),
                "missing peer origin"
            );
            for (dst, path) in &a.routes {
                ensure!(
                    path.first() == Some(&peer.name)
                        && path.last() == Some(dst)
                        && path.len() <= 16
                        && path.iter().collect::<BTreeSet<_>>().len() == path.len(),
                    "invalid path"
                );
                for n in path {
                    spool::endpoint(n)?;
                }
            }
            Ok(a.routes)
        };
        let routes = if caller {
            write_adv(&mut stream)?;
            read_adv(&mut stream)?
        } else {
            let routes = read_adv(&mut stream)?;
            write_adv(&mut stream)?;
            routes
        };
        self.views.insert(
            peer.name.clone(),
            View {
                seen: Instant::now(),
                routes,
            },
        );
        let now = bp7::dtn_time_now();
        if caller {
            let paths = self.paths(None);
            let selected = self
                .station
                .list()?
                .into_iter()
                .filter(|r| {
                    r.state == "queued"
                        && now < r.expires
                        && paths.get(&r.destination).and_then(|v| v.get(1)) == Some(&peer.name)
                })
                .min_by_key(|r| (r.expires, r.id.clone()));
            stream.write_all(&[selected.is_some() as u8])?;
            if let Some(row) = selected {
                link::send(self.station, &row.id, &mut stream, bp7::dtn_time_now)?;
                println!("forwarded {} to {}", row.id, peer.name);
            }
        } else {
            let mut flag = [0];
            stream.read_exact(&mut flag)?;
            ensure!(flag[0] <= 1, "unknown operation");
            if flag[0] == 1 {
                let id = link::receive(self.station, &mut stream, bp7::dtn_time_now)?;
                println!("accepted {id} from {}", peer.name);
            }
        }
        Ok(())
    }
}
fn setup(socket: &TcpStream, ms: u64) -> Result<()> {
    socket.set_nonblocking(false)?;
    let t = Some(Duration::from_millis(ms));
    socket.set_read_timeout(t)?;
    socket.set_write_timeout(t)?;
    socket.set_nodelay(true)?;
    Ok(())
}
pub fn serve(st: &Station, name: &str, config: &str, seconds: u64) -> Result<()> {
    let cfg: Config = serde_json::from_slice(&bounded(config, 256 * 1024)?)?;
    ensure!(
        (2..=1024).contains(&cfg.slots)
            && cfg.slot < cfg.slots
            && (100..=60000).contains(&cfg.slot_ms)
            && !cfg.peers.is_empty()
            && cfg.peers.len() <= 64,
        "invalid rendezvous profile"
    );
    let mut unique = BTreeSet::new();
    for p in &cfg.peers {
        spool::endpoint(&p.name)?;
        secure::key(&p.key)?;
        addr(&p.address)?;
        ensure!(p.name != name && unique.insert(&p.name), "duplicate peer");
    }
    let listener = TcpListener::bind(addr(&cfg.listen)?)?;
    listener.set_nonblocking(true)?;
    st.prune(bp7::dtn_time_now)?;
    let run = || run(st, name, &cfg, listener, seconds);
    #[cfg(unix)]
    if let Some(socket) = &cfg.control_socket {
        return crate::control::with_listener(socket, st, run);
    }
    ensure!(cfg.control_socket.is_none(), "queue socket requires Unix");
    run()
}

fn run(st: &Station, name: &str, cfg: &Config, listener: TcpListener, seconds: u64) -> Result<()> {
    println!("listening {}", listener.local_addr()?);
    std::io::stdout().flush()?;
    let mut router = Router {
        station: st,
        name,
        views: BTreeMap::new(),
        max_age: Duration::from_millis(cfg.slot_ms * cfg.slots * cfg.peers.len() as u64 * 4),
    };
    let start = Instant::now();
    let mut previous = None;
    let mut last_gc = Instant::now();
    while seconds == 0 || start.elapsed() < Duration::from_secs(seconds) {
        if last_gc.elapsed() >= Duration::from_secs(60) {
            st.prune(bp7::dtn_time_now)?;
            last_gc = Instant::now();
        }
        if let Ok((mut socket, _)) = listener.accept() {
            let result = (|| -> Result<()> {
                setup(&socket, cfg.slot_ms)?;
                let mut h = [0; 1];
                socket.read_exact(&mut h)?;
                ensure!(h[0] <= 32, "invalid peer name");
                let mut n = vec![0; h[0] as usize];
                socket.read_exact(&mut n)?;
                let peer = cfg
                    .peers
                    .iter()
                    .find(|p| p.name.as_bytes() == n)
                    .context("unknown peer")?;
                router.exchange(peer, socket, false)
            })();
            if let Err(e) = result {
                eprintln!("incoming contact: {e}")
            }
        }
        let tick = bp7::dtn_time_now() / cfg.slot_ms;
        if tick % cfg.slots == cfg.slot && previous != Some(tick) {
            previous = Some(tick);
            let cycle = tick / cfg.slots;
            // Reserve every other call for discovery so queued traffic cannot
            // permanently starve a newly reachable neighbor.
            let fallback = &cfg.peers[(cycle.div_ceil(2) % cfg.peers.len() as u64) as usize];
            let paths = router.paths(None);
            let now = bp7::dtn_time_now();
            let preferred = if cycle % 2 == 1 {
                st.list()?
                    .into_iter()
                    .filter(|r| r.state == "queued" && r.expires > now)
                    .min_by_key(|r| (!paths.contains_key(&r.destination), r.expires))
                    .and_then(|r| paths.get(&r.destination))
                    .and_then(|path| path.get(1))
                    .and_then(|next| cfg.peers.iter().find(|p| &p.name == next))
            } else {
                None
            };
            let p = preferred.unwrap_or(fallback);
            let result = (|| -> Result<()> {
                let mut s = TcpStream::connect_timeout(
                    &addr(&p.address)?,
                    Duration::from_millis(cfg.slot_ms / 2),
                )?;
                setup(&s, cfg.slot_ms)?;
                s.write_all(&[name.len() as u8])?;
                s.write_all(name.as_bytes())?;
                router.exchange(p, s, true)
            })();
            if let Err(e) = result {
                router.views.remove(&p.name);
                eprintln!("contact {}: {e}", p.name)
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
