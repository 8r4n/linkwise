//! Independently decode a Linkwise BP packet and generate a reply fixture.
use hardy_bpv7::{
    builder::Builder,
    bundle::{Flags, ParsedBundle},
    creation_timestamp::CreationTimestamp,
    hop_info::HopInfo,
};
fn main() {
    let arg: Vec<_> = std::env::args().collect();
    let wire = std::fs::read(&arg[1]).unwrap();
    let parsed = ParsedBundle::parse(&wire, hardy_bpv7::bpsec::no_keys).unwrap();
    assert_eq!(parsed.bundle.destination.to_string(), "dtn://B/inbox");
    let (_, reply) = Builder::new(
        "dtn://B/inbox".parse().unwrap(),
        "dtn://A/inbox".parse().unwrap(),
    )
    .with_flags(Flags {
        do_not_fragment: true,
        ..Default::default()
    })
    .with_hop_count(&HopInfo {
        limit: 32,
        count: 1,
    })
    .with_payload(vec![0, 0, 255, 10, 13].into())
    .build(CreationTimestamp::now())
    .unwrap();
    std::fs::write(&arg[2], reply).unwrap();
}
