use std::convert::TryInto;

pub const FH6_PACKET_SIZE: usize = 324;

#[derive(Clone, Debug, Default)]
pub struct Telemetry {
    pub is_race_on: i32,
    pub session_timestamp: u32,
    pub engine_max_rpm: f32,
    pub idle_rpm: f32,
    pub current_rpm: f32,
    pub accel_x: f32, pub accel_y: f32, pub accel_z: f32,
    pub vel_x: f32, pub vel_y: f32, pub vel_z: f32,
    pub ang_vel_x: f32, pub ang_vel_y: f32, pub ang_vel_z: f32,
    pub speed_ms: f32, pub power_w: f32, pub torque_nm: f32,
    pub boost_raw: f32,
    pub car_ordinal: i32, pub car_class: i32, pub pi: i32,
    pub drivetrain: i32, pub num_cylinders: i32,
    pub accel_raw: u8, pub brake_raw: u8, pub clutch_raw: u8, pub handbrake_raw: u8, pub gear: u8,
    pub slip_fl: f32, pub slip_fr: f32, pub slip_rl: f32, pub slip_rr: f32,
}
impl Telemetry {
    fn core_values_finite(&self) -> bool {
        [self.engine_max_rpm, self.idle_rpm, self.current_rpm,
         self.accel_x, self.accel_y, self.accel_z,
         self.vel_x, self.vel_y, self.vel_z,
         self.speed_ms, self.power_w, self.torque_nm, self.boost_raw,
         self.slip_fl, self.slip_fr, self.slip_rl, self.slip_rr]
            .iter().all(|v| v.is_finite())
    }
    fn f32_at(b: &[u8], o: usize) -> Option<f32> {
        Some(f32::from_le_bytes(b.get(o..o+4)?.try_into().ok()?))
    }
    fn i32_at(b: &[u8], o: usize) -> Option<i32> {
        Some(i32::from_le_bytes(b.get(o..o+4)?.try_into().ok()?))
    }
    fn u32_at(b: &[u8], o: usize) -> Option<u32> {
        Some(u32::from_le_bytes(b.get(o..o+4)?.try_into().ok()?))
    }
    pub fn parse(b: &[u8]) -> Option<Self> {
        if b.len() < FH6_PACKET_SIZE { return None; }
        let mut t = Self::default();
        t.is_race_on = Self::i32_at(b,0)?;
        t.session_timestamp = Self::u32_at(b,4)?;
        t.engine_max_rpm = Self::f32_at(b,8)?;
        t.idle_rpm = Self::f32_at(b,12)?;
        t.current_rpm = Self::f32_at(b,16)?;
        t.accel_x=Self::f32_at(b,20)?; t.accel_y=Self::f32_at(b,24)?; t.accel_z=Self::f32_at(b,28)?;
        t.vel_x=Self::f32_at(b,32)?; t.vel_y=Self::f32_at(b,36)?; t.vel_z=Self::f32_at(b,40)?;
        t.ang_vel_x=Self::f32_at(b,44)?; t.ang_vel_y=Self::f32_at(b,48)?; t.ang_vel_z=Self::f32_at(b,52)?;
        t.car_ordinal=Self::i32_at(b,212)?; t.car_class=Self::i32_at(b,216)?;
        t.pi=Self::i32_at(b,220)?; t.drivetrain=Self::i32_at(b,224)?;
        t.num_cylinders=Self::i32_at(b,228)?;
        t.speed_ms=Self::f32_at(b,256)?; t.power_w=Self::f32_at(b,260)?;
        t.torque_nm=Self::f32_at(b,264)?; t.boost_raw=Self::f32_at(b,284)?;
        t.slip_fl=Self::f32_at(b,84)?; t.slip_fr=Self::f32_at(b,88)?;
        t.slip_rl=Self::f32_at(b,92)?; t.slip_rr=Self::f32_at(b,96)?;
        t.accel_raw=b[315]; t.brake_raw=b[316]; t.clutch_raw=b[317]; t.handbrake_raw=b[318]; t.gear=b[319];
        if !t.core_values_finite() { return None; }
        Some(t)
    }
    pub fn speed_kmh(&self) -> f32 {
        let s = if self.speed_ms >= 0.0 && self.speed_ms < 200.0 { self.speed_ms }
            else { (self.vel_x*self.vel_x+self.vel_y*self.vel_y+self.vel_z*self.vel_z).sqrt() };
        s * 3.6
    }
    pub fn rpm_pct(&self) -> f32 {
        if self.engine_max_rpm > 0.0 { self.current_rpm / self.engine_max_rpm } else { 0.0 }
    }
    pub fn throttle(&self) -> f32 { self.accel_raw as f32 / 255.0 }
    pub fn brake(&self) -> f32 { self.brake_raw as f32 / 255.0 }
    pub fn handbrake(&self) -> f32 { self.handbrake_raw as f32 / 255.0 }
    pub fn rear_slip(&self) -> f32 { self.slip_rl.abs().max(self.slip_rr.abs()) }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_handbrake_from_packet_byte_318() {
        let mut packet = [0u8; FH6_PACKET_SIZE];
        packet[0..4].copy_from_slice(&1i32.to_le_bytes());
        packet[8..12].copy_from_slice(&9000.0f32.to_le_bytes());
        packet[12..16].copy_from_slice(&900.0f32.to_le_bytes());
        packet[16..20].copy_from_slice(&5000.0f32.to_le_bytes());
        packet[256..260].copy_from_slice(&25.0f32.to_le_bytes());
        packet[318] = 255;
        packet[319] = 4;

        let telemetry = Telemetry::parse(&packet).expect("valid FH6 packet");
        assert_eq!(telemetry.gear, 4);
        assert_eq!(telemetry.handbrake_raw, 255);
        assert_eq!(telemetry.handbrake(), 1.0);
    }
}
