//! `devices-enroll SERVER [--apk PATH] [--user NAME] [--serial SERIAL]`: a
//! phone plugged into this laptop made devices' device owner and signed in
//! (devices_server::enroll says each step).

fn main() -> anyhow::Result<()> {
    let e = devices_server::enroll::Enroll::parse(std::env::args().skip(1))?;
    devices_server::enroll::enroll(&e)
}
