pub fn greet(name: &str) -> String {
    format!("hi {name}")
}

pub struct Config {
    pub host: String,
}

pub enum Mode {
    On,
    Off,
}

pub trait Wakeup {
    fn wake(&self);
}

fn private_helper() {
    let _ = 1;
}
