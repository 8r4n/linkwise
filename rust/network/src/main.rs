mod link;
mod node;
mod secure;
mod sim;
mod spool;
use anyhow::{Context, Result, bail, ensure};
use serde_json::json;
use spool::Station;
use std::{
    env, fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    time::Duration,
};
const USAGE: &str = "linkwise-network simulate six|N|global-N|scenario.json [seed] [runs]\n  scenario six|N|global-N\n  submit DB NODE DEST FILE TTL_SECONDS\n  list DB NODE | read DB NODE ID | prune DB NODE\n  send DB NODE ID LOOPBACK_ADDR | listen DB NODE LOOPBACK_ADDR\n  serve DB NODE PROFILE_JSON SECONDS (0 for continuous)\n  receive-stream DB NODE CONNECTED_DATA_ADDR";
fn scenario(arg: &str) -> Result<sim::Scenario> {
    if let Some(n) = arg.strip_prefix("global-") {
        return sim::global(n.parse()?);
    }
    if arg == "six" || arg == "SIX" {
        return Ok(sim::six());
    }
    if let Ok(n) = arg.parse() {
        return sim::generated(n);
    }
    let data = bounded(arg, 32 * 1024 * 1024)?;
    Ok(serde_json::from_slice(&data)?)
}
fn bounded(file: &str, limit: usize) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    fs::File::open(file)?
        .take(limit as u64 + 1)
        .read_to_end(&mut buf)?;
    ensure!(buf.len() <= limit, "input too large");
    Ok(buf)
}
fn addr(s: &str) -> Result<SocketAddr> {
    let a: SocketAddr = s.parse()?;
    ensure!(a.ip().is_loopback(), "local bench sockets only");
    Ok(a)
}
fn timed(stream: TcpStream) -> Result<TcpStream> {
    let t = Some(Duration::from_secs(120));
    stream.set_read_timeout(t)?;
    stream.set_write_timeout(t)?;
    stream.set_nodelay(true)?;
    Ok(stream)
}
fn connect(s: &str) -> Result<TcpStream> {
    timed(TcpStream::connect_timeout(
        &addr(s)?,
        Duration::from_secs(5),
    )?)
}
fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    let a: Vec<_> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["scenario", arg] => println!("{}", serde_json::to_string_pretty(&scenario(arg)?)?),
        ["simulate", arg, rest @ ..] if rest.len() <= 2 => {
            let s = scenario(arg)?;
            let seed = rest
                .first()
                .map(|s| s.parse::<u64>())
                .transpose()?
                .unwrap_or(1);
            let runs = rest
                .get(1)
                .map(|s| s.parse::<u64>())
                .transpose()?
                .unwrap_or(1);
            ensure!((1..=10000).contains(&runs), "invalid run count");
            for k in 0..runs {
                let seed = seed.checked_add(k).context("seed overflow")?;
                for adapt_routes in [false, true] {
                    for adapt_contacts in [false, true] {
                        let policy = sim::Policy {
                            adapt_contacts,
                            adapt_routes,
                        };
                        let results = sim::run(&s, policy, seed, s.nodes <= 6)?;
                        println!(
                            "{}",
                            json!({"schema":1,"nodes":s.nodes,"seed":seed,"policy":policy,"result":results})
                        );
                    }
                }
            }
        }
        [command, db, node, rest @ ..] => {
            let st = Station::open(Path::new(db), node)?;
            let now = bp7::dtn_time_now;
            match (*command, rest) {
                ("serve", [config, seconds]) => node::serve(&st, node, config, seconds.parse()?)?,
                ("export", [id]) => std::io::stdout().write_all(&st.outgoing(id, now())?)?,
                ("import", [file]) => {
                    println!("{}", st.receive(&bounded(file, spool::LIMIT)?, now())?)
                }
                ("submit", [dest, file, ttl]) => println!(
                    "{}",
                    st.submit(
                        dest,
                        &bounded(file, spool::LIMIT - 1024)?,
                        now(),
                        ttl.parse::<u64>()?
                            .checked_mul(1000)
                            .context("lifetime overflow")?
                    )?
                ),
                ("list", []) => println!("{}", serde_json::to_string_pretty(&st.list()?)?),
                ("prune", []) => println!("{}", st.prune(now())?),
                ("read", [id]) => std::io::stdout().write_all(&st.read_payload(id)?)?,
                ("send", [id, address]) => link::send(&st, id, &mut connect(address)?, now())?,
                ("listen", [address]) => {
                    let server = TcpListener::bind(addr(address)?)?;
                    println!("{}", server.local_addr()?);
                    std::io::stdout().flush()?;
                    println!(
                        "{}",
                        link::receive(&st, &mut timed(server.accept()?.0)?, now)?
                    );
                }
                ("receive-stream", [address]) => {
                    println!("{}", link::receive(&st, &mut connect(address)?, now)?);
                }
                _ => bail!(USAGE),
            }
        }
        _ => bail!(USAGE),
    };
    Ok(())
}
