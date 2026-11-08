use fh6_virtual_tcu_rust::telemetry::{Telemetry, FH6_PACKET_SIZE};

#[test]
fn rejects_short_packets() {
    assert!(Telemetry::parse(&[0u8; FH6_PACKET_SIZE - 1]).is_none());
}
